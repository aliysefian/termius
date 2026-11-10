//! One tmux manager for the whole app. It owns *when* a host's tmux is read, never *how* a host is reached (that is
//! [`HostExec`], the connection the monitoring engine already keeps), and it knows nothing about terminal tabs.
//!
//! * **Shared.** A snapshot of one host and socket is read once and served to every view. Callers that arrive while a
//!   reading is under way wait for it. At most `max_concurrent` readings run at once, however many hosts there are.
//! * **Ordered.** Every accepted reading gets a higher `generation`. A view keeps only the highest it has seen, so a slow
//!   answer that arrives late cannot overwrite a newer one.
//! * **Honest about failure.** When a reading fails the last good snapshot is kept and marked stale with the reason; it is
//!   never replaced by an empty list. A failing host is not asked again until a back-off has passed.
//! * **One poll loop per host and socket.** Views *watch* a host under their own name; the loop runs while anyone watches
//!   and stops when the last watcher leaves, so a hidden view costs nothing and two views cost one loop.
//! * **Verified.** A mutation runs against the server identity of the snapshot it was chosen from, is followed by a fresh
//!   reading, and succeeds only if that reading shows the change.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::hostmetrics::manager::HostExec;
use crate::hostmetrics::procs;

use super::model::{valid_pane_id, valid_session_id, HostSnapshot, ServerState, Socket, Stale};
use super::ops::{verify, Op};
use super::parse::{parse_capture, parse_operation, parse_probe, Captured, OpOutcome, PaneInfo, ParseError};
use super::resources::{self, Resources};
use super::script::{self, CaptureOpts};
use super::search::{self, Hit, Matcher, Query};

