//! The one place host readings are taken. Every view (the host window, the dashboard, the terminal's side panel, the
//! alert checks) asks this manager for a sample instead of reading the host itself, so:
//!
//! * a sample that is recent enough is **shared**, not taken again (a per-host minimum interval and a caller's `max_age`);
//! * two callers asking at once wait for **one** collection (a per-host gate; the second one then finds it fresh);
//! * at most `max_concurrent` hosts are read at the same time, so a hundred hosts do not mean a hundred logins at once;
//! * a host that fails is left alone for a growing time (1x, 2x, 4x... up to a minute), not hammered;
//! * a collection can be **cancelled**, and dropping the caller's future cancels it too;
//! * history is a bounded ring of small points per host, in memory only.
//!
//! Nothing here knows about Tauri, SSH or the vault: how a script reaches a host is behind [`HostExec`]. The app's
//! implementation resolves the host's saved credentials, so a locked vault means no collection, whoever asks.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::derive::derive_with;
use super::logins;
use super::model::{Gpu, HistoryPoint, LoginReport, Platform, PortList, ProcessList, Sample, StaticInfo};
use super::parse::{self, Raw};
use super::ports;
use super::procs::{self, SignalError, SignalOutcome};
use super::script::{self, Options};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Runs a script on a saved host and returns what it printed. The connection is the implementor's to keep and reuse.
pub trait HostExec: Send + Sync + 'static {
    fn exec<'a>(&'a self, host: Uuid, script: &'a str, limit: Duration) -> BoxFuture<'a, Result<String, String>>;
}

/// Told about every sample that was actually collected (not about cache hits), so the window can be pushed one event
/// per reading however many views are open.
pub trait EventSink: Send + Sync + 'static {
    fn sampled(&self, sample: &Arc<Sample>);
    fn failed(&self, host: Uuid, error: &MetricsError);
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetricsError {
    /// Logging in or running the script failed; the text is the host's or the connection's own.
    Exec { message: String },
    /// The host answered with something that is not a reading.
    Unreadable { message: String },
    Timeout,
    Cancelled,
    /// The last attempt failed a moment ago; trying again now would only add load to a host that is struggling.
    Cooling { retry_in_secs: f64, last: String },
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricsError::Exec { message } | MetricsError::Unreadable { message } => write!(f, "{message}"),
            MetricsError::Timeout => write!(f, "the host did not answer in time"),
            MetricsError::Cancelled => write!(f, "cancelled"),
            MetricsError::Cooling { retry_in_secs, last } => write!(f, "{last} (trying again in {:.0} s)", retry_in_secs.ceil()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The shortest time between two collections on one host. Faster asks are answered from the last sample.
    pub min_interval: Duration,
    pub max_concurrent: usize,
    /// How long one collection may take, login included.
    pub timeout: Duration,
    /// Points kept per host for charts.
    pub history_len: usize,
    /// How often the rarely-changing facts (CPU model, kernel) are read again.
    pub static_every: Duration,
    pub backoff_max: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { min_interval: Duration::from_secs(1), max_concurrent: 4, timeout: Duration::from_secs(15), history_len: 3600, static_every: Duration::from_secs(3600), backoff_max: Duration::from_secs(60) }
    }
}

/// How often `nvidia-smi` is asked: it starts a program, so it is not on every reading.
const NVIDIA_EVERY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Data {
    /// NVIDIA GPUs from the last time `nvidia-smi` was asked, carried into the readings between.
    nvidia: Vec<Gpu>,
    nvidia_at: Option<Instant>,
    procs: Option<(Instant, Arc<ProcessList>)>,
    procs_prev: Option<procs::Snap>,
    ports: Option<(Instant, Arc<PortList>)>,
    logins: Option<(Instant, Arc<LoginReport>)>,
    prev: Option<Raw>,
    latest: Option<(Instant, Arc<Sample>)>,
    info: StaticInfo,
    info_at: Option<Instant>,
    history: VecDeque<HistoryPoint>,
    failures: u32,
    last_failure: Option<(Instant, String)>,
}

struct Slot {
    /// Held for the length of a collection: whoever comes next waits, then finds the new sample.
    gate: tokio::sync::Mutex<()>,
    data: Mutex<Data>,
    cancel: Mutex<CancellationToken>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Data {
    /// A cached view no older than `max_age`, whatever kind it is.
    fn pipe_cached<R: Clone>(&self, cached: &impl Fn(&Data) -> Option<(Instant, R)>, max_age: Duration) -> Option<R> {
        cached(self).filter(|(at, _)| at.elapsed() < max_age).map(|(_, r)| r)
    }
}

impl Slot {
    fn fresh(&self, max_age: Duration) -> Option<Arc<Sample>> {
        lock(&self.data).latest.as_ref().filter(|(at, _)| at.elapsed() < max_age).map(|(_, s)| Arc::clone(s))
    }

