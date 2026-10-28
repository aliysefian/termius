//! The scheduler: runs each host's checks on its own interval, feeds the readings to the rules, and sends an alert
//! when a state is confirmed to have changed.
//!
//! One supervisor task wakes every second. A host whose time has come gets its own task, at most 16 at once, so a
//! slow or dead host never holds up the others and nothing here touches the window's thread. The supervisor is the
//! owner of those tasks: stopping it (locking the vault) cancels all of them.
//!
//! The engine knows nothing about Tauri, the vault or SSH. What it needs from them (whether the vault is unlocked, the
//! stored settings, a way to run a script over SSH, somewhere to send events) is [`EngineHost`], so the tests give it
//! stand-ins.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

use super::checks::tcp_check;
use super::metrics;
use super::model::{AlertRule, ChannelKind, CheckSpec, Metric, MonitorTarget, NotifyChannels, MIN_SSH_INTERVAL_SECS};
use super::notify::bot::BotChannel;
use super::notify::smtp::SmtpChannel;
use super::notify::{BoxFuture, Channel, Delivery, Dispatcher, Notification};
use super::ping;
use super::rules::{classify, describe, Evaluator, StateKey, Status, Transition};

const TICK: Duration = Duration::from_secs(1);
const RELOAD_EVERY: Duration = Duration::from_secs(30);
const MAX_PARALLEL: usize = 16;
const HISTORY: usize = 200;
const TCP_LIMIT: Duration = Duration::from_secs(4);
const SSH_LIMIT: Duration = Duration::from_secs(15);
/// Connecting, logging in and running the script.
const SSH_TOTAL: Duration = Duration::from_secs(45);

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Everything stored that the engine acts on.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub targets: Vec<(Uuid, MonitorTarget)>,
    pub rules: Vec<(Uuid, AlertRule)>,
    pub channels: NotifyChannels,
}

