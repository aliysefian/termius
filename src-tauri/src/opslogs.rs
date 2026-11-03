//! Operations → Logs: following the system journal (or a log file) on several hosts at once.
//!
//! One managed task per host. Each logs in through the same [`MonitorManager`] the rest of Operations uses (a second
//! connection that never asks for a password), runs `journalctl --follow -o json`, turns each line into a typed
//! [`Entry`], and hands entries to the window in batches. What this module promises, and what it doesn't:
//!
//! * Every entry says which SSHVault host it came from. The journal's own `_HOSTNAME` is kept as a detail, not trusted
//!   as identity.
//! * A dropped connection is retried with a bounded, growing wait. The journal cursor of the last entry seen is used to
//!   carry on after it, so a reconnect neither repeats nor (normally) skips entries. A log file can't be resumed: the
//!   host is told so with a [`OpsEvent::Gap`]. Nothing here can promise there is no gap: a journal that rotated while
//!   the connection was down has lost those entries.
//! * Nothing from a host reaches a shell: the unit, the path and the cursor are checked and quoted.
//! * Secrets in messages and fields are hidden before an entry leaves this module (unless the person turned that off).
//! * A host that writes faster than [`MAX_PER_SEC`] entries a second has the excess counted and dropped, and the count
//!   is reported, so one noisy host can't swamp the window.
//! * Stopping ends the remote command: the connection's channel is closed, which hangs the process up.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch, Notify, Semaphore};
use uuid::Uuid;

use crate::containers::transport::shell_quote;
use crate::containers::{ContainerError, LogEvent, LogSink};
use crate::mask;
use crate::monitor::MonitorManager;
use crate::ssh::{SshError, Target};

/// Hosts one stream may follow.
pub const MAX_HOSTS: usize = 32;
/// Streams (hosts, counting all of them) running at once.
pub const MAX_RUNNING: usize = 64;
/// The longest line read; a longer one is dropped and counted.
pub const MAX_LINE: usize = 64 * 1024;
/// How long entries are held before being sent.
pub const FLUSH: Duration = Duration::from_millis(100);
/// Entries sent for one host in a second; the rest are counted as dropped.
pub const MAX_PER_SEC: usize = 3000;
/// The longest message kept (the rest is cut, and the entry says so).
pub const MAX_MESSAGE: usize = 16 * 1024;
const MAX_FIELDS: usize = 48;
const MAX_FIELD: usize = 1024;
/// Entries per batch.
const BATCH: usize = 500;
/// Cursors remembered to drop an entry seen twice.
const SEEN: usize = 256;
pub const MAX_HISTORY: u32 = 5000;
const MAX_CONTEXT: u32 = 50;

// -- what to follow ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// The system journal, optionally one unit, the kernel only, and only this priority (0 = emergency) and worse.
    Journal {
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        kernel: bool,
        #[serde(default)]
        priority: Option<u8>,
    },
    File { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum History {
    /// Start with the last N entries.
    Last(u32),
    /// Start with everything since this time (seconds since 1970).
    Since(i64),
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    pub source: Source,
    pub history: History,
    #[serde(default = "yes")]
    pub auto_reconnect: bool,
    /// Hide passwords, tokens and keys. On unless the person said otherwise.
    #[serde(default = "yes")]
    pub redact: bool,
}

const UNIT_TYPES: [&str; 8] = ["service", "socket", "timer", "target", "mount", "path", "slice", "scope"];

/// A systemd unit name: letters, digits and `:_.@\-`, ending in a unit type.
pub fn valid_unit(u: &str) -> bool {
    if u.is_empty() || u.len() > 256 || !u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '@' | '\\' | '-')) {
        return false;
    }
    u.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && UNIT_TYPES.contains(&ext))
}

/// A journal cursor as journald writes it: `s=...;i=...;b=...;m=...;t=...;x=...`.
pub fn valid_cursor(c: &str) -> bool {
    !c.is_empty() && c.len() <= 512 && c.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '=' | ';' | ':' | '_' | '.' | '-'))
}

impl Spec {
    pub fn check(&self) -> Result<(), String> {
        match &self.source {
            Source::Journal { unit, priority, .. } => {
                if let Some(u) = unit.as_deref().filter(|u| !u.is_empty()) {
                    if !valid_unit(u) {
                        return Err("The unit must look like nginx.service.".into());
                    }
                }
                if priority.is_some_and(|p| p > 7) {
                    return Err("A priority is 0 (emergency) to 7 (debug).".into());
                }
            }
            Source::File { path } => {
                if path.trim().is_empty() {
                    return Err("Give the path of a log file.".into());
                }
                if !path.starts_with('/') {
                    return Err("The path must be absolute (start with /).".into());
                }
                if path.len() > 1024 || path.contains('\0') || path.contains('\n') {
                    return Err("That path isn't usable.".into());
                }
                if matches!(self.history, History::Since(_)) {
                    return Err("A log file can only start with its last lines.".into());
                }
            }
        }
        match self.history {
            History::Last(n) if n > MAX_HISTORY => Err(format!("Start with at most {MAX_HISTORY} lines.")),
            History::Since(s) if s <= 0 || s > chrono::Utc::now().timestamp() + 60 => Err("That time isn't in the past.".into()),
            _ => Ok(()),
        }
    }

