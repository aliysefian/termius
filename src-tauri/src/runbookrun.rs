//! Running a runbook on hosts: one SSH connection per host for all its steps, hosts taken by a strategy (all at once up to a
//! limit, one by one, or in rolling batches), a live feed of what happens including each step's output as it is produced, and
//! a record of every run kept on this computer (outputs can hold secrets, so it is never synced).
//!
//! The steps of one host always run in order; the strategy is only about how many hosts are in flight and when to stop
//! starting more.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::containers::transport::shell_quote;
use crate::runbook::{run_host_opts, Executor, Output, OutputSink, Progress, RunOptions, Runbook, StepResult, StepStatus, Strategy, Stream, MAX_DOWNLOAD};
use crate::runbookhistory::{now, now_ms, History, HostRecord, HostStatus, RunRecord};
use crate::runner::{push_capped, HangUpOnDrop, CONCURRENCY};
use crate::ssh::{open_client, Client, Target};

/// A file chosen for an upload: at most this large, and this much in all.
pub const MAX_FILE: usize = 8 * 1024 * 1024;
pub const MAX_FILES_TOTAL: usize = 32 * 1024 * 1024;
/// How much of one step's output is sent live. The step's result still holds what the record keeps.
const LIVE_BYTES_PER_STEP: usize = 512 * 1024;
/// How long a connection test waits for one host.
const CONNECT_TEST: Duration = Duration::from_secs(20);

// -- over SSH ----------------------------------------------------------------------------

pub struct SshExec {
    client: Client,
}

/// Splits a stream of bytes into whole lines, so a secret is never cut in two between pieces that are shown.
struct Lines {
    pending: Vec<u8>,
    sent: usize,
    capped: bool,
}

impl Lines {
    fn new() -> Self {
        Self { pending: Vec::new(), sent: 0, capped: false }
    }

    /// Hand every complete line in what has arrived so far to `emit`, in one piece.
    fn push(&mut self, data: &[u8], stream: Stream, out: &dyn OutputSink) {
        self.pending.extend_from_slice(data);
        if let Some(end) = self.pending.iter().rposition(|b| *b == b'\n') {
            let whole: Vec<u8> = self.pending.drain(..=end).collect();
            self.emit(&whole, stream, out);
        }
        // A very long line with no break is sent as it is rather than held for ever.
        if self.pending.len() > 64 * 1024 {
            let all = std::mem::take(&mut self.pending);
            self.emit(&all, stream, out);
        }
    }

    fn flush(&mut self, stream: Stream, out: &dyn OutputSink) {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.emit(&rest, stream, out);
        }
    }

    fn emit(&mut self, bytes: &[u8], stream: Stream, out: &dyn OutputSink) {
        if self.capped {
            return;
        }
        if self.sent + bytes.len() > LIVE_BYTES_PER_STEP {
            self.capped = true;
            out.chunk(stream, "(more output than is shown live; the step's result keeps the first part)\n");
            return;
        }
        self.sent += bytes.len();
        out.chunk(stream, &String::from_utf8_lossy(bytes));
    }
}

