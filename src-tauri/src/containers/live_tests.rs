//! Tests against a real Docker, locally and through a real `sshd` on this
//! computer. Skipped unless `SSHVAULT_TEST_DOCKER` is set (to anything).
//! They create and remove their own containers, named `sshvault-ct-*`, from
//! the Python image (override with `SSHVAULT_TEST_DOCKER_IMAGE`), which must
//! already be present; nothing is pulled.
//!
//! ```text
//! SSHVAULT_TEST_DOCKER=1 cargo test containers::live -- --ignored --test-threads=1
//! ```

use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::parse::State;
use super::*;

fn enabled() -> bool {
    std::env::var_os("SSHVAULT_TEST_DOCKER").is_some()
}

fn image() -> String {
    std::env::var("SSHVAULT_TEST_DOCKER_IMAGE").unwrap_or_else(|_| "python:3.12-slim".into())
}

/// A container that prints numbered lines forever, removed when dropped.
struct Chatty {
    name: String,
    id: String,
}

impl Chatty {
    fn start(tag: &str) -> Self {
        let name = format!("sshvault-ct-{}-{}", std::process::id(), tag);
        let out = Command::new("docker")
            .args(["run", "-d", "--stop-timeout", "1", "--name", &name, &image(), "python", "-u", "-c"])
            .arg("import time\nfor i in range(100000):\n    print(f'line {i} é✓ {chr(10006)}', flush=True)\n    time.sleep(0.1)")
            .output()
            .expect("docker run");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Self { name, id: String::from_utf8_lossy(&out.stdout).trim().to_string() }
    }
}

impl Drop for Chatty {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.name]).output();
    }
}

#[derive(Default)]
struct Collect(Mutex<(String, Option<LogEvent>)>);

impl LogSink for Collect {
    fn event(&self, e: LogEvent) {
        let mut g = self.0.lock().unwrap();
        match e {
            LogEvent::Chunk { text } => g.0.push_str(&text),
            end @ LogEvent::End { .. } => g.1 = Some(end),
        }
    }
}