    /// The script that follows the source. `resume` is the cursor of the last entry seen, when there is one: the script
    /// then carries on after it instead of starting again.
    pub fn script(&self, resume: Option<&str>, reconnecting: bool) -> Result<String, String> {
        self.check()?;
        match &self.source {
            Source::File { path } => {
                // A file has no cursor: on a reconnect only new lines are followed (what came meanwhile is lost).
                let n = match (reconnecting, self.history) {
                    (true, _) => 0,
                    (false, History::Last(n)) => n,
                    (false, History::Since(_)) => 0,
                };
                Ok(format!("exec tail -n {n} -F -- {}", shell_quote(path)))
            }
            Source::Journal { unit, kernel, priority } => {
                let mut parts = vec!["SYSTEMD_COLORS=0 SYSTEMD_URLIFY=0 exec journalctl".to_string(), "--follow".into(), "--no-pager".into(), "-o json".into()];
                if *kernel {
                    parts.push("-k".into());
                }
                if let Some(u) = unit.as_deref().filter(|u| !u.is_empty()) {
                    parts.push(format!("-u {}", shell_quote(u)));
                }
                if let Some(p) = priority {
                    parts.push(format!("-p {p}"));
                }
                match (resume.filter(|c| valid_cursor(c)), self.history) {
                    (Some(c), _) => parts.push(format!("--after-cursor {}", shell_quote(c))),
                    (None, History::Last(n)) => parts.push(format!("-n {n}")),
                    (None, History::Since(s)) => {
                        let t = chrono::DateTime::from_timestamp(s, 0).ok_or("That time isn't usable.")?;
                        parts.push(format!("--since {}", shell_quote(&t.format("%Y-%m-%d %H:%M:%S UTC").to_string())));
                    }
                }
                Ok(parts.join(" "))
            }
        }
    }

    fn is_journal(&self) -> bool {
        matches!(self.source, Source::Journal { .. })
    }
}

// -- entries -------------------------------------------------------------------------------------

/// One log entry. `host_id` is the SSHVault host it was read from; `seq` counts up per host for the life of a stream, so
/// `(host_id, seq)` identifies an entry (a timestamp doesn't: two can share one).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub host_id: Uuid,
    pub seq: u64,
    /// Microseconds since 1970: the journal's own time, or the time it arrived for a file.
    pub ts_us: i64,
    /// 0 (emergency) to 7 (debug); none for a log file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The program's name (`SYSLOG_IDENTIFIER`, else `_COMM`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ident: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// What the journal calls the machine (`_HOSTNAME`). A detail only; the entry's host is `host_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal_host: Option<String>,
    /// The journal's other fields, in the order they came (secrets hidden, long values cut).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<(String, String)>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// An entry before it has a host and a number.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub ts_us: Option<i64>,
    pub priority: Option<u8>,
    pub unit: Option<String>,
    pub ident: Option<String>,
    pub pid: Option<u32>,
    pub message: String,
    pub cursor: Option<String>,
    pub journal_host: Option<String>,
    pub fields: Vec<(String, String)>,
    pub truncated: bool,
}

/// Names of syslog levels, for text.
pub fn priority_name(p: u8) -> &'static str {
    ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"].get(p as usize).copied().unwrap_or("info")
}

/// A journal field's value as text: a string, a list (a field written twice) joined, or bytes (a message that isn't text).
fn field_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Array(a) if !a.is_empty() && a.iter().all(|x| x.is_u64()) => {
            let bytes: Vec<u8> = a.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect();
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        serde_json::Value::Array(a) => {
            let parts: Vec<String> = a.iter().filter_map(field_text).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        _ => None,
    }
}

fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}…", &s[..end]), true)
}

/// Fields that repeat what is already shown, or are of no use to read.
const SKIP: [&str; 6] = ["MESSAGE", "__CURSOR", "__REALTIME_TIMESTAMP", "__MONOTONIC_TIMESTAMP", "SYSLOG_RAW", "_SOURCE_REALTIME_TIMESTAMP"];

