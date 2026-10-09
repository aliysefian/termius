//! Running a runbook on hosts: one SSH connection per host for all its steps, hosts in parallel, a live feed of
//! what happens, and a record of every run kept on this computer (outputs can hold secrets, so it is never synced).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::containers::transport::shell_quote;
use crate::runbook::{run_host_with, Executor, Output, Progress, Runbook, StepResult};
use crate::runbookhistory::{now, History, HostRecord, RunRecord};
use crate::runner::{push_capped, Order, CONCURRENCY};
use crate::ssh::{open_client, Client, Target};

/// A file chosen for an upload: at most this large, and this much in all.
pub const MAX_FILE: usize = 8 * 1024 * 1024;
pub const MAX_FILES_TOTAL: usize = 32 * 1024 * 1024;

// -- over SSH ----------------------------------------------------------------------------

pub struct SshExec {
    client: Client,
}

impl SshExec {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn run(&self, command: &str, stdin: Option<&[u8]>) -> Result<Output, String> {
        let started = Instant::now();
        let mut channel = self.client.channel_open_session().await.map_err(|e| e.to_string())?;
        channel.exec(true, command.as_bytes()).await.map_err(|e| e.to_string())?;
        if let Some(data) = stdin {
            channel.data(data).await.map_err(|e| e.to_string())?;
            channel.eof().await.map_err(|e| e.to_string())?;
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut truncated = false;
        let mut exit_code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => push_capped(&mut out, &data, &mut truncated),
                ChannelMsg::ExtendedData { data, ext: 1 } => push_capped(&mut err, &data, &mut truncated),
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok(Output {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            exit_code,
            truncated,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

impl Executor for SshExec {
    async fn exec(&self, command: &str, timeout: Duration) -> Result<Output, String> {
        tokio::time::timeout(timeout, self.run(command, None)).await.map_err(|_| format!("took longer than {} s", timeout.as_secs()))?
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
        tokio::time::timeout(Duration::from_secs(300), self.run(&command, Some(data))).await.map_err(|_| "the upload took longer than 5 minutes".to_string())?
    }
}

// -- the run -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RunbookEvent {
    HostStarted { host_id: Uuid },
    StepStarted { host_id: Uuid, index: usize },
    Step { host_id: Uuid, result: StepResult },
    HostDone { host_id: Uuid, ok: bool, error: Option<String> },
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
    host_id: Uuid,
    sink: Arc<dyn RunbookSink>,
    record: Arc<Mutex<RunRecord>>,
    cancelled: Arc<AtomicBool>,
}

impl Progress for HostProgress {
    fn started(&self, index: usize) {
        self.sink.event(RunbookEvent::StepStarted { host_id: self.host_id, index });
    }
    fn finished(&self, result: &StepResult) {
        if let Some(h) = self.record.lock().unwrap_or_else(|p| p.into_inner()).hosts.iter_mut().find(|h| h.host_id == self.host_id) {
            h.steps.push(result.clone());
        }
        self.sink.event(RunbookEvent::Step { host_id: self.host_id, result: result.clone() });
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

fn finish_host(record: &Mutex<RunRecord>, host_id: Uuid, ok: bool, error: Option<String>) {
    if let Some(h) = record.lock().unwrap_or_else(|p| p.into_inner()).hosts.iter_mut().find(|h| h.host_id == host_id) {
        h.ok = Some(ok);
        h.error = error;
    }
}

impl RunbookManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a run. `values` are the resolved text parameters, `files` the bytes of each file parameter.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        self: &Arc<Self>,
        run_id: String,
        runbook: Runbook,
        values: HashMap<String, String>,
        files: HashMap<String, Vec<u8>>,
        hosts: Vec<HostJob>,
        scheduled: bool,
        rollback: bool,
        order: Order,
        history: Option<History>,
        sink: Arc<dyn RunbookSink>,
    ) {
        let mut shown = values.clone();
        for k in files.keys() {
            shown.insert(k.clone(), "(a file was chosen)".into());
        }
        let record = Arc::new(Mutex::new(RunRecord {
            id: run_id.clone(),
            runbook: runbook.name.clone(),
            started_at: now(),
            finished_at: None,
            scheduled,
            rollback,
            cancelled: false,
            params: shown,
            hosts: hosts.iter().map(|h| HostRecord { host_id: h.host_id, label: h.label.clone(), ok: None, error: None, steps: Vec::new() }).collect(),
        }));
        let cancelled = Arc::new(AtomicBool::new(false));
        let me = Arc::clone(self);
        let id = run_id.clone();
        let (rec, flag, sink_for_task, hist) = (Arc::clone(&record), Arc::clone(&cancelled), Arc::clone(&sink), history.clone());
        // spawn-ok: runbook_start calls this inside tauri::async_runtime::spawn
        let handle = tokio::spawn(async move {
            let (runbook, values, files) = (Arc::new(runbook), Arc::new(values), Arc::new(files));
            let ctx = Arc::new(HostCtx { runbook, values, files, rollback, sink: Arc::clone(&sink_for_task), record: Arc::clone(&rec), cancelled: Arc::clone(&flag) });
            match order {
                Order::Parallel => {
                    let limit = Arc::new(Semaphore::new(CONCURRENCY));
                    let mut set = JoinSet::new();
                    for job in hosts {
                        let (limit, ctx) = (Arc::clone(&limit), Arc::clone(&ctx));
                        set.spawn(async move {
                            let Ok(_permit) = limit.acquire_owned().await else { return };
                            run_job(&ctx, job).await;
                        });
                    }
                    while set.join_next().await.is_some() {}
                }
                Order::Sequential { stop_on_failure } => {
                    let mut stopped = false;
                    for job in hosts {
                        if stopped {
                            let why = "skipped: an earlier host failed".to_string();
                            finish_host(&ctx.record, job.host_id, false, Some(why.clone()));
                            ctx.sink.event(RunbookEvent::HostDone { host_id: job.host_id, ok: false, error: Some(why) });
                            continue;
                        }
                        if !run_job(&ctx, job).await && stop_on_failure {
                            stopped = true;
                        }
                    }
                }
            }
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

    /// Stop a run: steps that are running are dropped, and what happened so far is kept in the record.
    pub fn cancel(&self, run_id: &str) -> bool {
        let Some(a) = self.runs.lock().unwrap_or_else(|p| p.into_inner()).remove(run_id) else { return false };
        a.cancelled.store(true, Ordering::Relaxed);
        a.abort.abort();
        {
            let mut r = a.record.lock().unwrap_or_else(|p| p.into_inner());
            r.cancelled = true;
            r.finished_at = Some(now());
            for h in r.hosts.iter_mut().filter(|h| h.ok.is_none()) {
                h.ok = Some(false);
                h.error = Some("cancelled".into());
            }
            if let Some(hist) = &a.history {
                let _ = hist.save(&r);
            }
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
}


/// What every host of one run shares.
struct HostCtx {
    runbook: Arc<Runbook>,
    values: Arc<HashMap<String, String>>,
    files: Arc<HashMap<String, Vec<u8>>>,
    rollback: bool,
    sink: Arc<dyn RunbookSink>,
    record: Arc<Mutex<RunRecord>>,
    cancelled: Arc<AtomicBool>,
}

/// Run the runbook on one host and record how it went. True when every step went through.
async fn run_job(ctx: &HostCtx, job: HostJob) -> bool {
    let host_id = job.host_id;
    ctx.sink.event(RunbookEvent::HostStarted { host_id });
    let fail = |why: String| {
        finish_host(&ctx.record, host_id, false, Some(why.clone()));
        ctx.sink.event(RunbookEvent::HostDone { host_id, ok: false, error: Some(why) });
        false
    };
    let target = match job.target {
        Ok(t) => t,
        Err(why) => return fail(why),
    };
    let client = match open_client(&target, None).await {
        Ok((c, _)) => c,
        Err(e) => return fail(e.to_string()),
    };
    let exec = SshExec::new(client);
    let progress = HostProgress { host_id, sink: Arc::clone(&ctx.sink), record: Arc::clone(&ctx.record), cancelled: Arc::clone(&ctx.cancelled) };
    let host_values = HashMap::from([("host".to_string(), job.hostname.clone()), ("label".to_string(), job.label.clone())]);
    let (ok, _) = run_host_with(&ctx.runbook, &ctx.values, &ctx.files, &host_values, &exec, &progress, ctx.rollback).await;
    exec.client.close().await;
    finish_host(&ctx.record, host_id, ok, None);
    ctx.sink.event(RunbookEvent::HostDone { host_id, ok, error: None });
    ok
}

#[cfg(test)]
mod tests;