// -- what goes to the window ----------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PingView {
    pub loss_pct: f64,
    pub avg_ms: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortView {
    pub port: u16,
    pub open: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricsView {
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    pub disk: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateView {
    pub rule_id: Uuid,
    pub rule: String,
    pub subject: String,
    pub status: Status,
    pub value: Option<f64>,
    pub since: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetStatus {
    pub id: Uuid,
    pub checked_at: u64,
    pub ping: Option<PingView>,
    pub ports: Vec<PortView>,
    pub metrics: Option<MetricsView>,
    pub states: Vec<StateView>,
    /// Checks that could not run, in words.
    pub errors: Vec<String>,
}

/// One alert that was raised, and how it went out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlertEntry {
    pub at: u64,
    pub target_id: Uuid,
    pub target: String,
    pub rule: String,
    pub status: Status,
    pub previous: Status,
    pub summary: String,
    pub deliveries: Vec<Delivery>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    Status(TargetStatus),
    Alert(AlertEntry),
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub running: bool,
    /// The vault is locked, so nothing is being checked.
    pub paused: bool,
    pub statuses: Vec<TargetStatus>,
    pub history: Vec<AlertEntry>,
    /// Why the stored settings could not be read, if they could not.
    pub config_error: Option<String>,
}

// -- what the engine needs from outside ------------------------------------------------------------------------------

pub trait EngineHost: Send + Sync + 'static {
    fn unlocked(&self) -> bool;
    fn load(&self) -> Result<Config, String>;
    /// Run `script` on the saved SSH host and return what it printed. The connection is the host's job to keep.
    fn ssh_exec<'a>(&'a self, host_id: Uuid, script: &'a str, limit: Duration) -> BoxFuture<'a, Result<String, String>>;
    fn emit(&self, event: EngineEvent);
    /// The channels to send on, and the ones that are switched on but cannot be built (so the failure is shown).
    fn channels(&self, cfg: &NotifyChannels) -> (Vec<Arc<dyn Channel>>, Vec<Delivery>) {
        build_channels(cfg)
    }
}

/// The real channels for what is switched on.
pub fn build_channels(cfg: &NotifyChannels) -> (Vec<Arc<dyn Channel>>, Vec<Delivery>) {
    let mut ok: Vec<Arc<dyn Channel>> = Vec::new();
    let mut failed: Vec<Delivery> = Vec::new();
    for (kind, bot) in [(ChannelKind::Telegram, &cfg.telegram), (ChannelKind::Bale, &cfg.bale)] {
        if !bot.enabled {
            continue;
        }
        match BotChannel::new(kind, bot) {
            Ok(c) => ok.push(Arc::new(c)),
            Err(e) => failed.push(Delivery { channel: kind, error: Some(e.0) }),
        }
    }
    if cfg.email.enabled {
        match SmtpChannel::new(&cfg.email) {
            Ok(c) => ok.push(Arc::new(c)),
            Err(e) => failed.push(Delivery { channel: ChannelKind::Email, error: Some(e.0) }),
        }
    }
    (ok, failed)
}

/// Send a test message on one channel, whether or not it is switched on, to see that the settings work.
pub async fn send_test(cfg: &NotifyChannels, kind: ChannelKind) -> Result<(), String> {
    let channel: Arc<dyn Channel> = match kind {
        ChannelKind::Telegram => Arc::new(BotChannel::new(kind, &cfg.telegram).map_err(|e| e.0)?),
        ChannelKind::Bale => Arc::new(BotChannel::new(kind, &cfg.bale).map_err(|e| e.0)?),
        ChannelKind::Email => Arc::new(SmtpChannel::new(&cfg.email).map_err(|e| e.0)?),
    };
    let n = Notification {
        status: Status::Ok,
        previous: Status::Ok,
        target: "SSHVault".into(),
        address: None,
        rule: "Test message".into(),
        summary: format!("This is a test. {} alerts from SSHVault will arrive here.", kind.label()),
        at: unix_now(),
        note: None,
    };
    let sent = tokio::time::timeout(Duration::from_secs(45), channel.send(&n)).await;
    match sent {
        Err(_) => Err("no answer within 45 seconds".into()),
        Ok(r) => r.map_err(|e| e.0),
    }
}

// -- the engine ---------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct State {
    cfg: Config,
    loaded_at: Option<Instant>,
    reload: bool,
    config_error: Option<String>,
    due: HashMap<Uuid, Instant>,
    ssh_last: HashMap<Uuid, Instant>,
    running: HashSet<Uuid>,
    /// Hosts to run everything on at the next pass, whatever their schedule.
    force: HashSet<Uuid>,
    eval: Evaluator,
    statuses: HashMap<Uuid, TargetStatus>,
    history: VecDeque<AlertEntry>,
}

struct Inner {
    state: Mutex<State>,
    wake: Notify,
    dispatcher: Dispatcher,
    paused: AtomicBool,
    /// Bumped on every start and stop, so a check that was still running when the engine was stopped does not write
    /// its result into the next run.
    generation: AtomicU64,
}

pub struct Engine {
    inner: Arc<Inner>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::with_dispatcher(Dispatcher::default())
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dispatcher(dispatcher: Dispatcher) -> Self {
        Self { inner: Arc::new(Inner { state: Mutex::default(), wake: Notify::new(), dispatcher, paused: AtomicBool::new(false), generation: AtomicU64::new(0) }), task: Mutex::new(None) }
    }

    /// Start (or restart) the scheduler.
    pub async fn start(&self, host: Arc<dyn EngineHost>) {
        self.stop();
        {
            let mut s = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            s.reload = true;
        }
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        let handle = tokio::spawn(supervise(inner, host));
        *self.task.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
    }

    /// Stop checking and forget everything read: statuses and the alert list are not kept across a lock.
    pub fn stop(&self) {
        if let Some(h) = self.task.lock().unwrap_or_else(|p| p.into_inner()).take() {
            h.abort();
        }
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        *self.inner.state.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }

    pub fn is_running(&self) -> bool {
        self.task.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Read the stored settings again at the next pass (they were just saved).
    pub fn reload(&self) {
        self.inner.state.lock().unwrap_or_else(|p| p.into_inner()).reload = true;
        self.inner.wake.notify_one();
    }

    /// Check one host now, every check, whatever its schedule.
    pub fn run_now(&self, id: Uuid) {
        {
            let mut s = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            s.force.insert(id);
        }
        self.inner.wake.notify_one();
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut statuses: Vec<TargetStatus> = s.statuses.values().cloned().collect();
        statuses.sort_by_key(|t| t.id);
        Snapshot {
            running: self.is_running(),
            paused: self.inner.paused.load(Ordering::Relaxed),
            statuses,
            history: s.history.iter().rev().cloned().collect(),
            config_error: s.config_error.clone(),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(h) = self.task.lock().unwrap_or_else(|p| p.into_inner()).take() {
            h.abort();
        }
    }
}

/// A host whose checks are to run now.
struct Job {
    id: Uuid,
    target: MonitorTarget,
    rules: Vec<(Uuid, AlertRule)>,
    channels: NotifyChannels,
    run_ssh: bool,
    generation: u64,
}

async fn supervise(inner: Arc<Inner>, host: Arc<dyn EngineHost>) {
    let permits = Arc::new(Semaphore::new(MAX_PARALLEL));
    // Owned here so that cancelling this task cancels every check in flight.
    let mut jobs = tokio::task::JoinSet::new();
    loop {
        while jobs.try_join_next().is_some() {}
        let unlocked = host.unlocked();
        inner.paused.store(!unlocked, Ordering::Relaxed);
        if unlocked {
            reload_if_needed(&inner, host.as_ref());
            for job in due_jobs(&inner) {
                let (inner, host, permits) = (Arc::clone(&inner), Arc::clone(&host), Arc::clone(&permits));
                jobs.spawn(async move {
                    let Ok(_permit) = permits.acquire_owned().await else { return };
                    run_job(&inner, host.as_ref(), job).await;
                });
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = inner.wake.notified() => {}
        }
    }
}

fn reload_if_needed(inner: &Inner, host: &dyn EngineHost) {
    let stale = {
        let s = inner.state.lock().unwrap_or_else(|p| p.into_inner());
        s.reload || s.loaded_at.is_none_or(|t| t.elapsed() >= RELOAD_EVERY)
    };
    if !stale {
        return;
    }
    let loaded = host.load();
    let mut s = inner.state.lock().unwrap_or_else(|p| p.into_inner());
    s.reload = false;
    s.loaded_at = Some(Instant::now());
    match loaded {
        Err(e) => s.config_error = Some(e),
        Ok(cfg) => {
            s.config_error = None;
            let targets: HashSet<Uuid> = cfg.targets.iter().map(|(id, _)| *id).collect();
            let rules: HashSet<Uuid> = cfg.rules.iter().map(|(id, _)| *id).collect();
            s.eval.retain(|k| targets.contains(&k.target) && rules.contains(&k.rule));
            s.statuses.retain(|id, _| targets.contains(id));
            s.due.retain(|id, _| targets.contains(id));
            s.ssh_last.retain(|id, _| targets.contains(id));
            s.force.retain(|id| targets.contains(id));
            s.cfg = cfg;
        }
    }
}

fn due_jobs(inner: &Inner) -> Vec<Job> {
    let mut s = inner.state.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let mut out = Vec::new();
    let targets = s.cfg.targets.clone();
    for (id, target) in targets {
        if !target.enabled || target.checks.is_empty() {
            s.force.remove(&id);
            continue;
        }
        // A request made while the host is being checked waits for that check to end.
        if s.running.contains(&id) {
            continue;
        }
        let forced = s.force.remove(&id);
        let ready = s.due.get(&id).is_none_or(|t| *t <= now);
        if !forced && !ready {
            continue;
        }
        let ssh_every = Duration::from_secs(u64::from(target.effective_interval().max(MIN_SSH_INTERVAL_SECS)));
        let run_ssh = forced || s.ssh_last.get(&id).is_none_or(|t| t.elapsed() >= ssh_every);
        s.running.insert(id);
        s.due.insert(id, now + Duration::from_secs(u64::from(target.effective_interval())));
        if run_ssh && target.has_ssh_check() {
            s.ssh_last.insert(id, now);
        }
        out.push(Job { id, target, rules: s.cfg.rules.clone(), channels: s.cfg.channels.clone(), run_ssh, generation: inner.generation.load(Ordering::SeqCst) });
    }
    out
}

// -- one host's checks ------------------------------------------------------------------------------------------------------

struct Reading {
    metric: Metric,
    /// Empty, or for example `tcp/22`.
    subject: String,
    /// `None`: the check could not run, so there is nothing to judge.
    value: Option<f64>,
}

#[derive(Default)]
struct Gathered {
    readings: Vec<Reading>,
    ping: Option<PingView>,
    ports: Vec<PortView>,
    metrics: Option<MetricsView>,
    errors: Vec<String>,
}

async fn gather(host: &dyn EngineHost, target: &MonitorTarget, run_ssh: bool) -> Gathered {
    let address = target.address.trim().to_string();
    let ping_spec = target.checks.iter().find_map(|c| match c {
        CheckSpec::Ping { count, timeout_ms } => Some((*count, *timeout_ms)),
        _ => None,
    });
    let ports: Vec<u16> = target
        .checks
        .iter()
        .filter_map(|c| match c {
            CheckSpec::Tcp { port } => Some(*port),
            _ => None,
        })
        .collect();
    let ssh_host = if run_ssh && target.has_ssh_check() { target.ssh_host_id } else { None };

    let (ping_result, tcp_results, ssh_result) = tokio::join!(
        async {
            match ping_spec {
                Some((count, timeout_ms)) => Some(ping::ping(&address, count, timeout_ms).await),
                None => None,
            }
        },
        async {
            let mut v = Vec::new();
            for p in &ports {
                v.push((*p, tcp_check(&address, *p, TCP_LIMIT).await));
            }
            v
        },
        async {
            match ssh_host {
                // Logging in has no deadline of its own, so bound the whole thing.
                Some(h) => Some(match tokio::time::timeout(SSH_TOTAL, host.ssh_exec(h, metrics::SCRIPT, SSH_LIMIT)).await {
                    Ok(r) => r,
                    Err(_) => Err("the host did not answer in time".to_string()),
                }),
                None => None,
            }
        }
    );

    let mut g = Gathered::default();
    match ping_result {
        Some(Ok(r)) => {
            g.readings.push(Reading { metric: Metric::PingLoss, subject: String::new(), value: Some(r.loss_pct) });
            g.readings.push(Reading { metric: Metric::PingLatency, subject: String::new(), value: r.avg_ms });
            g.ping = Some(PingView { loss_pct: r.loss_pct, avg_ms: r.avg_ms, error: r.error });
        }
        Some(Err(e)) => {
            g.readings.push(Reading { metric: Metric::PingLoss, subject: String::new(), value: None });
            g.readings.push(Reading { metric: Metric::PingLatency, subject: String::new(), value: None });
            g.errors.push(format!("Ping: {e}"));
        }
        None => {}
    }
    for (port, r) in tcp_results {
        g.readings.push(Reading { metric: Metric::PortClosed, subject: format!("tcp/{port}"), value: Some(if r.open { 0.0 } else { 1.0 }) });
        g.ports.push(PortView { port, open: r.open, latency_ms: r.latency_ms, error: r.error });
    }
    match ssh_result {
        Some(Ok(out)) => {
            let m = metrics::parse(&out);
            for (metric, value) in [(Metric::Cpu, m.cpu), (Metric::Mem, m.mem), (Metric::Disk, m.disk)] {
                g.readings.push(Reading { metric, subject: String::new(), value });
            }
            g.metrics = Some(MetricsView { cpu: m.cpu, mem: m.mem, disk: m.disk });
        }
        Some(Err(e)) => {
            for metric in [Metric::Cpu, Metric::Mem, Metric::Disk] {
                g.readings.push(Reading { metric, subject: String::new(), value: None });
            }
            g.errors.push(format!("SSH: {e}"));
        }
        None => {}
    }
    g
}

async fn run_job(inner: &Inner, host: &dyn EngineHost, job: Job) {
    let g = gather(host, &job.target, job.run_ssh).await;
    let now = unix_now();
    if inner.generation.load(Ordering::SeqCst) != job.generation {
        return;
    }

    // Judge the readings under the lock; send afterwards, without it.
    let (status, transitions) = {
        let mut s = inner.state.lock().unwrap_or_else(|p| p.into_inner());
        s.running.remove(&job.id);
        let mut transitions: Vec<(Transition, AlertRule, f64)> = Vec::new();
        let mut states: Vec<StateView> = Vec::new();
        for (rule_id, rule) in &job.rules {
            if !rule.enabled || !rule.applies_to(job.id, &job.target.tags) {
                continue;
            }
            for r in g.readings.iter().filter(|r| r.metric == rule.metric) {
                let key = StateKey { target: job.id, rule: *rule_id, subject: r.subject.clone() };
                let raw = match r.value {
                    Some(v) => classify(rule.op, rule.warn, rule.crit, v),
                    None => Status::Unknown,
                };
                if let Some(t) = s.eval.observe(&key, raw, r.value, rule.attempts, now) {
                    transitions.push((t, rule.clone(), r.value.unwrap_or(0.0)));
                }
                if let Some(shown) = s.eval.shown(&key) {
                    let name = if r.subject.is_empty() { rule.name.clone() } else { format!("{} [{}]", rule.name, r.subject) };
                    states.push(StateView { rule_id: *rule_id, rule: name, subject: r.subject.clone(), status: shown.status, value: shown.value, since: shown.since });
                }
            }
        }
        let status = TargetStatus { id: job.id, checked_at: now, ping: g.ping, ports: g.ports, metrics: g.metrics, states, errors: g.errors };
        s.statuses.insert(job.id, status.clone());
        (status, transitions)
    };
    host.emit(EngineEvent::Status(status));

    if transitions.is_empty() {
        return;
    }
    let (channels, unbuilt) = host.channels(&job.channels);
    for (t, rule, value) in transitions {
        let subject = t.key.subject.clone();
        let rule_name = if subject.is_empty() { rule.name.clone() } else { format!("{} [{}]", rule.name, subject) };
        let n = Notification {
            status: t.to,
            previous: t.from,
            target: job.target.label.clone(),
            address: job.channels.include_address.then(|| job.target.address.clone()),
            rule: rule_name.clone(),
            summary: describe(&rule, t.value.unwrap_or(value), t.to),
            at: t.at,
            note: None,
        };
        let mut deliveries = inner.dispatcher.dispatch(&n, &channels, &rule.channels).await;
        deliveries.extend(unbuilt.iter().filter(|d| rule.channels.is_empty() || rule.channels.contains(&d.channel)).cloned());
        let entry = AlertEntry { at: t.at, target_id: job.id, target: n.target, rule: rule_name, status: t.to, previous: t.from, summary: n.summary, deliveries };
        {
            let mut s = inner.state.lock().unwrap_or_else(|p| p.into_inner());
            s.history.push_back(entry.clone());
            while s.history.len() > HISTORY {
                s.history.pop_front();
            }
        }
        host.emit(EngineEvent::Alert(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::model::{Op, Scope};
    use crate::monitoring::notify::NotifyError;
    use std::sync::atomic::AtomicUsize;

    struct Recorder {
        kind: ChannelKind,
        got: Arc<Mutex<Vec<Notification>>>,
    }

    impl Channel for Recorder {
        fn kind(&self) -> ChannelKind {
            self.kind
        }
        fn send<'a>(&'a self, n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>> {
            Box::pin(async move {
                self.got.lock().unwrap().push(n.clone());
                Ok(())
            })
        }
    }

    struct FakeHost {
        unlocked: AtomicBool,
        cfg: Mutex<Config>,
        /// What the SSH script "prints"; `Err` is a login that failed.
        output: Mutex<Result<String, String>>,
        calls: AtomicUsize,
        got: Arc<Mutex<Vec<Notification>>>,
        events: Mutex<Vec<EngineEvent>>,
    }

    impl FakeHost {
        fn new(cfg: Config) -> Arc<Self> {
            Arc::new(Self {
                unlocked: AtomicBool::new(true),
                cfg: Mutex::new(cfg),
                output: Mutex::new(Ok("CPU=10\nMEM=10\nDISK=10\n".into())),
                calls: AtomicUsize::new(0),
                got: Arc::default(),
                events: Mutex::default(),
            })
        }
        fn say(&self, cpu: u32) {
            *self.output.lock().unwrap() = Ok(format!("CPU={cpu}\nMEM=10\nDISK=10\n"));
        }
        fn notified(&self) -> Vec<Status> {
            self.got.lock().unwrap().iter().map(|n| n.status).collect()
        }
        /// Rounds of checks that have finished (a new round can only start once the last is done).
        fn runs(&self) -> usize {
            self.events.lock().unwrap().iter().filter(|e| matches!(e, EngineEvent::Status(_))).count()
        }
    }

    impl EngineHost for FakeHost {
        fn unlocked(&self) -> bool {
            self.unlocked.load(Ordering::SeqCst)
        }
        fn load(&self) -> Result<Config, String> {
            Ok(self.cfg.lock().unwrap().clone())
        }
        fn ssh_exec<'a>(&'a self, _host_id: Uuid, _script: &'a str, _limit: Duration) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.output.lock().unwrap().clone()
            })
        }
        fn emit(&self, event: EngineEvent) {
            self.events.lock().unwrap().push(event);
        }
        fn channels(&self, _cfg: &NotifyChannels) -> (Vec<Arc<dyn Channel>>, Vec<Delivery>) {
            (vec![Arc::new(Recorder { kind: ChannelKind::Telegram, got: Arc::clone(&self.got) })], Vec::new())
        }
    }

    fn cpu_config(attempts: u8) -> (Config, Uuid) {
        let target_id = Uuid::new_v4();
        let target = MonitorTarget {
            label: "db-01".into(),
            address: "10.0.0.9".into(),
            tags: vec![],
            enabled: true,
            interval_secs: 3600,
            checks: vec![CheckSpec::SshMetrics],
            ssh_host_id: Some(Uuid::new_v4()),
            notes: String::new(),
        };
        let rule = AlertRule { name: "CPU".into(), enabled: true, metric: Metric::Cpu, op: Op::Gt, warn: Some(80.0), crit: Some(90.0), attempts, scope: Scope::All, channels: vec![] };
        (Config { targets: vec![(target_id, target)], rules: vec![(Uuid::new_v4(), rule)], channels: NotifyChannels::default() }, target_id)
    }

    fn quick() -> Engine {
        Engine::with_dispatcher(Dispatcher::default().with_retry_after(Duration::ZERO))
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..300 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("gave up waiting for {what}");
    }

    #[tokio::test]
    async fn a_confirmed_problem_is_announced_once_and_so_is_the_recovery() {
        let (cfg, id) = cpu_config(2);
        let host = FakeHost::new(cfg);
        host.say(95);
        let engine = quick();
        engine.start(host.clone()).await;
        // The first pass runs by itself: one reading, not yet confirmed.
        until("first check", || host.runs() >= 1).await;
        assert!(host.notified().is_empty());
        engine.run_now(id);
        until("the alert", || host.notified().len() == 1).await;
        assert_eq!(host.notified(), vec![Status::Critical]);
        // Still critical: nothing more.
        until("second check to finish", || host.runs() >= 2).await;
        engine.run_now(id);
        until("third check", || host.runs() >= 3).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(host.notified(), vec![Status::Critical]);
        // Better: one message, at once.
        host.say(10);
        engine.run_now(id);
        until("the recovery", || host.notified().len() == 2).await;
        assert_eq!(host.notified(), vec![Status::Critical, Status::Ok]);
        let n = host.got.lock().unwrap()[0].clone();
        assert_eq!((n.target.as_str(), n.rule.as_str()), ("db-01", "CPU"));
        assert!(n.summary.contains("CPU use is 95% (limit 90%)"), "{}", n.summary);
        assert!(n.address.is_none(), "addresses stay home unless asked for");
        let snap = engine.snapshot();
        assert!(snap.running && !snap.paused);
        assert_eq!(snap.history.len(), 2, "newest first");
        assert_eq!(snap.history[0].status, Status::Ok);
        assert_eq!(snap.statuses[0].states[0].status, Status::Ok);
        assert!(host.events.lock().unwrap().iter().any(|e| matches!(e, EngineEvent::Alert(_))));
        engine.stop();
        assert!(engine.snapshot().history.is_empty(), "nothing is kept once stopped");
    }

    #[tokio::test]
    async fn a_check_that_cannot_run_is_unknown_and_says_nothing() {
        let (cfg, id) = cpu_config(1);
        let host = FakeHost::new(cfg);
        *host.output.lock().unwrap() = Err("no credentials".into());
        let engine = quick();
        engine.start(host.clone()).await;
        until("a check", || host.runs() >= 1).await;
        engine.run_now(id);
        until("another", || host.runs() >= 2).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(host.notified().is_empty());
        let snap = engine.snapshot();
        assert_eq!(snap.statuses[0].errors, vec!["SSH: no credentials".to_string()]);
        assert!(snap.statuses[0].states.iter().all(|s| s.status == Status::Unknown));
        engine.stop();
    }

    #[tokio::test]
    async fn nothing_is_checked_while_the_vault_is_locked() {
        let (cfg, id) = cpu_config(1);
        let host = FakeHost::new(cfg);
        host.unlocked.store(false, Ordering::SeqCst);
        let engine = quick();
        engine.start(host.clone()).await;
        engine.run_now(id);
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert_eq!(host.calls.load(Ordering::SeqCst), 0);
        assert!(engine.snapshot().paused);
        host.unlocked.store(true, Ordering::SeqCst);
        engine.run_now(id);
        until("a check after unlocking", || host.runs() >= 1).await;
        assert!(!engine.snapshot().paused);
        engine.stop();
    }

    #[tokio::test]
    async fn disabled_hosts_and_rules_for_other_hosts_are_left_alone() {
        let (mut cfg, id) = cpu_config(1);
        cfg.targets[0].1.enabled = false;
        let host = FakeHost::new(cfg);
        host.say(99);
        let engine = quick();
        engine.start(host.clone()).await;
        engine.run_now(id);
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(host.calls.load(Ordering::SeqCst), 0);
        // Turn it on and scope the rule to some other host: checked, but nobody is told.
        {
            let mut c = host.cfg.lock().unwrap();
            c.targets[0].1.enabled = true;
            c.rules[0].1.scope = Scope::Target { id: Uuid::new_v4() };
        }
        engine.reload();
        engine.run_now(id);
        until("a check", || host.runs() >= 1).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(host.notified().is_empty());
        engine.stop();
    }

    #[tokio::test]
    async fn the_port_check_finds_a_closed_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let id = Uuid::new_v4();
        let target = MonitorTarget {
            label: "local".into(),
            address: "127.0.0.1".into(),
            tags: vec![],
            enabled: true,
            interval_secs: 3600,
            checks: vec![CheckSpec::Tcp { port }],
            ssh_host_id: None,
            notes: String::new(),
        };
        let rule = AlertRule { name: "Port".into(), enabled: true, metric: Metric::PortClosed, op: Op::Gt, warn: None, crit: Some(0.0), attempts: 1, scope: Scope::All, channels: vec![] };
        let host = FakeHost::new(Config { targets: vec![(id, target)], rules: vec![(Uuid::new_v4(), rule)], channels: NotifyChannels::default() });
        let engine = quick();
        engine.start(host.clone()).await;
        until("the first status", || !engine.snapshot().statuses.is_empty()).await;
        assert!(host.notified().is_empty(), "open port, nothing to say");
        drop(listener);
        engine.run_now(id);
        until("the alert", || !host.notified().is_empty()).await;
        let n = host.got.lock().unwrap()[0].clone();
        assert_eq!(n.status, Status::Critical);
        assert_eq!(n.rule, format!("Port [tcp/{port}]"));
        engine.stop();
    }

    #[test]
    fn switched_on_channels_that_cannot_be_built_are_reported_not_skipped() {
        let mut cfg = NotifyChannels::default();
        cfg.email.enabled = true;
        let (ok, failed) = build_channels(&cfg);
        assert!(ok.is_empty());
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].channel, ChannelKind::Email);
        cfg.email.enabled = false;
        cfg.bale.enabled = true;
        cfg.bale.bot_token = "1:a".into();
        cfg.bale.chat_id = "5".into();
        let (ok, failed) = build_channels(&cfg);
        assert_eq!((ok.len(), failed.len()), (1, 0));
        assert_eq!(ok[0].kind(), ChannelKind::Bale);
    }
}