/// One line of `journalctl -o json`. `None` for a line that isn't a JSON object (a hint or an error the command printed).
pub fn parse_journal_line(line: &str) -> Option<Raw> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = v.as_object()?;
    let get = |k: &str| obj.get(k).and_then(field_text);
    let (message, truncated) = cut(&get("MESSAGE").unwrap_or_default(), MAX_MESSAGE);
    let priority = get("PRIORITY").and_then(|p| p.trim().parse::<u8>().ok()).filter(|p| *p <= 7);
    let unit = get("_SYSTEMD_UNIT").or_else(|| get("_SYSTEMD_USER_UNIT")).or_else(|| get("UNIT")).or_else(|| get("USER_UNIT")).filter(|u| !u.is_empty());
    let ident = get("SYSLOG_IDENTIFIER").or_else(|| get("_COMM")).filter(|u| !u.is_empty());
    let pid = get("_PID").or_else(|| get("SYSLOG_PID")).and_then(|p| p.trim().parse().ok());
    let ts_us = get("__REALTIME_TIMESTAMP").and_then(|t| t.trim().parse::<i64>().ok());
    let cursor = get("__CURSOR").filter(|c| valid_cursor(c));
    let journal_host = get("_HOSTNAME").filter(|h| !h.is_empty()).map(|h| cut(&h, 255).0);
    let mut fields = Vec::new();
    for (k, val) in obj {
        if SKIP.contains(&k.as_str()) || fields.len() >= MAX_FIELDS {
            continue;
        }
        if let Some(t) = field_text(val) {
            fields.push((k.clone(), cut(&t, MAX_FIELD).0));
        }
    }
    Some(Raw { ts_us, priority, unit, ident, pid, message, cursor, journal_host, fields, truncated })
}

/// Text without terminal escape sequences (colours, cursor moves, links). The command runs on a terminal so that it ends
/// when the connection does, and some programs colour what they print when they see one.
pub fn strip_ansi(text: &str) -> String {
    if !text.contains('\x1b') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then a final byte.
            Some('[') => {
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ST.
            Some(']') => {
                while let Some(n) = chars.next() {
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            // A character set choice takes one more byte.
            Some('(' | ')' | '*' | '+') => {
                chars.next();
            }
            _ => {}
        }
    }
    out
}

/// Hides secrets in what is about to be shown. Keeps the state a multi-line private key needs.
#[derive(Debug, Default)]
pub struct Redactor {
    on: bool,
    in_key: bool,
}

impl Redactor {
    pub fn new(on: bool) -> Self {
        Self { on, in_key: false }
    }

    pub fn message(&mut self, text: &str) -> String {
        if !self.on {
            return text.to_string();
        }
        if self.in_key {
            if text.contains("-----END ") {
                self.in_key = false;
            }
            return "[private key hidden]".into();
        }
        if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY") {
            self.in_key = !text.contains("-----END ");
            return "[private key hidden]".into();
        }
        mask::secrets(text)
    }

    pub fn field(&self, name: &str, value: &str) -> String {
        if !self.on {
            value.to_string()
        } else if mask::is_secret_name(name) {
            mask::HIDDEN.to_string()
        } else {
            mask::secrets(value)
        }
    }
}

fn finish(raw: Raw, host_id: Uuid, seq: u64, redactor: &mut Redactor, arrived_us: i64) -> Entry {
    Entry {
        host_id,
        seq,
        ts_us: raw.ts_us.unwrap_or(arrived_us),
        priority: raw.priority,
        unit: raw.unit,
        ident: raw.ident,
        pid: raw.pid,
        message: redactor.message(&raw.message),
        cursor: raw.cursor,
        journal_host: raw.journal_host,
        fields: raw.fields.into_iter().map(|(k, v)| { let v = redactor.field(&k, &v); (k, v) }).collect(),
        truncated: raw.truncated,
    }
}

/// A line of a log file, as an entry that has only text.
fn file_raw(line: &str, ident: &str) -> Raw {
    let (message, truncated) = cut(line, MAX_MESSAGE);
    Raw { ts_us: None, priority: None, unit: None, ident: Some(ident.to_string()), pid: None, message, cursor: None, journal_host: None, fields: Vec::new(), truncated }
}

/// Cuts text arriving in pieces into whole lines.
#[derive(Debug, Default)]
pub struct Lines {
    rest: String,
    /// A line that was too long and is being skipped to its end.
    skipping: bool,
    /// Lines thrown away for being too long.
    pub overlong: u64,
}

impl Lines {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        let mut out = Vec::new();
        for piece in chunk.split_inclusive('\n') {
            let ends = piece.ends_with('\n');
            if self.skipping {
                if ends {
                    self.skipping = false;
                }
                continue;
            }
            self.rest.push_str(piece.trim_end_matches('\n'));
            if self.rest.len() > MAX_LINE {
                self.rest.clear();
                self.overlong += 1;
                self.skipping = !ends;
                continue;
            }
            if ends {
                let line = std::mem::take(&mut self.rest);
                out.push(line.trim_end_matches('\r').to_string());
            }
        }
        out
    }
}

/// Remembers the last few cursors, to refuse an entry seen twice (after a reconnect).
#[derive(Debug, Default)]
pub struct Seen {
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl Seen {
    /// True if `cursor` is new (and now remembered).
    pub fn new_cursor(&mut self, cursor: &str) -> bool {
        if self.set.contains(cursor) {
            return false;
        }
        self.set.insert(cursor.to_string());
        self.order.push_back(cursor.to_string());
        if self.order.len() > SEEN {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }
}

/// Counts entries in the current second.
#[derive(Debug)]
pub struct Limiter {
    window: Instant,
    count: usize,
    max: usize,
}

impl Limiter {
    pub fn new(max: usize) -> Self {
        Self { window: Instant::now(), count: 0, max }
    }

    pub fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window) >= Duration::from_secs(1) {
            self.window = now;
            self.count = 0;
        }
        if self.count >= self.max {
            return false;
        }
        self.count += 1;
        true
    }
}

