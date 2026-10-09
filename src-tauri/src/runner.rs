//! Run one command on many hosts in the background and collect each host's
//! output, exit code and timing. Uses non-interactive `exec` channels (no
//! PTY), like `ssh host 'command'`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::ssh::{open_client, SshError, Target};

/// Per-stream output cap, so a runaway command can't exhaust memory.
pub const MAX_OUTPUT: usize = 256 * 1024;
/// Hosts contacted at the same time.
pub const CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    /// `None` when the server didn't report one (e.g. killed by a signal).
    pub exit_code: Option<u32>,
    pub truncated: bool,
    pub duration_ms: u64,
}

/// How a run takes its hosts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// All at once, up to [`CONCURRENCY`] at a time.
    #[default]
    Parallel,
    /// One host after another, in the order given. With `stop_on_failure` the first host that fails (a non-zero exit,
    /// no answer, a failed login) ends the run: the rest are reported as skipped and are never contacted. That is the
    /// way to roll a change through a fleet without breaking all of it at once.
    Sequential { stop_on_failure: bool },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RunEvent {
    /// Not contacted, because an earlier host failed in a run that stops at the first failure.
    Skipped {
        host_id: Uuid,
    },
    Started {
        host_id: Uuid,
    },
    Finished {
        host_id: Uuid,
        output: ExecOutput,
    },
    Failed {
        host_id: Uuid,
        message: String,
    },
    /// Every host has finished, failed, or the run was cancelled.
    Done,
}

pub trait RunSink: Send + Sync + 'static {
    fn event(&self, e: RunEvent);
}

pub(crate) fn push_capped(buf: &mut Vec<u8>, data: &[u8], truncated: &mut bool) {
    let room = MAX_OUTPUT.saturating_sub(buf.len());
    if data.len() > room {
        *truncated = true;
    }
    buf.extend_from_slice(&data[..data.len().min(room)]);
}