    fn token(&self) -> CancellationToken {
        lock(&self.cancel).clone()
    }

    fn cooling(&self, cfg: &Config) -> Option<MetricsError> {
        let d = lock(&self.data);
        let (at, last) = d.last_failure.as_ref()?;
        let backoff = cfg.min_interval.max(Duration::from_secs(2)).saturating_mul(1u32 << d.failures.saturating_sub(1).min(10)).min(cfg.backoff_max);
        let waited = at.elapsed();
        (waited < backoff).then(|| MetricsError::Cooling { retry_in_secs: (backoff - waited).as_secs_f64(), last: last.clone() })
    }
}

pub struct HostMetricsManager<E: HostExec> {
    exec: Arc<E>,
    cfg: Config,
    slots: Mutex<HashMap<Uuid, Arc<Slot>>>,
    permits: Arc<Semaphore>,
    sink: Option<Arc<dyn EventSink>>,
    collections: AtomicU64,
}

impl<E: HostExec> HostMetricsManager<E> {
    pub fn new(exec: Arc<E>, cfg: Config, sink: Option<Arc<dyn EventSink>>) -> Self {
        let permits = Arc::new(Semaphore::new(cfg.max_concurrent.max(1)));
        Self { exec, cfg, slots: Mutex::new(HashMap::new()), permits, sink, collections: AtomicU64::new(0) }
    }

    fn slot(&self, host: Uuid) -> Arc<Slot> {
        Arc::clone(lock(&self.slots).entry(host).or_insert_with(|| {
            Arc::new(Slot { gate: tokio::sync::Mutex::new(()), data: Mutex::new(Data::default()), cancel: Mutex::new(CancellationToken::new()) })
        }))
    }

    /// A reading no older than `max_age` (and never older than the minimum interval allows asking again). Collects one
    /// if there is none, or waits for the collection already under way. Callers may be many and unrelated.
    pub async fn sample(&self, host: Uuid, max_age: Duration) -> Result<Arc<Sample>, MetricsError> {
        let slot = self.slot(host);
        let max_age = max_age.max(self.cfg.min_interval);
        if let Some(s) = slot.fresh(max_age) {
            return Ok(s);
        }
        let _gate = slot.gate.lock().await;
        // Whoever held the gate may just have collected.
        if let Some(s) = slot.fresh(max_age) {
            return Ok(s);
        }
        if let Some(e) = slot.cooling(&self.cfg) {
            return Err(e);
        }
        let token = slot.token();
        let _permit = tokio::select! {
            p = Arc::clone(&self.permits).acquire_owned() => p.map_err(|_| MetricsError::Cancelled)?,
            _ = token.cancelled() => return Err(MetricsError::Cancelled),
        };
        let opts = {
            let d = lock(&slot.data);
            Options {
                with_static: d.info_at.is_none_or(|at| at.elapsed() >= self.cfg.static_every),
                // Only once the first reading has said the tool is there, and not more often than it is worth.
                nvidia: d.latest.as_ref().is_some_and(|(_, s)| s.capabilities.nvidia_smi) && d.nvidia_at.is_none_or(|at| at.elapsed() >= NVIDIA_EVERY),
            }
        };
        let script = script::script_with(opts);
        let ran = self.run(host, &token, &script).await;
        let outcome = ran.and_then(|out| parse::parse(&out).map_err(|e| MetricsError::Unreadable { message: e.to_string() }));
        match outcome {
            Ok(raw) => Ok(self.accept(host, &slot, raw)),
            Err(e) => {
                // A cancel is the caller's own doing, not the host's fault: it does not start a back-off.
                if e != MetricsError::Cancelled {
                    let mut d = lock(&slot.data);
                    d.failures = d.failures.saturating_add(1);
                    d.last_failure = Some((Instant::now(), e.to_string()));
                }
                if let Some(sink) = &self.sink {
                    sink.failed(host, &e);
                }
                Err(e)
            }
        }
    }