// -- events --------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostState {
    Connecting,
    Streaming,
    /// Lost, waiting to try again (or trying again).
    Reconnecting,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OpsEvent {
    State {
        host_id: Uuid,
        state: HostState,
        /// Consecutive tries that failed, while reconnecting.
        attempt: u32,
        /// Seconds until the next try, while waiting for it.
        retry_in_secs: Option<u64>,
        /// Why, in words, for a failure or a lost connection.
        message: Option<String>,
        /// What the command said that isn't a log line (for example that this user can't read the whole journal).
        notice: Option<String>,
    },
    Batch {
        host_id: Uuid,
        entries: Vec<Entry>,
        /// Entries left out since the last batch because the host was writing faster than they can be shown.
        dropped: u64,
    },
    /// Entries may be missing here: the connection was lost and what came meanwhile could not be recovered.
    Gap { host_id: Uuid, reason: String },
}

pub trait OpsSink: Send + Sync + 'static {
    fn event(&self, e: OpsEvent);
}

// -- policy --------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub base: Duration,
    pub max_wait: Duration,
    /// Failed tries in a row before giving up (a try that streamed something starts the count again).
    pub attempts: u32,
    /// Hosts being logged in to at once.
    pub connecting: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self { base: Duration::from_secs(2), max_wait: Duration::from_secs(60), attempts: 10, connecting: 6 }
    }
}

impl Policy {
    /// The wait before try number `attempt` (1 is the first retry): doubling, to at most `max_wait`.
    pub fn wait(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(16);
        self.base.saturating_mul(1u32 << shift).min(self.max_wait)
    }
}

/// An error that trying again can't fix.
fn permanent(e: &ContainerError) -> bool {
    matches!(
        e,
        ContainerError::Ssh(SshError::AuthFailed { .. } | SshError::HostKeyChanged { .. } | SshError::HostKeyUnknown { .. } | SshError::HostKeyStore { .. } | SshError::Key(_))
    )
}

// -- the manager ---------------------------------------------------------------------------------

pub struct HostJob {
    pub host_id: Uuid,
    pub label: String,
    pub target: Result<Target, String>,
}

struct HostCtl {
    cancel: watch::Sender<bool>,
    retry: Notify,
    /// The connection the host's stream is on, while it has one (context lookups use it).
    session: Mutex<Option<Uuid>>,
    state: Mutex<HostState>,
}

struct Stream {
    spec: Spec,
    sink: Arc<dyn OpsSink>,
    hosts: Mutex<HashMap<Uuid, Arc<HostCtl>>>,
    stopped: AtomicBool,
}

struct Inner {
    monitors: Arc<MonitorManager>,
    streams: Mutex<HashMap<String, Arc<Stream>>>,
    gate: Arc<Semaphore>,
    policy: Policy,
}

pub struct OpsLogManager {
    inner: Arc<Inner>,
}

/// Which entries [`OpsLogManager::context`] should read: those around the one at `cursor`, from the same unit if given.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    pub cursor: &'a str,
    pub before: u32,
    pub after: u32,
    pub unit: Option<&'a str>,
    pub redact: bool,
}

/// What [`OpsLogManager::context`] found around an entry.
#[derive(Debug, Serialize)]
pub struct Context {
    pub before: Vec<Entry>,
    pub after: Vec<Entry>,
}

impl OpsLogManager {
    pub fn new(monitors: Arc<MonitorManager>) -> Self {
        Self::with_policy(monitors, Policy::default())
    }

    pub fn with_policy(monitors: Arc<MonitorManager>, policy: Policy) -> Self {
        Self { inner: Arc::new(Inner { monitors, streams: Mutex::new(HashMap::new()), gate: Arc::new(Semaphore::new(policy.connecting.max(1))), policy }) }
    }