impl SshExec {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn run(&self, command: &str, stdin: Option<&[u8]>, live: Option<&dyn OutputSink>) -> Result<Output, String> {
        let started = Instant::now();
        let mut guard = HangUpOnDrop(Some(self.client.channel_open_session().await.map_err(|e| e.to_string())?));
        let channel = guard.0.as_mut().ok_or("no channel")?;
        channel.exec(true, command.as_bytes()).await.map_err(|e| e.to_string())?;
        if let Some(data) = stdin {
            channel.data(data).await.map_err(|e| e.to_string())?;
            channel.eof().await.map_err(|e| e.to_string())?;
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (mut out_lines, mut err_lines) = (Lines::new(), Lines::new());
        let mut truncated = false;
        let mut exit_code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    push_capped(&mut out, &data, &mut truncated);
                    if let Some(l) = live {
                        out_lines.push(&data, Stream::Stdout, l);
                    }
                }
                ChannelMsg::ExtendedData { data, ext: 1 } => {
                    push_capped(&mut err, &data, &mut truncated);
                    if let Some(l) = live {
                        err_lines.push(&data, Stream::Stderr, l);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        if let Some(l) = live {
            out_lines.flush(Stream::Stdout, l);
            err_lines.flush(Stream::Stderr, l);
        }
        // It finished by itself: nothing to hang up.
        guard.0 = None;
        Ok(Output {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            exit_code,
            truncated,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// The bytes of a remote file, at most [`MAX_DOWNLOAD`] of them.
    async fn read_file(&self, remote: &str) -> Result<Vec<u8>, String> {
        let mut guard = HangUpOnDrop(Some(self.client.channel_open_session().await.map_err(|e| e.to_string())?));
        let channel = guard.0.as_mut().ok_or("no channel")?;
        channel.exec(true, format!("cat -- {}", shell_quote(remote)).as_bytes()).await.map_err(|e| e.to_string())?;
        let (mut data, mut err) = (Vec::new(), Vec::new());
        let mut exit_code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data: d } => {
                    if data.len() + d.len() > MAX_DOWNLOAD {
                        // Dropping the guard hangs up on the remote `cat`.
                        return Err(format!("the file is larger than {} MB", MAX_DOWNLOAD / (1024 * 1024)));
                    }
                    data.extend_from_slice(&d);
                }
                ChannelMsg::ExtendedData { data: d, ext: 1 } => {
                    if err.len() < 4096 {
                        err.extend_from_slice(&d);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        guard.0 = None;
        if exit_code == Some(0) {
            Ok(data)
        } else {
            let why = String::from_utf8_lossy(&err).trim().to_string();
            Err(if why.is_empty() { format!("could not read {remote}") } else { why })
        }
    }
}

impl Executor for SshExec {
    async fn exec(&self, command: &str, timeout: Duration) -> Result<Output, String> {
        tokio::time::timeout(timeout, self.run(command, None, None)).await.map_err(|_| format!("took longer than {} s", timeout.as_secs()))?
    }

    async fn exec_stream<'a>(&'a self, command: &'a str, timeout: Duration, out: &'a dyn OutputSink) -> Result<Output, String> {
        tokio::time::timeout(timeout, self.run(command, None, Some(out))).await.map_err(|_| format!("took longer than {} s", timeout.as_secs()))?
    }

    async fn upload(&self, data: &[u8], remote: &str, mode: Option<&str>) -> Result<Output, String> {
        if data.len() > MAX_FILE {
            return Err("the file is too large".into());
        }
        // Written beside the target and moved into place, so a failed upload never leaves half a file.
        let tmp = format!("{remote}.sshvault-upload");
        let (r, t) = (shell_quote(remote), shell_quote(&tmp));
        let chmod = mode.map(|m| format!(" && chmod {m} {t}")).unwrap_or_default();
        let command = format!("cat > {t}{chmod} && mv -f {t} {r} || {{ rm -f {t}; exit 1; }}");
        tokio::time::timeout(Duration::from_secs(300), self.run(&command, Some(data), None)).await.map_err(|_| "the upload took longer than 5 minutes".to_string())?
    }

    async fn download<'a>(&'a self, remote: &'a str) -> Result<Vec<u8>, String> {
        tokio::time::timeout(Duration::from_secs(300), self.read_file(remote)).await.map_err(|_| "the download took longer than 5 minutes".to_string())?
    }
}

// -- the run -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RunbookEvent {
    /// Work on this host has begun: it is being connected to.
    HostStarted { host_id: Uuid },
    /// The connection is up and the steps are about to run.
    Connected { host_id: Uuid },
    StepStarted { host_id: Uuid, index: usize },
    /// A piece of a step's output, as it is produced, whole lines at a time and with secrets hidden. `seq` counts the pieces of
    /// one host, so a reader can tell none was lost.
    Output { host_id: Uuid, index: usize, stream: Stream, text: String, seq: u64 },
    Step { host_id: Uuid, result: StepResult },
    HostDone { host_id: Uuid, ok: bool, error: Option<String>, status: HostStatus, duration_ms: u64 },
    Done { cancelled: bool },
}

pub trait RunbookSink: Send + Sync + 'static {
    fn event(&self, e: RunbookEvent);
}

/// One host to run on: its target, or why there isn't one.
pub struct HostJob {
    pub host_id: Uuid,
    pub label: String,
    pub hostname: String,
    pub target: Result<Target, String>,
}

/// Everything a run is started with.
pub struct Start {
    pub run_id: String,
    pub runbook: Runbook,
    /// The resolved text parameters (a folder parameter's value is the path chosen).
    pub values: HashMap<String, String>,
    /// The bytes of each file parameter.
    pub files: HashMap<String, Vec<u8>>,
    pub hosts: Vec<HostJob>,
    pub scheduled: bool,
    pub rollback: bool,
    pub strategy: Strategy,
    pub from_step: Option<usize>,
    pub retry_of: Option<String>,
    pub history: Option<History>,
}

struct Active {
    abort: tokio::task::AbortHandle,
    record: Arc<Mutex<RunRecord>>,
    cancelled: Arc<AtomicBool>,
    sink: Arc<dyn RunbookSink>,
    history: Option<History>,
}

#[derive(Default)]
pub struct RunbookManager {
    runs: Mutex<HashMap<String, Active>>,
}

struct HostProgress {
    /// Values of secret parameters, taken out of anything shown or kept.
    redact: Arc<Vec<String>>,
    host_id: Uuid,
    sink: Arc<dyn RunbookSink>,
    record: Arc<Mutex<RunRecord>>,
    cancelled: Arc<AtomicBool>,
    /// Another host failed in a run that stops everything.
    halt: Arc<AtomicBool>,
    seq: AtomicU64,
}

fn hide(secrets: &[String], text: &str) -> String {
    secrets.iter().filter(|v| !v.is_empty()).fold(text.to_string(), |acc, v| acc.replace(v.as_str(), crate::mask::HIDDEN))
}

impl Progress for HostProgress {
    fn started(&self, index: usize) {
        self.sink.event(RunbookEvent::StepStarted { host_id: self.host_id, index });
    }
    fn finished(&self, result: &StepResult) {
        let result = &redacted(result, &self.redact);
        if let Some(h) = self.record.lock().unwrap_or_else(|p| p.into_inner()).hosts.iter_mut().find(|h| h.host_id == self.host_id) {
            h.steps.push(result.clone());
        }
        self.sink.event(RunbookEvent::Step { host_id: self.host_id, result: result.clone() });
    }
    fn output(&self, index: usize, stream: Stream, text: &str) {
        self.sink.event(RunbookEvent::Output { host_id: self.host_id, index, stream, text: hide(&self.redact, text), seq: self.seq.fetch_add(1, Ordering::Relaxed) });
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.halt.load(Ordering::Relaxed)
    }
}

/// The result with every secret value replaced, in the output and the note.
fn redacted(r: &StepResult, secrets: &[String]) -> StepResult {
    if secrets.is_empty() {
        return r.clone();
    }
    let mut r = r.clone();
    r.output.stdout = hide(secrets, &r.output.stdout);
    r.output.stderr = hide(secrets, &r.output.stderr);
    r.note = hide(secrets, &r.note);
    r
}

fn finish_host(record: &Mutex<RunRecord>, host_id: Uuid, ok: bool, error: Option<String>, status: HostStatus) {
    if let Some(h) = record.lock().unwrap_or_else(|p| p.into_inner()).hosts.iter_mut().find(|h| h.host_id == host_id) {
        h.ok = Some(ok);
        h.error = error;
        h.status = Some(status);
        h.finished_ms = Some(now_ms());
    }
}

fn mark(record: &Mutex<RunRecord>, host_id: Uuid, status: HostStatus) {
    if let Some(h) = record.lock().unwrap_or_else(|p| p.into_inner()).hosts.iter_mut().find(|h| h.host_id == host_id) {
        h.status = Some(status);
        if status == HostStatus::Connecting {
            h.started_ms = Some(now_ms());
        }
    }
}

/// What each host of one run shares.
struct HostCtx {
    runbook: Arc<Runbook>,
    values: Arc<HashMap<String, String>>,
    files: Arc<HashMap<String, Vec<u8>>>,
    opts: RunOptions,
    redact: Arc<Vec<String>>,
    sink: Arc<dyn RunbookSink>,
    record: Arc<Mutex<RunRecord>>,
    cancelled: Arc<AtomicBool>,
    halt: Arc<AtomicBool>,
}

impl RunbookManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a run in the background. The caller must be inside a tokio runtime.
    pub fn start(self: &Arc<Self>, spec: Start, sink: Arc<dyn RunbookSink>) {
        let Start { run_id, runbook, values, files, hosts, scheduled, rollback, strategy, from_step, retry_of, history } = spec;
        let mut shown = values.clone();
        let mut secret_values = Vec::new();
        for p in runbook.params.iter().filter(|p| p.kind == crate::runbook::ParamKind::Secret) {
            if let Some(v) = shown.get_mut(&p.name).filter(|v| !v.is_empty()) {
                secret_values.push(std::mem::replace(v, crate::mask::HIDDEN.to_string()));
            }
        }
        let secret_values = Arc::new(secret_values);
        for k in files.keys() {
            shown.insert(k.clone(), "(a file was chosen)".into());
        }
        let step_names = runbook.steps.iter().enumerate().map(|(i, s)| if s.name.is_empty() { format!("Step {}", i + 1) } else { s.name.clone() }).chain(runbook.rollback.iter().enumerate().map(|(i, s)| if s.name.is_empty() { format!("Rollback step {}", i + 1) } else { s.name.clone() })).collect();
        let record = Arc::new(Mutex::new(RunRecord {
            id: run_id.clone(),
            runbook: runbook.name.clone(),
            started_at: now(),
            finished_at: None,
            scheduled,
            rollback,
            cancelled: false,
            params: shown,
            hosts: hosts.iter().map(|h| HostRecord { host_id: h.host_id, label: h.label.clone(), ok: None, error: None, steps: Vec::new(), status: Some(HostStatus::Pending), started_ms: None, finished_ms: None }).collect(),
            strategy: Some(strategy),
            trigger: Some(if retry_of.is_some() { "retry" } else if scheduled { "schedule" } else { "manual" }.to_string()),
            retry_of,
            from_step,
            step_names,
        }));
        let cancelled = Arc::new(AtomicBool::new(false));
        let me = Arc::clone(self);
        let id = run_id.clone();
        let (rec, flag, sink_for_task, hist) = (Arc::clone(&record), Arc::clone(&cancelled), Arc::clone(&sink), history.clone());
        // spawn-ok: runbook_start calls this inside tauri::async_runtime::spawn
        let handle = tokio::spawn(async move {
            let ctx = Arc::new(HostCtx {
                runbook: Arc::new(runbook),
                values: Arc::new(values),
                files: Arc::new(files),
                opts: RunOptions { rollback, from_step },
                redact: secret_values,
                sink: Arc::clone(&sink_for_task),
                record: Arc::clone(&rec),
                cancelled: Arc::clone(&flag),
                halt: Arc::new(AtomicBool::new(false)),
            });
            run_all(&ctx, hosts, strategy).await;
            {
                let mut r = rec.lock().unwrap_or_else(|p| p.into_inner());
                r.finished_at = Some(now());
                if let Some(h) = &hist {
                    let _ = h.save(&r);
                }
            }
            sink_for_task.event(RunbookEvent::Done { cancelled: false });
            me.runs.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        });
        self.runs.lock().unwrap_or_else(|p| p.into_inner()).insert(run_id, Active { abort: handle.abort_handle(), record, cancelled, sink, history });
    }