impl Collect {
    fn text(&self) -> String {
        self.0.lock().unwrap().0.clone()
    }
    fn ended(&self) -> Option<LogEvent> {
        self.0.lock().unwrap().1.clone()
    }
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn state_of(m: &ContainerManager, s: Uuid, name: &str) -> Option<State> {
    m.list(s, Runtime::Docker, false).await.unwrap().containers.iter().find(|c| c.name == name).map(|c| c.state)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_local_list_and_actions() {
    if !enabled() {
        return;
    }
    let m = ContainerManager::new();
    let (s, runtimes) = m.open(None).await.unwrap();
    assert!(runtimes.contains(&Runtime::Docker), "{runtimes:?}");
    let c = Chatty::start("act");

    let listing = m.list(s, Runtime::Docker, false).await.unwrap();
    let me = listing.containers.iter().find(|x| x.name == c.name).expect("listed");
    assert_eq!(me.id, c.id, "the full ID");
    assert_eq!(me.state, State::Running);
    assert!(me.image.contains("python"));
    assert!(me.created.is_some_and(|t| t > 1_700_000_000));
    assert!(listing.images.iter().any(|i| format!("{}:{}", i.repository, i.tag).contains("python")), "images are listed");
    assert!(listing.images_error.is_none());

    // Sizes, when asked for (newer Docker prints one regardless; older needs -s).
    let sized = m.list(s, Runtime::Docker, true).await.unwrap();
    assert!(sized.containers.iter().find(|x| x.name == c.name).unwrap().size.is_some());

    m.act(s, Runtime::Docker, Action::Stop, &c.id).await.unwrap();
    assert_eq!(state_of(&m, s, &c.name).await, Some(State::Exited));
    m.act(s, Runtime::Docker, Action::Start, &c.id).await.unwrap();
    assert_eq!(state_of(&m, s, &c.name).await, Some(State::Running));
    m.act(s, Runtime::Docker, Action::Restart, &c.id).await.unwrap();
    assert_eq!(state_of(&m, s, &c.name).await, Some(State::Running));

    // Removing a running container needs force, and says why when it isn't.
    let err = m.act(s, Runtime::Docker, Action::Remove { force: false }, &c.id).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("running") || err.to_lowercase().contains("force"), "{err}");
    assert!(state_of(&m, s, &c.name).await.is_some(), "a refused remove removes nothing");
    m.act(s, Runtime::Docker, Action::Remove { force: true }, &c.id).await.unwrap();
    assert_eq!(state_of(&m, s, &c.name).await, None);

    // Errors are the runtime's words; bad references never reach it.
    let err = m.act(s, Runtime::Docker, Action::Stop, "no-such-container-xyz").await.unwrap_err().to_string();
    assert!(err.contains("No such container"), "{err}");
    assert!(matches!(m.act(s, Runtime::Docker, Action::Remove { force: true }, "--all").await, Err(ContainerError::Invalid(_))));
    assert!(matches!(m.list(s, Runtime::Nerdctl, false).await, Err(ContainerError::NotInstalled(ref b)) if b == "nerdctl"));
    assert!(matches!(m.list(Uuid::new_v4(), Runtime::Docker, false).await, Err(ContainerError::NoSession)));

    m.close(s).await;
    assert_eq!(m.open_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_local_inspect_and_logs() {
    if !enabled() {
        return;
    }
    let m = Arc::new(ContainerManager::new());
    let (s, _) = m.open(None).await.unwrap();
    let c = Chatty::start("logs");

    let json = m.inspect(s, Runtime::Docker, &c.id).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v[0]["Name"], format!("/{}", c.name));
    assert!(m.inspect(s, Runtime::Docker, "no-such-container-xyz").await.is_err());

    // A bounded read returns the last lines and ends by itself.
    let sink = Arc::new(Collect::default());
    until("some output", || true).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    m.start_logs(s, Runtime::Docker, &c.id, LogOptions { tail: 3, follow: false, timestamps: false }, sink.clone()).unwrap();
    until("the read to end", || sink.ended().is_some()).await;
    assert_eq!(sink.text().lines().count(), 3, "{:?}", sink.text());
    assert!(sink.text().contains("é✓"), "non-ASCII survives: {:?}", sink.text());
    assert!(matches!(sink.ended(), Some(LogEvent::End { code: Some(0), error: None })));

    // Following: new lines keep arriving until it is stopped, and then nothing more does.
    let sink = Arc::new(Collect::default());
    let stream = m.start_logs(s, Runtime::Docker, &c.id, LogOptions { tail: 1, follow: true, timestamps: true }, sink.clone()).unwrap();
    until("five lines", || sink.text().lines().count() >= 5).await;
    assert!(sink.ended().is_none());
    assert!(sink.text().lines().all(|l| l.contains("line ")), "timestamps asked for: {:?}", sink.text());
    m.stop_stream(stream);
    until("the stream to report its end", || sink.ended().is_some()).await;
    let n = sink.text().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sink.text().len(), n, "nothing arrives after stop");

    // A container that doesn't exist: the runtime's error arrives as text and a failing code.
    let sink = Arc::new(Collect::default());
    m.start_logs(s, Runtime::Docker, "no-such-container-xyz", LogOptions { tail: 10, follow: false, timestamps: false }, sink.clone()).unwrap();
    until("the failed read to end", || sink.ended().is_some()).await;
    assert!(sink.text().contains("No such container"), "{:?}", sink.text());
    assert!(matches!(sink.ended(), Some(LogEvent::End { code: Some(c), .. }) if c != 0));

    // Closing the session ends its streams.
    let sink = Arc::new(Collect::default());
    m.start_logs(s, Runtime::Docker, &c.id, LogOptions { tail: 1, follow: true, timestamps: false }, sink.clone()).unwrap();
    until("a line", || !sink.text().is_empty()).await;
    m.close(s).await;
    until("the stream to end with the session", || sink.ended().is_some()).await;
}

fn count_processes(pattern: &str) -> usize {
    let out = Command::new("pgrep").args(["-fc", pattern]).output().expect("pgrep");
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_over_ssh_reuses_one_connection_and_stopping_ends_the_remote_process() {
    use crate::ssh::testutil::{spawn_sshd, target};
    if !enabled() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let via = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let m = Arc::new(ContainerManager::new());
    let (s, runtimes) = m.open(Some(via)).await.unwrap();
    assert!(runtimes.contains(&Runtime::Docker), "detected over SSH: {runtimes:?}");
    let c = Chatty::start("ssh");

    for _ in 0..5 {
        let l = m.list(s, Runtime::Docker, false).await.unwrap();
        assert!(l.containers.iter().any(|x| x.name == c.name));
    }
    m.act(s, Runtime::Docker, Action::Restart, &c.id).await.unwrap();
    assert_eq!(state_of(&m, s, &c.name).await, Some(State::Running));
    assert!(m.inspect(s, Runtime::Docker, &c.id).await.unwrap().contains(&c.name));
    let err = m.act(s, Runtime::Docker, Action::Stop, "no-such-container-xyz").await.unwrap_err().to_string();
    assert!(err.contains("No such container"), "{err}");
    assert!(matches!(m.list(s, Runtime::Nerdctl, false).await, Err(ContainerError::NotInstalled(ref b)) if b == "nerdctl"));

    // Following over SSH: non-ASCII intact, and stopping hangs the remote `docker logs` up.
    let pattern = format!("docker logs.*{}", c.id);
    let sink = Arc::new(Collect::default());
    let stream = m.start_logs(s, Runtime::Docker, &c.id, LogOptions { tail: 1, follow: true, timestamps: false }, sink.clone()).unwrap();
    until("lines over ssh", || sink.text().lines().count() >= 4).await;
    assert!(sink.text().contains("é✓"), "{:?}", sink.text());
    until("the remote docker logs to be running", || count_processes(&pattern) >= 1).await;
    m.stop_stream(stream);
    until("the remote process to be gone", || count_processes(&pattern) == 0).await;
    until("the end event", || sink.ended().is_some()).await;

    // The connection is dropped and reopened on demand.
    let session = m.get(s).unwrap();
    session.transport.close().await;
    let l = m.list(s, Runtime::Docker, false).await.unwrap();
    assert!(l.containers.iter().any(|x| x.name == c.name), "listing works again after the connection was dropped");
    m.close(s).await;
}