#[derive(Debug, Clone)]
pub struct Config {
    /// Readings of one host are never closer than this.
    pub min_interval: Duration,
    pub timeout: Duration,
    pub max_concurrent: usize,
    pub backoff_max: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { min_interval: Duration::from_millis(800), timeout: Duration::from_secs(12), max_concurrent: 4, backoff_max: Duration::from_secs(60) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TmuxError {
    /// Reaching the host failed (login, connection, a command that would not run).
    Exec { message: String },
    Timeout,
    /// The host answered with something that is not tmux's output.
    Unreadable { message: String },
    /// Refused before it was sent.
    Invalid { message: String },
    /// tmux ran the command and said no.
    Rejected { message: String },
    /// The server that answered is not the one the ids came from: refresh and choose again.
    ServerChanged,
    Cancelled,
}

impl std::fmt::Display for TmuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmuxError::Exec { message } | TmuxError::Unreadable { message } | TmuxError::Invalid { message } | TmuxError::Rejected { message } => write!(f, "{message}"),
            TmuxError::Timeout => write!(f, "the host did not answer in time"),
            TmuxError::ServerChanged => write!(f, "the tmux server on this host has restarted; refresh and choose again"),
            TmuxError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl From<ParseError> for TmuxError {
    fn from(e: ParseError) -> Self {
        TmuxError::Unreadable { message: e.to_string() }
    }
}

/// Told when a host's snapshot actually changed (not on every reading).
pub trait TmuxSink: Send + Sync + 'static {
    fn changed(&self, snapshot: &Arc<HostSnapshot>);
}

type Key = (Uuid, Socket);

#[derive(Default)]
struct Data {
    generation: u64,
    last: Option<Arc<HostSnapshot>>,
    read_at: Option<Instant>,
    failures: u32,
    last_failure: Option<(Instant, String)>,
}

struct Slot {
    gate: tokio::sync::Mutex<()>,
    data: Mutex<Data>,
}

struct Watch {
    subscribers: HashMap<String, Duration>,
    stop: CancellationToken,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub struct TmuxManager<E: HostExec> {
    exec: Arc<E>,
    cfg: Config,
    slots: Mutex<HashMap<Key, Arc<Slot>>>,
    permits: Arc<Semaphore>,
    sink: Option<Arc<dyn TmuxSink>>,
    watches: Mutex<HashMap<Key, Watch>>,
    procs: Mutex<HashMap<Uuid, procs::Snap>>,
    searches: Mutex<HashMap<String, (u64, CancellationToken)>>,
    search_serial: AtomicU64,
    /// How many scripts were sent to hosts, for tests and diagnostics.
    runs: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpResult {
    /// The id a creating operation made (`$N`, `@N` or `%N`).
    pub created: Option<String>,
    /// The reading taken after the operation, which `verify` accepted.
    pub snapshot: HostSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneCapture {
    pub info: PaneInfo2,
    pub text: String,
    pub taken_at_ms: u64,
}

/// [`PaneInfo`] in a form that serialises with the rest of the API.
#[derive(Debug, Clone, Serialize)]
pub struct PaneInfo2 {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub command: String,
    pub dead: bool,
    pub alternate: bool,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub in_mode: bool,
    pub history: u32,
    pub pid: Option<u32>,
    pub title: String,
}

impl From<PaneInfo> for PaneInfo2 {
    fn from(p: PaneInfo) -> Self {
        Self { id: p.id, width: p.width, height: p.height, command: p.command, dead: p.dead, alternate: p.alternate, cursor_x: p.cursor_x, cursor_y: p.cursor_y, in_mode: p.in_mode, history: p.history, pid: p.pid, title: p.title }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneHits {
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchReport {
    pub results: Vec<PaneHits>,
    /// Panes whose text could not be read, with the reason.
    pub failed: Vec<(String, String)>,
    pub searched: u32,
    /// The search stopped early because it reached its limit.
    pub limited: bool,
    pub cancelled: bool,
}

/// A pane to search, as the snapshot named it.
#[derive(Debug, Clone)]
pub struct SearchTarget {
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
}

pub const SEARCH_MAX_PANES: usize = 200;
pub const SEARCH_MAX_HITS_PER_PANE: usize = 50;
pub const SEARCH_MAX_HITS: usize = 500;

impl<E: HostExec> TmuxManager<E> {
    pub fn new(exec: Arc<E>, cfg: Config, sink: Option<Arc<dyn TmuxSink>>) -> Self {
        let permits = Arc::new(Semaphore::new(cfg.max_concurrent.max(1)));
        Self { exec, cfg, slots: Mutex::new(HashMap::new()), permits, sink, watches: Mutex::new(HashMap::new()), procs: Mutex::new(HashMap::new()), searches: Mutex::new(HashMap::new()), search_serial: AtomicU64::new(0), runs: AtomicU64::new(0) }
    }

    pub fn runs(&self) -> u64 {
        self.runs.load(Ordering::Relaxed)
    }

    fn slot(&self, key: &Key) -> Arc<Slot> {
        Arc::clone(lock(&self.slots).entry(key.clone()).or_insert_with(|| Arc::new(Slot { gate: tokio::sync::Mutex::new(()), data: Mutex::new(Data::default()) })))
    }

    /// Run a script on the host: counted, bounded by the timeout and the concurrency limit.
    async fn run(&self, host: Uuid, script: &str, cancel: Option<&CancellationToken>) -> Result<String, TmuxError> {
        let never = CancellationToken::new();
        let cancel = cancel.unwrap_or(&never);
        let _permit = tokio::select! {
            p = Arc::clone(&self.permits).acquire_owned() => p.map_err(|_| TmuxError::Cancelled)?,
            _ = cancel.cancelled() => return Err(TmuxError::Cancelled),
        };
        self.runs.fetch_add(1, Ordering::Relaxed);
        tokio::select! {
            r = tokio::time::timeout(self.cfg.timeout, self.exec.exec(host, script, self.cfg.timeout)) => match r {
                Ok(Ok(out)) => Ok(out),
                Ok(Err(message)) => Err(TmuxError::Exec { message }),
                Err(_) => Err(TmuxError::Timeout),
            },
            _ = cancel.cancelled() => Err(TmuxError::Cancelled),
        }
    }

    /// The last snapshot, whatever its age. Never connects.
    pub fn latest(&self, host: Uuid, socket: &Socket) -> Option<Arc<HostSnapshot>> {
        let slot = lock(&self.slots).get(&(host, socket.clone())).cloned()?;
        let d = lock(&slot.data);
        d.last.clone()
    }

    /// Every host's last snapshot (for the overview). Never connects.
    pub fn all_latest(&self) -> Vec<Arc<HostSnapshot>> {
        let slots: Vec<Arc<Slot>> = lock(&self.slots).values().cloned().collect();
        let mut out: Vec<Arc<HostSnapshot>> = slots.iter().filter_map(|s| lock(&s.data).last.clone()).collect();
        out.sort_by_key(|a| (a.host_id, a.socket.key()));
        out
    }

    fn backoff(&self, d: &Data) -> Option<Duration> {
        let (at, _) = d.last_failure.as_ref()?;
        let wait = Duration::from_secs(2).saturating_mul(1u32 << d.failures.saturating_sub(1).min(10)).min(self.cfg.backoff_max);
        let waited = at.elapsed();
        (waited < wait).then(|| wait - waited)
    }

    /// A snapshot no older than `max_age`. Reads the host if there is none, or waits for the reading already under way.
    /// A failed reading with an earlier good one returns that one, marked stale; only a host never read successfully
    /// answers with the error.
    pub async fn snapshot(&self, host: Uuid, socket: &Socket, max_age: Duration) -> Result<Arc<HostSnapshot>, TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        let key = (host, socket.clone());
        let slot = self.slot(&key);
        let max_age = max_age.max(self.cfg.min_interval);
        let fresh = |slot: &Slot| {
            let d = lock(&slot.data);
            match (&d.last, d.read_at) {
                (Some(s), Some(at)) if at.elapsed() < max_age => Some(Arc::clone(s)),
                _ => None,
            }
        };
        if let Some(s) = fresh(&slot) {
            return Ok(s);
        }
        let _gate = slot.gate.lock().await;
        if let Some(s) = fresh(&slot) {
            return Ok(s);
        }
        // A host that is failing is not asked again yet: what was last known stays on screen.
        let cooling = {
            let d = lock(&slot.data);
            self.backoff(&d).and_then(|_| d.last.clone())
        };
        if let Some(s) = cooling {
            return Ok(s);
        }
        self.read(&key, &slot, None).await
    }

    /// Read the host now, whatever the cache holds (after a change, or when the user asks).
    pub async fn refresh(&self, host: Uuid, socket: &Socket) -> Result<Arc<HostSnapshot>, TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        let key = (host, socket.clone());
        let slot = self.slot(&key);
        let _gate = slot.gate.lock().await;
        self.read(&key, &slot, None).await
    }

    /// One reading under the slot's gate.
    async fn read(&self, key: &Key, slot: &Slot, cancel: Option<&CancellationToken>) -> Result<Arc<HostSnapshot>, TmuxError> {
        let nonce = script::nonce();
        let script = script::probe(&key.1, &nonce);
        let outcome = match self.run(key.0, &script, cancel).await {
            Ok(out) => parse_probe(&out, &nonce, &key.1).map_err(TmuxError::from),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(p) => {
                let mut d = lock(&slot.data);
                d.failures = 0;
                d.last_failure = None;
                d.generation += 1;
                let snap = Arc::new(HostSnapshot {
                    host_id: key.0,
                    socket: key.1.clone(),
                    generation: d.generation,
                    taken_at_ms: now_ms(),
                    version: p.version,
                    state: p.state,
                    server: p.server,
                    sessions: p.sessions,
                    other_sockets: p.other_sockets,
                    stale: None,
                });
                let changed = d.last.as_ref().is_none_or(|old| differs(old, &snap));
                d.last = Some(Arc::clone(&snap));
                d.read_at = Some(Instant::now());
                drop(d);
                if changed {
                    if let Some(sink) = &self.sink {
                        sink.changed(&snap);
                    }
                }
                Ok(snap)
            }
            Err(e) => {
                if e == TmuxError::Cancelled {
                    return Err(e);
                }
                let mut d = lock(&slot.data);
                d.failures = d.failures.saturating_add(1);
                d.last_failure = Some((Instant::now(), e.to_string()));
                d.read_at = Some(Instant::now());
                let Some(prev) = d.last.clone() else { return Err(e) };
                d.generation += 1;
                let since = prev.stale.as_ref().map(|s| s.since_ms).unwrap_or_else(now_ms);
                let mut snap = (*prev).clone();
                snap.generation = d.generation;
                snap.stale = Some(Stale { since_ms: since, message: e.to_string() });
                let snap = Arc::new(snap);
                let changed = differs(&prev, &snap);
                d.last = Some(Arc::clone(&snap));
                drop(d);
                if changed {
                    if let Some(sink) = &self.sink {
                        sink.changed(&snap);
                    }
                }
                Ok(snap)
            }
        }
    }

    // -- watching ------------------------------------------------------------------------------------------------------

    /// Keep this host's snapshot up to date every `interval` for as long as someone watches. `subscriber` names the view
    /// (`monitor`, `panel:<pane>`); watching again under the same name changes its interval. One loop serves everyone.
    /// `async` so that it can only be called from inside the runtime that the poll loop is spawned on (a plain function reachable
    /// from a sync Tauri command has none: see `spawnlint`).
    pub async fn watch(self: &Arc<Self>, host: Uuid, socket: Socket, subscriber: &str, interval: Duration) -> Result<(), TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        let interval = interval.clamp(Duration::from_secs(1), Duration::from_secs(60));
        let key = (host, socket);
        let mut w = lock(&self.watches);
        let start = match w.get_mut(&key) {
            Some(existing) => {
                existing.subscribers.insert(subscriber.to_string(), interval);
                false
            }
            None => {
                w.insert(key.clone(), Watch { subscribers: HashMap::from([(subscriber.to_string(), interval)]), stop: CancellationToken::new() });
                true
            }
        };
        if start {
            let stop = w[&key].stop.clone();
            let me = Arc::clone(self);
            tokio::spawn(async move { me.poll(key, stop).await });
        }
        Ok(())
    }

    /// Stop watching for `subscriber`, on every host. The loop of a host nobody else watches ends.
    pub fn unwatch(&self, subscriber: &str) {
        let mut w = lock(&self.watches);
        w.retain(|_, watch| {
            watch.subscribers.remove(subscriber);
            if watch.subscribers.is_empty() {
                watch.stop.cancel();
                false
            } else {
                true
            }
        });
    }

    pub fn watching(&self) -> usize {
        lock(&self.watches).len()
    }

    async fn poll(self: Arc<Self>, key: Key, stop: CancellationToken) {
        loop {
            let interval = {
                let w = lock(&self.watches);
                match w.get(&key) {
                    Some(watch) if !watch.stop.is_cancelled() => watch.subscribers.values().min().copied(),
                    _ => None,
                }
            };
            let Some(interval) = interval else { return };
            let slot = self.slot(&key);
            {
                let gate = slot.gate.lock().await;
                // Someone else may have read it while this loop waited for the gate.
                let recent = lock(&slot.data).read_at.is_some_and(|at| at.elapsed() < interval / 2);
                let cooling = self.backoff(&lock(&slot.data)).is_some();
                if !recent && !cooling && !stop.is_cancelled() {
                    let _ = self.read(&key, &slot, Some(&stop)).await;
                }
                drop(gate);
            }
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = stop.cancelled() => return,
            }
        }
    }

    // -- panes -----------------------------------------------------------------------------------------------------------

    /// The text of one pane: the visible screen, and up to `opts.scrollback` lines above it.
    pub async fn capture(&self, host: Uuid, socket: &Socket, pane: &str, opts: CaptureOpts, cancel: Option<&CancellationToken>) -> Result<PaneCapture, TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        if !valid_pane_id(pane) {
            return Err(TmuxError::Invalid { message: "That is not a tmux pane id.".into() });
        }
        let server = self.latest(host, socket).and_then(|s| s.server.clone());
        let nonce = script::nonce();
        let out = self.run(host, &script::capture(socket, &nonce, pane, opts, server.as_ref()), cancel).await?;
        match parse_capture(&out, &nonce)? {
            Captured::Pane { info, text } => Ok(PaneCapture { info: info.into(), text, taken_at_ms: now_ms() }),
            Captured::Failed { message } => Err(TmuxError::Rejected { message: if message.is_empty() { "The pane's output could not be captured.".into() } else { message } }),
            Captured::Stale => Err(TmuxError::ServerChanged),
        }
    }

    /// Search the panes `targets` for `query`, capturing a bounded amount of each. Captures run a few at a time, text is
    /// searched as it arrives and then dropped, and `cancel` stops the whole thing. Nothing is kept.
    pub async fn search(&self, host: Uuid, socket: &Socket, targets: Vec<SearchTarget>, query: &Query, scrollback: u32, cancel: &CancellationToken) -> Result<SearchReport, TmuxError> {
        let matcher = Matcher::new(query).map_err(|message| TmuxError::Invalid { message })?;
        let limited_by_count = targets.len() > SEARCH_MAX_PANES;
        let targets: Vec<SearchTarget> = targets.into_iter().take(SEARCH_MAX_PANES).collect();
        let opts = CaptureOpts { ansi: false, join: true, scrollback: scrollback.min(script::MAX_SCROLLBACK) };
        let mut results = Vec::new();
        let mut failed = Vec::new();
        let mut searched = 0u32;
        let mut total = 0usize;
        let mut limited = limited_by_count;
        let mut cancelled = false;

        // At most `max_concurrent` captures are in flight (the semaphore in `run` enforces the global limit; this keeps a
        // 200-pane search from queueing 200 futures at once).
        let width = self.cfg.max_concurrent.max(1);
        for chunk in targets.chunks(width) {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            let caps = futures_util::future::join_all(chunk.iter().map(|t| self.capture(host, socket, &t.pane_id, opts, Some(cancel)))).await;
            for (t, cap) in chunk.iter().zip(caps) {
                match cap {
                    Ok(c) => {
                        searched += 1;
                        let room = SEARCH_MAX_HITS.saturating_sub(total).min(SEARCH_MAX_HITS_PER_PANE);
                        let hits = search::find(&matcher, &c.text, room, 1);
                        total += hits.len();
                        if !hits.is_empty() {
                            results.push(PaneHits { session_id: t.session_id.clone(), window_id: t.window_id.clone(), pane_id: t.pane_id.clone(), hits });
                        }
                    }
                    Err(TmuxError::Cancelled) => cancelled = true,
                    Err(e) => failed.push((t.pane_id.clone(), e.to_string())),
                }
            }
            if total >= SEARCH_MAX_HITS {
                limited = true;
                break;
            }
        }
        Ok(SearchReport { results, failed, searched, limited, cancelled })
    }

    /// A search the caller can stop by name. Starting one with a name already in use stops the earlier one first: the newer
    /// search is the one the person is waiting for. Returns the token to search with and a serial for [`Self::end_search`].
    pub fn begin_search(&self, id: &str) -> (CancellationToken, u64) {
        let token = CancellationToken::new();
        let serial = self.search_serial.fetch_add(1, Ordering::Relaxed);
        if let Some((_, old)) = lock(&self.searches).insert(id.to_string(), (serial, token.clone())) {
            old.cancel();
        }
        (token, serial)
    }

    pub fn cancel_search(&self, id: &str) {
        if let Some((_, t)) = lock(&self.searches).remove(id) {
            t.cancel();
        }
    }

    /// The search is over; forget its name, unless a newer search has taken it.
    pub fn end_search(&self, id: &str, serial: u64) {
        let mut s = lock(&self.searches);
        if s.get(id).is_some_and(|(n, _)| *n == serial) {
            s.remove(id);
        }
    }

    // -- changes ---------------------------------------------------------------------------------------------------------

    /// Run one operation, then read the host again and accept the operation only if that reading shows it.
    pub async fn operate(&self, host: Uuid, socket: &Socket, op: &Op) -> Result<OpResult, TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        op.validate().map_err(|message| TmuxError::Invalid { message })?;
        let server = if op.needs_server() {
            // The identity comes from the snapshot the user was looking at; with none, there is nothing to compare with
            // and the operation is refused rather than aimed at whatever answers.
            let snap = self.latest(host, socket).ok_or_else(|| TmuxError::Invalid { message: "Refresh the list before changing it.".into() })?;
            Some(snap.server.clone().ok_or_else(|| TmuxError::Invalid { message: "There is no tmux server to change.".into() })?)
        } else {
            None
        };
        let nonce = script::nonce();
        let out = self.run(host, &op.script(socket, &nonce, server.as_ref()), None).await?;
        let created = match parse_operation(&out, &nonce)? {
            OpOutcome::Done { output } => output.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()),
            OpOutcome::Failed { message } => {
                // The server may have changed under us (a session died): show what is true now.
                let _ = self.refresh(host, socket).await;
                return Err(TmuxError::Rejected { message });
            }
            OpOutcome::Stale => {
                let _ = self.refresh(host, socket).await;
                return Err(TmuxError::ServerChanged);
            }
        };
        let created = created.filter(|c| valid_session_id(c) || super::model::valid_window_id(c) || valid_pane_id(c));
        let snapshot = self.refresh(host, socket).await?;
        if snapshot.stale.is_some() {
            return Err(TmuxError::Unreadable { message: "The change was sent, but the host could not be read afterwards to confirm it.".into() });
        }
        verify(op, created.as_deref(), &snapshot).map_err(|message| TmuxError::Rejected { message })?;
        Ok(OpResult { created, snapshot: (*snapshot).clone() })
    }