    /// Run a script on the host: counted, bounded by the timeout, and stopped by a cancel. The caller holds the host's gate
    /// and a permit.
    async fn run(&self, host: Uuid, token: &CancellationToken, script: &str) -> Result<String, MetricsError> {
        self.collections.fetch_add(1, Ordering::Relaxed);
        tokio::select! {
            r = tokio::time::timeout(self.cfg.timeout, self.exec.exec(host, script, self.cfg.timeout)) => match r {
                Ok(Ok(out)) => Ok(out),
                Ok(Err(message)) => Err(MetricsError::Exec { message }),
                Err(_) => Err(MetricsError::Timeout),
            },
            _ = token.cancelled() => Err(MetricsError::Cancelled),
        }
    }

    /// The common shape of an on-demand view (processes, ports, logins): a fresh enough cached copy, else one reading under
    /// the host's gate and a permit. A host that is failing is not asked; a failure here does not start a back-off of its own,
    /// because a view this host cannot give (no `last`, say) is not the host being down.
    async fn view<T, R: Clone>(
        &self,
        host: Uuid,
        max_age: Duration,
        cached: impl Fn(&Data) -> Option<(Instant, R)>,
        script: impl Fn(&Data) -> String,
        build: impl Fn(&mut Data, &str) -> Result<T, MetricsError>,
        store: impl Fn(&mut Data, Instant, &T) -> R,
    ) -> Result<R, MetricsError> {
        let slot = self.slot(host);
        let max_age = max_age.max(self.cfg.min_interval);
        let fresh = |slot: &Slot| lock(&slot.data).pipe_cached(&cached, max_age);
        if let Some(r) = fresh(&slot) {
            return Ok(r);
        }
        let _gate = slot.gate.lock().await;
        if let Some(r) = fresh(&slot) {
            return Ok(r);
        }
        if let Some(e) = slot.cooling(&self.cfg) {
            return Err(e);
        }
        let token = slot.token();
        let _permit = tokio::select! {
            p = Arc::clone(&self.permits).acquire_owned() => p.map_err(|_| MetricsError::Cancelled)?,
            _ = token.cancelled() => return Err(MetricsError::Cancelled),
        };
        let text = script(&lock(&slot.data));
        let out = self.run(host, &token, &text).await?;
        let mut d = lock(&slot.data);
        let built = build(&mut d, &out)?;
        Ok(store(&mut d, Instant::now(), &built))
    }

    /// The processes of the host, busiest first (at most 500). The first list after a while is read twice, a second apart,
    /// so it has CPU figures at once; later ones are compared with the one before.
    pub async fn processes(&self, host: Uuid, max_age: Duration) -> Result<Arc<ProcessList>, MetricsError> {
        self.view(
            host,
            max_age,
            |d| d.procs.clone(),
            |d| procs::script(d.procs_prev.is_none()),
            |d, out| {
                let raw = procs::parse(out).map_err(|e| MetricsError::Unreadable { message: e.to_string() })?;
                if raw.os != "Linux" {
                    return Err(MetricsError::Unreadable { message: format!("Processes are not listed for {} yet: only Linux hosts are read.", if raw.os.is_empty() { "this system" } else { &raw.os }) });
                }
                let list = procs::derive(host, now_ms(), d.procs_prev.as_ref(), &raw);
                d.procs_prev = Some(raw.now);
                Ok(list)
            },
            |d, at, list| {
                let a = Arc::new(list.clone());
                d.procs = Some((at, Arc::clone(&a)));
                a
            },
        )
        .await
    }

    /// What is listening on the host.
    pub async fn ports(&self, host: Uuid, max_age: Duration) -> Result<Arc<PortList>, MetricsError> {
        self.view(
            host,
            max_age,
            |d| d.ports.clone(),
            |_| ports::SCRIPT.to_string(),
            |_, out| match ports::parse(host, now_ms(), out).map_err(|e| MetricsError::Unreadable { message: e.to_string() })? {
                Some(l) => Ok(l),
                None => Err(MetricsError::Unreadable { message: "Listening ports are only read for Linux hosts so far.".into() }),
            },
            |d, at, list| {
                let a = Arc::new(list.clone());
                d.ports = Some((at, Arc::clone(&a)));
                a
            },
        )
        .await
    }

    /// Logins and SSH authentication events, with what could not be read and why.
    pub async fn logins(&self, host: Uuid, max_age: Duration) -> Result<Arc<LoginReport>, MetricsError> {
        self.view(
            host,
            max_age,
            |d| d.logins.clone(),
            |_| logins::SCRIPT.to_string(),
            |_, out| match logins::parse(host, now_ms(), out).map_err(|e| MetricsError::Unreadable { message: e.to_string() })? {
                Some(l) => Ok(l),
                None => Err(MetricsError::Unreadable { message: "Logins are only read for Linux hosts so far.".into() }),
            },
            |d, at, list| {
                let a = Arc::new(list.clone());
                d.logins = Some((at, Arc::clone(&a)));
                a
            },
        )
        .await
    }