    /// How many hosts are being followed, across streams.
    pub fn running(&self) -> usize {
        self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).values().map(|s| s.hosts.lock().unwrap_or_else(|p| p.into_inner()).len()).sum()
    }

    pub fn stream_count(&self) -> usize {
        self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Start following `hosts`. Each host has its own task; one that can't connect doesn't stop the others.
    pub fn start(&self, id: String, spec: Spec, hosts: Vec<HostJob>, sink: Arc<dyn OpsSink>) -> Result<(), String> {
        spec.check()?;
        if hosts.is_empty() {
            return Err("Choose at least one host.".into());
        }
        if hosts.len() > MAX_HOSTS {
            return Err(format!("Follow at most {MAX_HOSTS} hosts at once."));
        }
        if self.running() + hosts.len() > MAX_RUNNING {
            return Err("Too many logs are being followed already. Stop some first.".into());
        }
        let stream = Arc::new(Stream { spec, sink, hosts: Mutex::new(HashMap::new()), stopped: AtomicBool::new(false) });
        {
            let mut all = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner());
            if all.contains_key(&id) {
                return Err("That stream is already running.".into());
            }
            all.insert(id.clone(), Arc::clone(&stream));
        }
        self.spawn_hosts(&id, &stream, hosts);
        Ok(())
    }

    fn spawn_hosts(&self, id: &str, stream: &Arc<Stream>, hosts: Vec<HostJob>) {
        for job in hosts {
            let ctl = Arc::new(HostCtl { cancel: watch::channel(false).0, retry: Notify::new(), session: Mutex::new(None), state: Mutex::new(HostState::Connecting) });
            // A host listed twice is followed once.
            if stream.hosts.lock().unwrap_or_else(|p| p.into_inner()).insert(job.host_id, Arc::clone(&ctl)).is_some() {
                continue;
            }
            let inner = Arc::clone(&self.inner);
            let stream = Arc::clone(stream);
            let id = id.to_string();
            tauri::async_runtime::spawn(async move {
                host_worker(Arc::clone(&inner), Arc::clone(&stream), job, Arc::clone(&ctl)).await;
                let mut hosts = stream.hosts.lock().unwrap_or_else(|p| p.into_inner());
                // Only our own entry: the host may have been taken out and added again meanwhile.
                hosts.retain(|_, c| !Arc::ptr_eq(c, &ctl));
                let empty = hosts.is_empty();
                drop(hosts);
                if empty && stream.stopped.load(Ordering::SeqCst) {
                    inner.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                }
            });
        }
    }

    /// Follow more hosts in a running stream.
    pub fn add_hosts(&self, id: &str, hosts: Vec<HostJob>) -> Result<(), String> {
        let stream = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned().ok_or("That stream isn't running.")?;
        if stream.stopped.load(Ordering::SeqCst) {
            return Err("That stream is stopping.".into());
        }
        let have = stream.hosts.lock().unwrap_or_else(|p| p.into_inner()).len();
        if have + hosts.len() > MAX_HOSTS {
            return Err(format!("Follow at most {MAX_HOSTS} hosts at once."));
        }
        if self.running() + hosts.len() > MAX_RUNNING {
            return Err("Too many logs are being followed already. Stop some first.".into());
        }
        self.spawn_hosts(id, &stream, hosts);
        Ok(())
    }

    /// Stop following some of a running stream's hosts. The others carry on.
    pub fn remove_hosts(&self, id: &str, hosts: &[Uuid]) {
        let Some(stream) = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned() else { return };
        let map = stream.hosts.lock().unwrap_or_else(|p| p.into_inner());
        for h in hosts {
            if let Some(c) = map.get(h) {
                let _ = c.cancel.send(true);
            }
        }
    }

    /// Try a failed host again now, without waiting. Counts from the start.
    pub fn retry(&self, id: &str, host: Uuid) -> bool {
        let Some(stream) = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned() else { return false };
        let ctl = stream.hosts.lock().unwrap_or_else(|p| p.into_inner()).get(&host).cloned();
        ctl.map(|c| c.retry.notify_one()).is_some()
    }

    /// Stop a stream: its remote commands are ended, and each host reports `Stopped` when its connection is closed.
    pub fn stop(&self, id: &str) -> bool {
        let Some(stream) = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned() else { return false };
        stream.stopped.store(true, Ordering::SeqCst);
        let ctls: Vec<Arc<HostCtl>> = stream.hosts.lock().unwrap_or_else(|p| p.into_inner()).values().cloned().collect();
        if ctls.is_empty() {
            self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        }
        for c in ctls {
            let _ = c.cancel.send(true);
        }
        true
    }

    pub fn stop_all(&self) {
        let ids: Vec<String> = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect();
        for id in ids {
            self.stop(&id);
        }
    }

    /// The state each host of a stream is in now.
    pub fn states(&self, id: &str) -> Vec<(Uuid, HostState)> {
        let Some(stream) = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned() else { return Vec::new() };
        let map = stream.hosts.lock().unwrap_or_else(|p| p.into_inner());
        map.iter().map(|(h, c)| (*h, *c.state.lock().unwrap_or_else(|p| p.into_inner()))).collect()
    }

    /// The connection of a host in a stream, if it has one now.
    pub fn session_of(&self, id: &str, host: Uuid) -> Option<Uuid> {
        let stream = self.inner.streams.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()?;
        let ctl = stream.hosts.lock().unwrap_or_else(|p| p.into_inner()).get(&host).cloned()?;
        let s = *ctl.session.lock().unwrap_or_else(|p| p.into_inner());
        s
    }

    /// Run a short script on a host over the stream's own connection if it has one, else over a temporary one.
    async fn exec_on(&self, id: &str, host: Uuid, target: Option<Target>, script: &str) -> Result<String, String> {
        let monitors = &self.inner.monitors;
        if let Some(s) = self.session_of(id, host) {
            if let Ok(out) = monitors.exec(s, script, Duration::from_secs(20)).await {
                return Ok(out.stdout);
            }
        }
        let target = target.ok_or("This host can't be reached without asking for a password.")?;
        let session = monitors.open(with_keepalive(target)).await.map_err(|e| e.to_string())?;
        let out = monitors.exec(session, script, Duration::from_secs(20)).await;
        monitors.close(session).await;
        out.map(|o| o.stdout).map_err(|e| e.to_string())
    }

    /// The entries just before and after one, read from the host's journal by cursor. Only real entries: if the journal
    /// no longer has them, the lists are shorter.
    pub async fn context(&self, id: &str, host: Uuid, target: Option<Target>, around: Around<'_>) -> Result<Context, String> {
        let Around { cursor, before, after, unit, redact } = around;
        if !valid_cursor(cursor) {
            return Err("That isn't a journal position.".into());
        }
        if unit.is_some_and(|u| !valid_unit(u)) {
            return Err("The unit must look like nginx.service.".into());
        }
        let (before, after) = (before.min(MAX_CONTEXT), after.min(MAX_CONTEXT));
        let base = format!("journalctl --no-pager -q -o json{}", unit.map(|u| format!(" -u {}", shell_quote(u))).unwrap_or_default());
        let q = shell_quote(cursor);
        let mut redactor = Redactor::new(redact);
        let mut read = |text: &str| -> Vec<Entry> {
            text.lines().filter_map(parse_journal_line).map(|r| finish(r, host, 0, &mut redactor, 0)).collect()
        };
        let mut earlier = Vec::new();
        if before > 0 {
            // Newest first, starting at the entry itself.
            let out = self.exec_on(id, host, target.clone(), &format!("{base} -r --cursor {q} -n {}", before + 1)).await?;
            earlier = read(&out).into_iter().filter(|e| e.cursor.as_deref() != Some(cursor)).collect();
            earlier.reverse();
        }
        let mut later = Vec::new();
        if after > 0 {
            let out = self.exec_on(id, host, target, &format!("{base} --after-cursor {q} | head -n {after}")).await?;
            later = read(&out);
        }
        Ok(Context { before: earlier, after: later })
    }

    /// Unit names on a host, for the unit picker. Empty when the host has no systemd.
    pub async fn units(&self, host: Uuid, target: Target) -> Result<Vec<String>, String> {
        let script = "command -v systemctl >/dev/null 2>&1 || exit 0; systemctl list-units --all --plain --no-legend --no-pager --type=service,socket,timer,target 2>/dev/null | awk '{print $1}' | head -n 3000";
        let out = self.exec_on("", host, Some(target), script).await?;
        let mut names: Vec<String> = out.lines().map(|l| l.trim_start_matches(['●', '○', '×', '*', ' ']).trim().to_string()).filter(|l| valid_unit(l)).collect();
        names.sort();
        names.dedup();
        Ok(names)
    }
}