    /// Move the client that the tab marked `tab` is attached through to `session`. `Ok(false)`: no such client (the tab
    /// is not attached to this server), so the caller attaches the tab instead.
    pub async fn switch_client(&self, host: Uuid, socket: &Socket, tab: &str, session: &str) -> Result<bool, TmuxError> {
        socket.validate().map_err(|message| TmuxError::Invalid { message })?;
        if !valid_session_id(session) {
            return Err(TmuxError::Invalid { message: "That is not a tmux session id.".into() });
        }
        if tab.is_empty() || tab.len() > 64 || !tab.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(TmuxError::Invalid { message: "Not a terminal tab marker.".into() });
        }
        let snap = self.latest(host, socket).ok_or_else(|| TmuxError::Invalid { message: "Refresh the list before switching.".into() })?;
        let nonce = script::nonce();
        let out = self.run(host, &script::switch_client(socket, &nonce, snap.server.as_ref(), tab, session), None).await?;
        let sec = super::parse::sections(&out, &nonce)?;
        if sec.contains_key("stale") {
            return Err(TmuxError::ServerChanged);
        }
        if sec.contains_key("notattached") {
            return Ok(false);
        }
        let rc: i32 = sec.get("result").and_then(|s| s.trim().parse().ok()).unwrap_or(-1);
        let now = sec.get("now").map(|s| s.trim().to_string()).unwrap_or_default();
        if rc != 0 || now != session {
            let said = sec.get("output").map(|s| s.trim().to_string()).unwrap_or_default();
            let now = if now.is_empty() { "unknown".to_string() } else { super::model::clean(&now) };
            return Err(TmuxError::Rejected { message: if said.is_empty() { format!("tmux did not move the client (it is on session {now}).") } else { super::model::clean(&said) } });
        }
        let _ = self.refresh(host, socket).await;
        Ok(true)
    }

    // -- resources -------------------------------------------------------------------------------------------------------

    /// CPU and memory of the sessions in the last snapshot, from the host's process table.
    pub async fn resources(&self, host: Uuid, socket: &Socket) -> Result<Resources, TmuxError> {
        let snap = self.snapshot(host, socket, Duration::from_secs(5)).await?;
        let first = lock(&self.procs).get(&host).is_none();
        let out = self.run(host, &procs::script(first), None).await?;
        let raw = procs::parse(&out).map_err(|e| TmuxError::Unreadable { message: e.to_string() })?;
        if raw.os != "Linux" {
            return Err(TmuxError::Unreadable { message: format!("Resource figures are read from /proc, which {} does not have.", if raw.os.is_empty() { "this system" } else { &raw.os }) });
        }
        let prev = lock(&self.procs).get(&host).cloned();
        let res = resources::derive(host, now_ms(), &snap.sessions, &raw, prev.as_ref());
        lock(&self.procs).insert(host, raw.now);
        Ok(res)
    }

    // -- lifecycle -------------------------------------------------------------------------------------------------------

    /// Forget what is kept about a host.
    pub fn forget(&self, host: Uuid) {
        lock(&self.slots).retain(|(h, _), _| *h != host);
        lock(&self.procs).remove(&host);
    }

    /// The vault was locked: nothing read from hosts is kept and nothing is polled.
    pub fn clear(&self) {
        for (_, w) in lock(&self.watches).drain() {
            w.stop.cancel();
        }
        lock(&self.slots).clear();
        lock(&self.procs).clear();
        for (_, (_, t)) in lock(&self.searches).drain() {
            t.cancel();
        }
    }
}

/// Did the part of a snapshot a view shows change? Generation and read time always do.
fn differs(a: &HostSnapshot, b: &HostSnapshot) -> bool {
    a.state != b.state || a.sessions != b.sessions || a.server != b.server || a.stale != b.stale || a.version != b.version || a.other_sockets != b.other_sockets
}

/// Whether a snapshot has sessions worth listing.
pub fn has_sessions(s: &HostSnapshot) -> bool {
    matches!(s.state, ServerState::Ready) && !s.sessions.is_empty()
}