    /// Send a signal to one process, after the host has checked that the pid is still the process that was shown (same
    /// name, same start time). One attempt only: a timeout is reported as unknown and never retried.
    pub async fn signal(&self, host: Uuid, pid: u32, start_ticks: u64, name: &str, signal: &str) -> Result<SignalOutcome, SignalError> {
        let script = procs::signal_script(pid, start_ticks, name, signal)?;
        let slot = self.slot(host);
        let _gate = slot.gate.lock().await;
        let token = slot.token();
        let Ok(_permit) = Arc::clone(&self.permits).acquire_owned().await else { return Ok(SignalOutcome::Unknown) };
        let outcome = match self.run(host, &token, &script).await {
            Ok(out) => procs::parse_signal(&out),
            Err(MetricsError::Timeout | MetricsError::Cancelled) => SignalOutcome::Unknown,
            Err(e) => SignalOutcome::Refused { reason: e.to_string() },
        };
        // The list on screen is out of date now: the next ask reads a new one.
        lock(&slot.data).procs = None;
        Ok(outcome)
    }

    fn accept(&self, host: Uuid, slot: &Slot, raw: Raw) -> Arc<Sample> {
        let sample = {
            let mut d = lock(&slot.data);
            if let Some(info) = &raw.info {
                d.info = info.clone();
            }
            d.info_at = Some(Instant::now());
            if let Some(n) = &raw.nvidia {
                d.nvidia = n.clone();
                d.nvidia_at = Some(Instant::now());
            }
            let (sample, keep) = derive_with(host, now_ms(), d.prev.as_ref(), &raw, &d.info, &d.nvidia);
            if keep && raw.linux {
                d.prev = Some(raw);
            }
            d.failures = 0;
            d.last_failure = None;
            let sample = Arc::new(sample);
            if matches!(sample.platform, Platform::Linux) {
                d.history.push_back(HistoryPoint::of(&sample));
                while d.history.len() > self.cfg.history_len {
                    d.history.pop_front();
                }
            }
            d.latest = Some((Instant::now(), Arc::clone(&sample)));
            sample
        };
        if let Some(sink) = &self.sink {
            sink.sampled(&sample);
        }
        sample
    }

    /// The last sample taken, however old, and how old it is. Never touches the host.
    pub fn latest(&self, host: Uuid) -> Option<(Arc<Sample>, Duration)> {
        let slot = lock(&self.slots).get(&host).cloned()?;
        let d = lock(&slot.data);
        d.latest.as_ref().map(|(at, s)| (Arc::clone(s), at.elapsed()))
    }

    /// The last sample of every host that has one, for ranking and summaries. Never touches a host.
    pub fn all_latest(&self) -> Vec<(Arc<Sample>, Duration)> {
        let slots: Vec<Arc<Slot>> = lock(&self.slots).values().cloned().collect();
        slots.iter().filter_map(|s| lock(&s.data).latest.as_ref().map(|(at, x)| (Arc::clone(x), at.elapsed()))).collect()
    }

    /// Kept points of a host from `since_ms` (milliseconds since 1970), oldest first.
    pub fn history(&self, host: Uuid, since_ms: u64) -> Vec<HistoryPoint> {
        let Some(slot) = lock(&self.slots).get(&host).cloned() else { return Vec::new() };
        let d = lock(&slot.data);
        d.history.iter().filter(|p| p.t >= since_ms).cloned().collect()
    }

    /// Stop the collection running on a host, if any. The next `sample` starts normally.
    pub fn cancel(&self, host: Uuid) {
        if let Some(slot) = lock(&self.slots).get(&host) {
            let mut t = lock(&slot.cancel);
            t.cancel();
            *t = CancellationToken::new();
        }
    }

    /// Forget a host: its samples, history and baseline. A running collection is cancelled.
    pub fn forget(&self, host: Uuid) {
        self.cancel(host);
        lock(&self.slots).remove(&host);
    }

    /// Forget everything: for when the vault locks, so nothing about a host's load stays behind.
    pub fn clear(&self) {
        let all: Vec<Arc<Slot>> = lock(&self.slots).drain().map(|(_, s)| s).collect();
        for s in all {
            lock(&s.cancel).cancel();
        }
    }

    pub fn host_count(&self) -> usize {
        lock(&self.slots).len()
    }

    /// How many collections have been run (not cache hits). For tests and diagnostics.
    pub fn collections(&self) -> u64 {
        self.collections.load(Ordering::Relaxed)
    }
}