    /// Stop a run. Steps that are running are dropped (their SSH channel is closed and hung up on, which ends most remote
    /// commands but is not a promise that one does), queued hosts never start, and what happened so far is kept in the record.
    pub fn cancel(&self, run_id: &str) -> bool {
        let Some(a) = self.runs.lock().unwrap_or_else(|p| p.into_inner()).remove(run_id) else { return false };
        a.cancelled.store(true, Ordering::Relaxed);
        a.abort.abort();
        let mut unfinished = Vec::new();
        {
            let mut r = a.record.lock().unwrap_or_else(|p| p.into_inner());
            r.cancelled = true;
            r.finished_at = Some(now());
            let ended = now_ms();
            for h in r.hosts.iter_mut().filter(|h| h.ok.is_none()) {
                let started = h.started_ms.is_some();
                h.ok = Some(false);
                h.error = Some(if started { "cancelled" } else { "cancelled before it started" }.into());
                h.status = Some(HostStatus::Cancelled);
                h.finished_ms = Some(ended);
                unfinished.push((h.host_id, h.started_ms.map_or(0, |s| ended.saturating_sub(s)), h.error.clone()));
            }
            if let Some(hist) = &a.history {
                let _ = hist.save(&r);
            }
        }
        for (host_id, duration_ms, error) in unfinished {
            a.sink.event(RunbookEvent::HostDone { host_id, ok: false, error, status: HostStatus::Cancelled, duration_ms });
        }
        a.sink.event(RunbookEvent::Done { cancelled: true });
        true
    }