/// Resolves when the host's stream is told to stop. (Its own function so the watch guard never lives across an await.)
async fn wait_cancel(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|c| *c).await;
}

/// A silent drop (a cable pulled, a NAT that forgot the connection) is noticed in under a minute.
fn with_keepalive(mut t: Target) -> Target {
    if t.keepalive_secs.is_none() {
        t.keepalive_secs = Some(15);
    }
    t
}

// -- one host -------------------------------------------------------------------------------------

struct ToChannel(mpsc::UnboundedSender<LogEvent>);
impl LogSink for ToChannel {
    fn event(&self, e: LogEvent) {
        let _ = self.0.send(e);
    }
}

/// How one try ended.
enum Attempt {
    Cancelled,
    Ended {
        /// Some entries were read before it ended.
        streamed: bool,
        /// Trying again would fail the same way.
        permanent: bool,
        message: String,
        /// The saved position wasn't accepted by the journal.
        bad_cursor: bool,
    },
}

/// What the loop around a host carries from one try to the next.
struct Carry {
    seq: u64,
    cursor: Option<String>,
    seen: Seen,
    redactor: Redactor,
    notices: Vec<String>,
    /// The newest journal time shown so far, to recognise a replay of old entries after a resume.
    last_ts_us: i64,
}

async fn host_worker(inner: Arc<Inner>, stream: Arc<Stream>, job: HostJob, ctl: Arc<HostCtl>) {
    let host_id = job.host_id;
    let sink = Arc::clone(&stream.sink);
    let state = |s: HostState, attempt: u32, retry_in: Option<Duration>, message: Option<String>, notice: Option<String>| {
        *ctl.state.lock().unwrap_or_else(|p| p.into_inner()) = s;
        sink.event(OpsEvent::State { host_id, state: s, attempt, retry_in_secs: retry_in.map(|d| d.as_secs().max(1)), message, notice });
    };
    let mut cancel = ctl.cancel.subscribe();

    let target = match job.target {
        Ok(t) => with_keepalive(t),
        Err(message) => {
            state(HostState::Failed, 0, None, Some(message), None);
            let _ = wait_cancel(&mut cancel).await;
            state(HostState::Stopped, 0, None, None, None);
            return;
        }
    };

    let mut carry = Carry { seq: 0, cursor: None, seen: Seen::default(), redactor: Redactor::new(stream.spec.redact), notices: Vec::new(), last_ts_us: 0 };
    let mut attempt = 0u32;
    let mut reconnecting = false;

    'outer: loop {
        if *cancel.borrow() {
            break;
        }
        state(if reconnecting { HostState::Reconnecting } else { HostState::Connecting }, attempt, None, None, None);
        let end = run_attempt(&inner, &stream, &target, host_id, &ctl, &mut cancel, &mut carry, reconnecting, &state).await;
        let (streamed, permanent_failure, message, bad_cursor) = match end {
            Attempt::Cancelled => break,
            Attempt::Ended { streamed, permanent, message, bad_cursor } => (streamed, permanent, message, bad_cursor),
        };
        if streamed {
            attempt = 0;
        }
        let notice = (!carry.notices.is_empty()).then(|| carry.notices.join(" "));
        if bad_cursor {
            // The journal doesn't know the saved position (it was rotated away): say so, and start from now.
            carry.cursor = None;
            stream.sink.event(OpsEvent::Gap { host_id, reason: "The journal no longer has the last entry seen, so entries written while the connection was down are missing.".into() });
            reconnecting = true;
            continue;
        }
        if permanent_failure || !stream.spec.auto_reconnect {
            let shown = if permanent_failure { message } else { format!("Disconnected: {message}") };
            state(HostState::Failed, attempt, None, Some(shown), notice);
            tokio::select! {
                _ = wait_cancel(&mut cancel) => break 'outer,
                _ = ctl.retry.notified() => { attempt = 0; reconnecting = true; continue 'outer; }
            }
        }
        attempt += 1;
        if attempt > inner.policy.attempts {
            state(HostState::Failed, attempt - 1, None, Some(format!("Gave up after {} tries: {message}", inner.policy.attempts)), notice);
            tokio::select! {
                _ = wait_cancel(&mut cancel) => break 'outer,
                _ = ctl.retry.notified() => { attempt = 0; reconnecting = true; continue 'outer; }
            }
        }
        let wait = inner.policy.wait(attempt);
        state(HostState::Reconnecting, attempt, Some(wait), Some(message), notice);
        reconnecting = true;
        tokio::select! {
            _ = wait_cancel(&mut cancel) => break 'outer,
            _ = ctl.retry.notified() => {}
            _ = tokio::time::sleep(wait) => {}
        }
    }
    *ctl.session.lock().unwrap_or_else(|p| p.into_inner()) = None;
    state(HostState::Stopped, 0, None, None, None);
}

#[allow(clippy::too_many_arguments)]
async fn run_attempt(
    inner: &Arc<Inner>,
    stream: &Arc<Stream>,
    target: &Target,
    host_id: Uuid,
    ctl: &Arc<HostCtl>,
    cancel: &mut watch::Receiver<bool>,
    carry: &mut Carry,
    reconnecting: bool,
    state: &(dyn Fn(HostState, u32, Option<Duration>, Option<String>, Option<String>) + Sync),
) -> Attempt {
    let sink = &stream.sink;
    let spec = &stream.spec;
    let monitors = &inner.monitors;

    // Log in, a few hosts at a time.
    let permit = tokio::select! {
        _ = wait_cancel(&mut *cancel) => return Attempt::Cancelled,
        p = inner.gate.acquire() => p.ok(),
    };
    let opened = tokio::select! {
        _ = wait_cancel(&mut *cancel) => return Attempt::Cancelled,
        r = monitors.open(target.clone()) => r,
    };
    drop(permit);
    let session = match opened {
        Ok(s) => s,
        Err(e) => return Attempt::Ended { streamed: false, permanent: permanent(&e), message: e.to_string(), bad_cursor: false },
    };
    *ctl.session.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);

    let resumed = carry.cursor.clone();
    let script = match spec.script(resumed.as_deref(), reconnecting) {
        Ok(s) => s,
        Err(message) => {
            monitors.close(session).await;
            return Attempt::Ended { streamed: false, permanent: true, message, bad_cursor: false };
        }
    };
    // A file can't be resumed: say that entries may be missing once it is back.
    if reconnecting && !spec.is_journal() {
        sink.event(OpsEvent::Gap { host_id, reason: "Reconnected. Lines written to the file while the connection was down are not recovered.".into() });
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<LogEvent>();
    let stream_id = match monitors.start_stream(session, &script, Arc::new(ToChannel(tx))) {
        Ok(id) => id,
        Err(e) => {
            monitors.close(session).await;
            return Attempt::Ended { streamed: false, permanent: false, message: e.to_string(), bad_cursor: false };
        }
    };
    state(HostState::Streaming, 0, None, None, (!carry.notices.is_empty()).then(|| carry.notices.join(" ")));

    let ident = match &spec.source {
        Source::File { path } => path.rsplit('/').next().unwrap_or("file").to_string(),
        _ => String::new(),
    };
    let mut lines = Lines::default();
    let mut limiter = Limiter::new(MAX_PER_SEC);
    let mut pending: Vec<Entry> = Vec::new();
    let mut dropped = 0u64;
    let mut streamed = false;
    let mut ticker = tokio::time::interval(FLUSH);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut notified_notice = carry.notices.len();
    let mut ended: Option<(Option<i32>, Option<String>)> = None;
    let mut stopping = false;
    let mut replayed = 0u64;

    let flush = |pending: &mut Vec<Entry>, dropped: &mut u64| {
        if !pending.is_empty() || *dropped > 0 {
            sink.event(OpsEvent::Batch { host_id, entries: std::mem::take(pending), dropped: std::mem::take(dropped) });
        }
    };

    loop {
        tokio::select! {
            _ = wait_cancel(&mut *cancel), if !stopping => {
                // Ending the command: the channel is closed, which hangs it up. Wait for the end so that stopping
                // means stopped, but not for long.
                monitors.stop_stream(stream_id);
                stopping = true;
                let _ = tokio::time::timeout(Duration::from_secs(5), async {
                    while let Some(e) = rx.recv().await {
                        if matches!(e, LogEvent::End { .. }) { break; }
                    }
                }).await;
                break;
            }
            ev = rx.recv() => {
                let Some(ev) = ev else { ended = Some((None, Some("The stream closed.".into()))); break };
                match ev {
                    LogEvent::Chunk { text } => {
                        for line in lines.push(&text) {
                            let line = strip_ansi(&line);
                            if line.is_empty() { continue; }
                            let raw = if spec.is_journal() {
                                match parse_journal_line(&line) {
                                    Some(r) => r,
                                    None => {
                                        note(&mut carry.notices, &line);
                                        continue;
                                    }
                                }
                            } else {
                                file_raw(&line, &ident)
                            };
                            // A journal that no longer has the saved position starts again from its oldest entries. Those
                            // were shown before (or are long gone): leave them out, and say so.
                            if resumed.is_some() && is_replay(raw.ts_us, carry.last_ts_us) {
                                replayed += 1;
                                continue;
                            }
                            if let Some(t) = raw.ts_us { carry.last_ts_us = carry.last_ts_us.max(t); }
                            if let Some(c) = raw.cursor.as_deref() {
                                if !carry.seen.new_cursor(c) { continue; }
                                carry.cursor = Some(c.to_string());
                            }
                            streamed = true;
                            if !limiter.allow(Instant::now()) {
                                dropped += 1;
                                continue;
                            }
                            carry.seq += 1;
                            pending.push(finish(raw, host_id, carry.seq, &mut carry.redactor, now_us()));
                            if pending.len() >= BATCH { flush(&mut pending, &mut dropped); }
                        }
                        dropped += std::mem::take(&mut lines.overlong);
                        if carry.notices.len() != notified_notice {
                            notified_notice = carry.notices.len();
                            state(HostState::Streaming, 0, None, None, Some(carry.notices.join(" ")));
                        }
                    }
                    LogEvent::End { code, error } => { ended = Some((code, error)); break; }
                }
            }
            _ = ticker.tick() => flush(&mut pending, &mut dropped),
        }
    }
    flush(&mut pending, &mut dropped);
    if replayed > 0 {
        sink.event(OpsEvent::Gap { host_id, reason: format!("The journal no longer has the last entry seen, so it started again from older entries ({replayed} left out). Entries written while the connection was down may be missing.") });
    }
    *ctl.session.lock().unwrap_or_else(|p| p.into_inner()) = None;
    monitors.close(session).await;

    if stopping || *cancel.borrow() {
        return Attempt::Cancelled;
    }
    let (code, error) = ended.unwrap_or((None, None));
    let notices = carry.notices.join(" ");
    let bad_cursor = resumed.is_some() && !streamed && code.is_some_and(|c| c != 0) && notices.to_ascii_lowercase().contains("cursor");
    match (error, code) {
        (Some(e), _) => Attempt::Ended { streamed, permanent: false, message: e, bad_cursor: false },
        (None, Some(c)) if c != 0 => {
            let message = if notices.is_empty() { format!("The command ended with status {c}.") } else { notices };
            Attempt::Ended { streamed, permanent: !bad_cursor, message, bad_cursor }
        }
        _ => Attempt::Ended { streamed, permanent: false, message: "The connection closed.".into(), bad_cursor: false },
    }
}

/// An entry much older than the newest one already shown, arriving on a resumed stream.
pub fn is_replay(ts_us: Option<i64>, newest_shown_us: i64) -> bool {
    const SLACK_US: i64 = 60_000_000;
    newest_shown_us > 0 && ts_us.is_some_and(|t| t + SLACK_US < newest_shown_us)
}

/// Keep a few lines the command printed that aren't log lines, once each.
fn note(notices: &mut Vec<String>, line: &str) {
    let t = line.trim();
    if t.is_empty() || t.starts_with("-- ") {
        return;
    }
    let (text, _) = cut(&mask::secrets(t), 200);
    if notices.len() < 4 && !notices.contains(&text) {
        notices.push(text);
    }
}

fn now_us() -> i64 {
    chrono::Utc::now().timestamp_micros()
}

#[cfg(test)]
mod tests;
