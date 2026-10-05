//! Containers: list, start, stop, inspect and follow logs for Docker, Podman
//! and nerdctl, on this computer or on a saved SSH host. Tauri-agnostic, like
//! the other engine modules.
//!
//! Everything is the runtime's own command line, with its JSON output parsed
//! (never its table), run through [`transport`]. Nothing is installed on a
//! host and no daemon socket is exposed.

pub mod parse;
pub mod transport;

#[cfg(test)]
mod live_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::ssh::{SshError, Target};
use parse::{Container, Image};
use transport::{SshShell, Transport, Utf8Chunker};

/// A listing or an inspect should be quick; this only stops a hung host.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(40);
/// Stopping waits for the container's grace period, 10 s by default.
pub const ACTION_TIMEOUT: Duration = Duration::from_secs(120);
/// Lines of log shown at once, at most.
pub const MAX_TAIL: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Docker,
    Podman,
    Nerdctl,
}

impl Runtime {
    pub const ALL: [Runtime; 3] = [Runtime::Docker, Runtime::Podman, Runtime::Nerdctl];

    pub fn binary(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Podman => "podman",
            Runtime::Nerdctl => "nerdctl",
        }
    }

    fn from_binary(s: &str) -> Option<Runtime> {
        Runtime::ALL.into_iter().find(|r| r.binary() == s)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("{0}")]
    Io(String),
    /// The runtime ran and refused; its own words, with a hint where one helps.
    #[error("{0}")]
    Command(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0} isn't installed here, or isn't on the PATH")]
    NotInstalled(String),
    #[error("timed out after {0} s")]
    Timeout(u64),
    #[error("no such session; it may have been closed")]
    NoSession,
    #[error("couldn't read the runtime's output: {0}")]
    Parse(String),
}

pub type ContainerResult<T> = Result<T, ContainerError>;

// -- what to run -------------------------------------------------------------------

/// A container or image reference is passed to the runtime as an argument.
/// Names and IDs never need more than this, so anything else is refused
/// rather than escaped.
pub fn check_ref(r: &str) -> ContainerResult<&str> {
    let ok = !r.is_empty()
        && r.len() <= 200
        && r.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && r.chars().all(|c| c.is_ascii_alphanumeric() || "_.-:/@".contains(c));
    if ok {
        Ok(r)
    } else {
        Err(ContainerError::Invalid(format!("\"{r}\" isn't a valid container or image name or ID")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Restart,
    /// `force` also removes a running container.
    Remove { force: bool },
}

pub fn list_containers_args(rt: Runtime, sizes: bool) -> Vec<String> {
    let mut a: Vec<String> = ["ps", "-a", "--no-trunc"].map(String::from).into();
    if sizes {
        a.push("-s".into());
    }
    a.extend(parse::json_format_args(rt).map(String::from));
    a
}

pub fn list_images_args(rt: Runtime) -> Vec<String> {
    let mut a: Vec<String> = vec!["images".into()];
    a.extend(parse::json_format_args(rt).map(String::from));
    a
}

pub fn action_args(action: Action, id: &str) -> ContainerResult<Vec<String>> {
    let id = check_ref(id)?.to_string();
    Ok(match action {
        Action::Start => vec!["start".into(), id],
        Action::Stop => vec!["stop".into(), id],
        Action::Restart => vec!["restart".into(), id],
        Action::Remove { force: false } => vec!["rm".into(), id],
        Action::Remove { force: true } => vec!["rm".into(), "-f".into(), id],
    })
}

pub fn logs_args(id: &str, tail: u32, follow: bool, timestamps: bool) -> ContainerResult<Vec<String>> {
    let mut a: Vec<String> = vec!["logs".into(), "--tail".into(), tail.clamp(1, MAX_TAIL).to_string()];
    if timestamps {
        a.push("--timestamps".into());
    }
    if follow {
        a.push("--follow".into());
    }
    a.push(check_ref(id)?.to_string());
    Ok(a)
}

pub fn inspect_args(id: &str) -> ContainerResult<Vec<String>> {
    Ok(vec!["inspect".into(), check_ref(id)?.to_string()])
}

/// What the runtime printed when it failed, with a hint for the usual causes.
fn failure(rt: Runtime, out: &transport::Output) -> ContainerError {
    let text = if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() };
    let lower = text.to_ascii_lowercase();
    if out.code == Some(127) || lower.contains("command not found") || lower.contains("not found") && lower.contains(rt.binary()) {
        return ContainerError::NotInstalled(rt.binary().to_string());
    }
    let hint = if lower.contains("permission denied") && (lower.contains("docker.sock") || lower.contains("daemon socket")) {
        "\nHint: this user isn't allowed to use Docker. Add it to the \"docker\" group (then sign in again), or use rootless Docker or Podman."
    } else if lower.contains("cannot connect to the docker daemon") || lower.contains("is the docker daemon running") {
        "\nHint: the Docker daemon doesn't seem to be running, or this user can't reach it."
    } else if lower.contains("cannot connect to podman") || lower.contains("podman.sock") && lower.contains("no such file") {
        "\nHint: the Podman socket isn't available for this user."
    } else {
        ""
    };
    let message = if text.is_empty() { format!("{} exited with status {}", rt.binary(), out.code.map_or("?".into(), |c| c.to_string())) } else { text.to_string() };
    ContainerError::Command(format!("{message}{hint}"))
}

// -- sessions ----------------------------------------------------------------------

pub struct Session {
    transport: Transport,
    pub runtimes: Vec<Runtime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub containers: Vec<Container>,
    pub images: Vec<Image>,
    /// Containers listed but images didn't (rare); say why instead of hiding it.
    pub images_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent {
    Chunk { text: String },
    /// The command ended (or was stopped: no code).
    End { code: Option<i32>, error: Option<String> },
}

/// How much of a log to read.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LogOptions {
    /// Lines from the end.
    pub tail: u32,
    /// Keep reading as new lines arrive.
    pub follow: bool,
    pub timestamps: bool,
}

pub trait LogSink: Send + Sync + 'static {
    fn event(&self, e: LogEvent);
}

#[derive(Default)]
pub struct ContainerManager {
    sessions: Mutex<HashMap<Uuid, Arc<Session>>>,
    /// Running log streams: the session they belong to, and how to stop them.
    streams: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<()>)>>,
}