/// Run `command` on `target` and wait for it to finish, up to `timeout`.
pub async fn exec(
    target: &Target,
    command: &str,
    timeout: Duration,
) -> Result<ExecOutput, SshError> {
    let started = Instant::now();
    let label = format!("{}:{}", target.hostname, target.port);
    let work = async {
        let (client, _) = open_client(target, None).await?;
        let mut channel = client.channel_open_session().await?;
        channel.exec(true, command.as_bytes()).await?;

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut truncated = false;
        let mut exit_code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => push_capped(&mut out, &data, &mut truncated),
                ChannelMsg::ExtendedData { data, ext: 1 } => {
                    push_capped(&mut err, &data, &mut truncated)
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        client.close().await;
        Ok::<_, SshError>(ExecOutput {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            exit_code,
            truncated,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    };
    tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| SshError::Timeout(label))?
}

/// Run one job and report it. True if the host counts as failed (no result, or a command that did not exit with 0).
async fn run_job(sink: &dyn RunSink, host_id: Uuid, target: &Target, command: &str, timeout: Duration) -> bool {
    sink.event(RunEvent::Started { host_id });
    match exec(target, command, timeout).await {
        Ok(output) => {
            let failed = output.exit_code != Some(0);
            sink.event(RunEvent::Finished { host_id, output });
            failed
        }
        Err(e) => {
            sink.event(RunEvent::Failed { host_id, message: e.to_string() });
            true
        }
    }
}

/// Tracks background runs so they can be cancelled.
#[derive(Default)]
pub struct RunManager {
    runs: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}

impl RunManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a run. Each job is `(host_id, target, command)`, so commands can
    /// differ per host (snippet variables are filled in per host).
    pub fn start(
        self: &Arc<Self>,
        run_id: String,
        jobs: Vec<(Uuid, Target, String)>,
        timeout: Duration,
        sink: Arc<dyn RunSink>,
    ) {
        self.start_ordered(run_id, jobs, timeout, Order::Parallel, sink);
    }

    /// Like [`Self::start`], with a choice of [`Order`].
    pub fn start_ordered(
        self: &Arc<Self>,
        run_id: String,
        jobs: Vec<(Uuid, Target, String)>,
        timeout: Duration,
        order: Order,
        sink: Arc<dyn RunSink>,
    ) {
        let me = Arc::clone(self);
        let id = run_id.clone();
        // spawn-ok: run_on_hosts and the CLI handler call this inside the async runtime
        let handle = tokio::spawn(async move {
            match order {
                Order::Parallel => {
                    let limit = Arc::new(Semaphore::new(CONCURRENCY));
                    let mut set = JoinSet::new();
                    for (host_id, target, command) in jobs {
                        let limit = Arc::clone(&limit);
                        let sink = Arc::clone(&sink);
                        set.spawn(async move {
                            let Ok(_permit) = limit.acquire_owned().await else {
                                return;
                            };
                            run_job(sink.as_ref(), host_id, &target, &command, timeout).await;
                        });
                    }
                    while set.join_next().await.is_some() {}
                }
                Order::Sequential { stop_on_failure } => {
                    let mut stopped = false;
                    for (host_id, target, command) in jobs {
                        if stopped {
                            sink.event(RunEvent::Skipped { host_id });
                            continue;
                        }
                        let failed = run_job(sink.as_ref(), host_id, &target, &command, timeout).await;
                        stopped = failed && stop_on_failure;
                    }
                }
            }
            sink.event(RunEvent::Done);
            me.runs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
        });
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(run_id, handle.abort_handle());
    }

    /// Abort a run. Hosts still running are dropped; their connections close.
    pub fn cancel(&self, run_id: &str) -> bool {
        match self
            .runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(run_id)
        {
            Some(h) => {
                h.abort();
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        for (_, h) in self.runs.lock().unwrap_or_else(|p| p.into_inner()).drain() {
            h.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::testutil::{spawn_sshd, target};

    struct Collect(std::sync::mpsc::Sender<RunEvent>);
    impl RunSink for Collect {
        fn event(&self, e: RunEvent) {
            let _ = self.0.send(e);
        }
    }

    #[test]
    fn output_is_capped() {
        let mut buf = Vec::new();
        let mut t = false;
        push_capped(&mut buf, &vec![b'a'; MAX_OUTPUT - 1], &mut t);
        assert!(!t);
        push_capped(&mut buf, b"xyz", &mut t);
        assert!(t);
        assert_eq!(buf.len(), MAX_OUTPUT);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exec_collects_streams_exit_codes_and_timeouts() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));

        let out = exec(
            &t,
            "echo out; echo err >&2; exit 3",
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        assert_eq!(out.stdout, "out\n");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.truncated);

        let err = exec(&t, "sleep 5", Duration::from_millis(500))
            .await
            .unwrap_err();
        assert!(matches!(err, SshError::Timeout(_)), "{err}");

        // Several hosts (the same server, different commands) in one run.
        let mgr = Arc::new(RunManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let mut bad = t.clone();
        bad.auth = crate::models::AuthMethod::PrivateKey {
            private_key: sshd.other_key.clone(),
            passphrase: None,
            certificate: None,
        };
        mgr.start(
            "r1".into(),
            vec![
                (ids[0], t.clone(), "echo one".into()),
                (ids[1], t.clone(), "echo two".into()),
                (ids[2], bad, "echo never".into()),
            ],
            Duration::from_secs(15),
            Arc::new(Collect(tx)),
        );
        let mut finished = HashMap::new();
        let mut failed = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).expect("run event") {
                RunEvent::Finished { host_id, output } => {
                    finished.insert(host_id, output.stdout);
                }
                RunEvent::Failed { host_id, message } => failed.push((host_id, message)),
                RunEvent::Done => break,
                RunEvent::Started { .. } | RunEvent::Skipped { .. } => {}
            }
        }
        assert_eq!(finished[&ids[0]], "one\n");
        assert_eq!(finished[&ids[1]], "two\n");
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, ids[2]);
        assert!(
            failed[0].1.contains("authentication failed"),
            "{}",
            failed[0].1
        );

        // Cancelling stops a long run promptly and reports no Done.
        let (tx, rx) = std::sync::mpsc::channel();
        mgr.start(
            "r2".into(),
            vec![(Uuid::new_v4(), t.clone(), "sleep 30".into())],
            Duration::from_secs(60),
            Arc::new(Collect(tx)),
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(mgr.cancel("r2"));
        assert!(!mgr.cancel("r2"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(rx.try_iter().all(|e| !matches!(e, RunEvent::Done)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_rolling_run_goes_one_host_at_a_time_and_can_stop_at_the_first_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let mgr = Arc::new(RunManager::new());
        let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let jobs = |cmds: [&str; 4]| ids.iter().zip(cmds).map(|(id, c)| (*id, t.clone(), c.to_string())).collect::<Vec<_>>();
        // Each event as "<index> <what>" until Done.
        let collect = |rx: &std::sync::mpsc::Receiver<RunEvent>| {
            let mut seen = Vec::new();
            loop {
                let (id, what) = match rx.recv_timeout(Duration::from_secs(30)).expect("run event") {
                    RunEvent::Started { host_id } => (host_id, "started"),
                    RunEvent::Finished { host_id, output } => (host_id, if output.exit_code == Some(0) { "ok" } else { "failed" }),
                    RunEvent::Failed { host_id, .. } => (host_id, "error"),
                    RunEvent::Skipped { host_id } => (host_id, "skipped"),
                    RunEvent::Done => break,
                };
                seen.push(format!("{} {what}", ids.iter().position(|i| *i == id).unwrap()));
            }
            seen
        };

        // The third host fails: the fourth is skipped, never started, and nothing overlaps.
        let (tx, rx) = std::sync::mpsc::channel();
        mgr.start_ordered("r1".into(), jobs(["true", "true", "exit 7", "echo never > /tmp/never-rolled"]), Duration::from_secs(15), Order::Sequential { stop_on_failure: true }, Arc::new(Collect(tx)));
        assert_eq!(collect(&rx), ["0 started", "0 ok", "1 started", "1 ok", "2 started", "2 failed", "3 skipped"]);

        // Without stop_on_failure every host still runs, in order.
        let (tx, rx) = std::sync::mpsc::channel();
        mgr.start_ordered("r2".into(), jobs(["true", "exit 1", "true", "true"]), Duration::from_secs(15), Order::Sequential { stop_on_failure: false }, Arc::new(Collect(tx)));
        assert_eq!(collect(&rx), ["0 started", "0 ok", "1 started", "1 failed", "2 started", "2 ok", "3 started", "3 ok"]);

        // The default is the old behaviour: all hosts, none skipped.
        let (tx, rx) = std::sync::mpsc::channel();
        mgr.start("r3".into(), jobs(["true", "exit 1", "true", "true"]), Duration::from_secs(15), Arc::new(Collect(tx)));
        let seen = collect(&rx);
        assert_eq!(seen.iter().filter(|e| e.ends_with("started")).count(), 4);
        assert!(!seen.iter().any(|e| e.ends_with("skipped")));
        assert_eq!(Order::default(), Order::Parallel);
        assert_eq!(serde_json::from_str::<Order>(r#"{"sequential":{"stop_on_failure":true}}"#).unwrap(), Order::Sequential { stop_on_failure: true });
        assert_eq!(serde_json::from_str::<Order>(r#""parallel""#).unwrap(), Order::Parallel);
    }
}