    pub fn cancel_all(&self) {
        let ids: Vec<String> = self.runs.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect();
        for id in ids {
            self.cancel(&id);
        }
    }

    pub fn running(&self) -> usize {
        self.runs.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// Take the hosts the way the strategy says.
async fn run_all(ctx: &Arc<HostCtx>, hosts: Vec<HostJob>, strategy: Strategy) {
    match strategy {
        Strategy::Parallel => group(ctx, hosts, CONCURRENCY, false).await,
        Strategy::Concurrent { limit, stop_on_failure } => group(ctx, hosts, limit.clamp(1, crate::runbook::MAX_CONCURRENCY), stop_on_failure).await,
        Strategy::Sequential { stop_on_failure } => {
            for job in hosts {
                if ctx.halt.load(Ordering::Relaxed) {
                    skip(ctx, &job, "skipped: an earlier host failed");
                    continue;
                }
                if !run_job(ctx, job).await && stop_on_failure {
                    ctx.halt.store(true, Ordering::Relaxed);
                }
            }
        }
        Strategy::Rolling { batch_size, concurrency, stop_on_failure } => {
            let size = batch_size.clamp(1, crate::runbook::MAX_CONCURRENCY);
            let limit = concurrency.unwrap_or(size).clamp(1, size);
            let mut batches = hosts.into_iter().peekable();
            let mut failed_batch = false;
            while batches.peek().is_some() {
                let batch: Vec<HostJob> = batches.by_ref().take(size).collect();
                if failed_batch {
                    for job in &batch {
                        skip(ctx, job, "skipped: an earlier batch had a failed host");
                    }
                    continue;
                }
                let results = group_results(ctx, batch, limit, false).await;
                if stop_on_failure && results.iter().any(|ok| !ok) {
                    failed_batch = true;
                }
            }
        }
    }
}

/// Run hosts with at most `limit` in flight. With `fail_fast`, the first failure stops the others from starting, and the ones
/// running stop after their current step.
async fn group(ctx: &Arc<HostCtx>, hosts: Vec<HostJob>, limit: usize, fail_fast: bool) {
    group_results(ctx, hosts, limit, fail_fast).await;
}

async fn group_results(ctx: &Arc<HostCtx>, hosts: Vec<HostJob>, limit: usize, fail_fast: bool) -> Vec<bool> {
    let permits = Arc::new(Semaphore::new(limit));
    let mut set = JoinSet::new();
    let mut skipped = Vec::new();
    for job in hosts {
        // The permit is taken here, in the order the hosts were given, so hosts start in that order.
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else { break };
        if ctx.halt.load(Ordering::Relaxed) {
            skip(ctx, &job, "skipped: another host failed and the run stops at the first failure");
            skipped.push(false);
            continue;
        }
        let ctx = Arc::clone(ctx);
        set.spawn(async move {
            let _permit = permit;
            let ok = run_job(&ctx, job).await;
            if !ok && fail_fast {
                ctx.halt.store(true, Ordering::Relaxed);
            }
            ok
        });
    }
    let mut results = skipped;
    while let Some(r) = set.join_next().await {
        results.push(r.unwrap_or(false));
    }
    results
}

/// Report a host that is not going to be contacted.
fn skip(ctx: &HostCtx, job: &HostJob, why: &str) {
    finish_host(&ctx.record, job.host_id, false, Some(why.to_string()), HostStatus::Skipped);
    ctx.sink.event(RunbookEvent::HostDone { host_id: job.host_id, ok: false, error: Some(why.to_string()), status: HostStatus::Skipped, duration_ms: 0 });
}

/// Run the runbook on one host and record how it went. True when every step went through.
async fn run_job(ctx: &HostCtx, job: HostJob) -> bool {
    let host_id = job.host_id;
    let started = Instant::now();
    mark(&ctx.record, host_id, HostStatus::Connecting);
    ctx.sink.event(RunbookEvent::HostStarted { host_id });
    let done = |ok: bool, error: Option<String>, status: HostStatus| {
        finish_host(&ctx.record, host_id, ok, error.clone(), status);
        ctx.sink.event(RunbookEvent::HostDone { host_id, ok, error, status, duration_ms: started.elapsed().as_millis() as u64 });
        ok
    };
    let target = match job.target {
        Ok(t) => t,
        Err(why) => return done(false, Some(why), HostStatus::Failed),
    };
    let client = match open_client(&target, None).await {
        Ok((c, _)) => c,
        Err(e) => return done(false, Some(e.to_string()), HostStatus::Failed),
    };
    mark(&ctx.record, host_id, HostStatus::Running);
    ctx.sink.event(RunbookEvent::Connected { host_id });
    let exec = SshExec::new(client);
    let progress = HostProgress {
        redact: Arc::clone(&ctx.redact),
        host_id,
        sink: Arc::clone(&ctx.sink),
        record: Arc::clone(&ctx.record),
        cancelled: Arc::clone(&ctx.cancelled),
        halt: Arc::clone(&ctx.halt),
        seq: AtomicU64::new(0),
    };
    let host_values = HashMap::from([("host".to_string(), job.hostname.clone()), ("label".to_string(), job.label.clone())]);
    let (ok, results) = run_host_opts(&ctx.runbook, &ctx.values, &ctx.files, &host_values, &exec, &progress, ctx.opts).await;
    exec.client.close().await;
    if ok {
        return done(true, None, HostStatus::Success);
    }
    // Stopped by another host's failure, not by one of its own steps?
    let stopped = ctx.halt.load(Ordering::Relaxed) && !results.iter().any(|r| matches!(r.status, StepStatus::Failed | StepStatus::TimedOut | StepStatus::Error));
    if stopped {
        return done(false, Some("stopped: another host failed and the run stops at the first failure".into()), HostStatus::Cancelled);
    }
    let timed_out = results.iter().rev().find(|r| r.status != StepStatus::Skipped).is_some_and(|r| r.status == StepStatus::TimedOut);
    done(false, None, if timed_out { HostStatus::TimedOut } else { HostStatus::Failed })
}

// -- trying connections ---------------------------------------------------------------------------

/// What a connection test found for one host.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectCheck {
    pub host_id: Uuid,
    pub label: String,
    pub ok: bool,
    pub error: Option<String>,
    pub ms: u64,
}

/// Really connect to each host (signing in and then hanging up) with at most eight at a time. Nothing is run on them.
pub async fn connect_check(hosts: Vec<HostJob>) -> Vec<ConnectCheck> {
    let permits = Arc::new(Semaphore::new(CONCURRENCY));
    let mut set = JoinSet::new();
    for (i, job) in hosts.into_iter().enumerate() {
        let permits = Arc::clone(&permits);
        set.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else { return (i, None) };
            let started = Instant::now();
            let result = match job.target {
                Err(why) => Err(why),
                Ok(t) => match tokio::time::timeout(CONNECT_TEST, open_client(&t, None)).await {
                    Err(_) => Err(format!("no answer within {} s", CONNECT_TEST.as_secs())),
                    Ok(Err(e)) => Err(e.to_string()),
                    Ok(Ok((client, _))) => {
                        client.close().await;
                        Ok(())
                    }
                },
            };
            (i, Some(ConnectCheck { host_id: job.host_id, label: job.label, ok: result.is_ok(), error: result.err(), ms: started.elapsed().as_millis() as u64 }))
        });
    }
    let mut out: Vec<(usize, ConnectCheck)> = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok((i, Some(c))) = r {
            out.push((i, c));
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests;