impl ContainerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open this computer (`via` is `None`) or a host. Reports which runtimes
    /// are installed there.
    pub async fn open(&self, via: Option<Target>) -> ContainerResult<(Uuid, Vec<Runtime>)> {
        let transport = match via {
            Some(t) => Transport::Ssh(Box::new(SshShell::connect(t).await?)),
            None => Transport::Local,
        };
        let names: Vec<&str> = Runtime::ALL.iter().map(|r| r.binary()).collect();
        let runtimes: Vec<Runtime> = transport.which(&names).await?.iter().filter_map(|s| Runtime::from_binary(s)).collect();
        let id = Uuid::new_v4();
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Session { transport, runtimes: runtimes.clone() }));
        Ok((id, runtimes))
    }

    fn get(&self, id: Uuid) -> ContainerResult<Arc<Session>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned().ok_or(ContainerError::NoSession)
    }

    pub async fn close(&self, id: Uuid) {
        let session = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        let mine: Vec<Uuid> = self
            .streams
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(_, (sid, _))| *sid == id)
            .map(|(k, _)| *k)
            .collect();
        for s in mine {
            self.stop_stream(s);
        }
        if let Some(s) = session {
            s.transport.close().await;
        }
    }

    pub async fn close_all(&self) {
        let ids: Vec<Uuid> = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).keys().copied().collect();
        for id in ids {
            self.close(id).await;
        }
    }

    pub async fn list(&self, session: Uuid, rt: Runtime, sizes: bool) -> ContainerResult<Listing> {
        let s = self.get(session)?;
        let (c_args, i_args) = (list_containers_args(rt, sizes), list_images_args(rt));
        let (cs, is) = tokio::join!(
            s.transport.exec(rt.binary(), &c_args, LIST_TIMEOUT),
            s.transport.exec(rt.binary(), &i_args, LIST_TIMEOUT)
        );
        let cs = cs?;
        if cs.code != Some(0) {
            return Err(failure(rt, &cs));
        }
        let mut containers = parse::parse_containers(&cs.stdout).map_err(ContainerError::Parse)?;
        containers.sort_by_key(|c| c.name.to_lowercase());
        let (images, images_error) = match is {
            Ok(o) if o.code == Some(0) => match parse::parse_images(&o.stdout) {
                Ok(mut list) => {
                    list.sort_by_key(|i| (i.repository.to_lowercase(), i.tag.clone()));
                    (list, None)
                }
                Err(e) => (Vec::new(), Some(e)),
            },
            Ok(o) => (Vec::new(), Some(failure(rt, &o).to_string())),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        Ok(Listing { containers, images, images_error })
    }

    pub async fn act(&self, session: Uuid, rt: Runtime, action: Action, id: &str) -> ContainerResult<()> {
        let args = action_args(action, id)?;
        let out = self.get(session)?.transport.exec(rt.binary(), &args, ACTION_TIMEOUT).await?;
        if out.code == Some(0) {
            Ok(())
        } else {
            Err(failure(rt, &out))
        }
    }

    /// The runtime's `inspect` output, as the JSON text it printed.
    pub async fn inspect(&self, session: Uuid, rt: Runtime, id: &str) -> ContainerResult<String> {
        let args = inspect_args(id)?;
        let out = self.get(session)?.transport.exec(rt.binary(), &args, LIST_TIMEOUT).await?;
        if out.code != Some(0) {
            return Err(failure(rt, &out));
        }
        // Check it is JSON, so the window never has to guess.
        serde_json::from_str::<serde_json::Value>(&out.stdout).map_err(|e| ContainerError::Parse(e.to_string()))?;
        Ok(out.stdout)
    }

    /// Start reading a container's log in the background. Returns an id for
    /// [`Self::stop_stream`]. The sink gets the text as it arrives and a final
    /// `End`.
    pub fn start_logs(self: &Arc<Self>, session: Uuid, rt: Runtime, id: &str, opts: LogOptions, sink: Arc<dyn LogSink>) -> ContainerResult<Uuid> {
        let args = logs_args(id, opts.tail, opts.follow, opts.timestamps)?;
        let s = self.get(session)?;
        let stream_id = Uuid::new_v4();
        let (stop_tx, stop_rx) = oneshot::channel();
        self.streams.lock().unwrap_or_else(|p| p.into_inner()).insert(stream_id, (session, stop_tx));
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let chunker = Mutex::new(Utf8Chunker::default());
            let result = s
                .transport
                .stream(
                    rt.binary(),
                    &args,
                    |bytes| {
                        let text = chunker.lock().unwrap_or_else(|p| p.into_inner()).push(bytes);
                        if !text.is_empty() {
                            sink.event(LogEvent::Chunk { text });
                        }
                    },
                    stop_rx,
                )
                .await;
            let rest = chunker.lock().unwrap_or_else(|p| p.into_inner()).finish();
            if !rest.is_empty() {
                sink.event(LogEvent::Chunk { text: rest });
            }
            me.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(&stream_id);
            sink.event(match result {
                Ok(code) => LogEvent::End { code, error: None },
                Err(e) => LogEvent::End { code: None, error: Some(e.to_string()) },
            });
        });
        Ok(stream_id)
    }

    pub fn stop_stream(&self, stream_id: Uuid) {
        if let Some((_, stop)) = self.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(&stream_id) {
            let _ = stop.send(());
        }
    }

    pub fn open_count(&self) -> usize {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stderr: &str) -> transport::Output {
        transport::Output { stdout: String::new(), stderr: stderr.into(), code: Some(code) }
    }

    #[test]
    fn references_are_checked_not_escaped() {
        for ok in ["web", "c0-web", "my_app.1", "fd4c241537ef", &"a".repeat(64), "nginx:1.27", "localhost:5000/team/app:v2", "sha256:ab"] {
            assert!(check_ref(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-rf", "--all", "a b", "a;b", "$(x)", "`x`", "a\nb", "a'b", "a\"b", "../x", " x", &"a".repeat(201)] {
            assert!(check_ref(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn argument_lists() {
        assert_eq!(list_containers_args(Runtime::Docker, false), ["ps", "-a", "--no-trunc", "--format", "{{json .}}"]);
        assert_eq!(list_containers_args(Runtime::Docker, true), ["ps", "-a", "--no-trunc", "-s", "--format", "{{json .}}"]);
        assert_eq!(list_containers_args(Runtime::Podman, false), ["ps", "-a", "--no-trunc", "--format", "json"]);
        assert_eq!(list_containers_args(Runtime::Nerdctl, false)[4], "{{json .}}");
        assert_eq!(list_images_args(Runtime::Podman), ["images", "--format", "json"]);
        assert_eq!(action_args(Action::Stop, "abc").unwrap(), ["stop", "abc"]);
        assert_eq!(action_args(Action::Remove { force: false }, "abc").unwrap(), ["rm", "abc"]);
        assert_eq!(action_args(Action::Remove { force: true }, "abc").unwrap(), ["rm", "-f", "abc"]);
        assert_eq!(logs_args("abc", 500, true, true).unwrap(), ["logs", "--tail", "500", "--timestamps", "--follow", "abc"]);
        assert_eq!(logs_args("abc", 0, false, false).unwrap(), ["logs", "--tail", "1", "abc"]);
        assert_eq!(logs_args("abc", u32::MAX, false, false).unwrap()[2], MAX_TAIL.to_string());
        assert_eq!(inspect_args("abc").unwrap(), ["inspect", "abc"]);
    }

    #[test]
    fn a_reference_that_looks_like_a_flag_never_reaches_the_runtime() {
        assert!(action_args(Action::Remove { force: true }, "--all").is_err());
        assert!(logs_args("-f", 10, false, false).is_err());
        assert!(inspect_args("--format=x").is_err());
    }

    #[test]
    fn failures_explain_themselves() {
        let e = failure(Runtime::Docker, &out(1, "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock")).to_string();
        assert!(e.contains("permission denied") && e.contains("docker\" group"), "{e}");
        let e = failure(Runtime::Docker, &out(1, "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?")).to_string();
        assert!(e.contains("daemon doesn't seem to be running"), "{e}");
        assert!(matches!(failure(Runtime::Docker, &out(127, "sh: 1: exec: docker: not found")), ContainerError::NotInstalled(ref b) if b == "docker"));
        let e = failure(Runtime::Docker, &out(1, "Error response from daemon: No such container: nope")).to_string();
        assert_eq!(e, "Error response from daemon: No such container: nope", "no hint when none applies");
        let e = failure(Runtime::Podman, &out(125, "")).to_string();
        assert_eq!(e, "podman exited with status 125");
    }
}
