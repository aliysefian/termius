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

// -- C1: images, volumes, networks, Compose ---------------------------------------
//
// These run against a Docker that has other people's things on it, so they
// only ever name resources called `sshvault-ct-*` and never run the runtime's
// own prune.

fn docker(args: &[&str]) -> std::process::Output {
    Command::new("docker").args(args).output().expect("docker")
}

fn docker_ok(args: &[&str]) {
    let o = docker(args);
    assert!(o.status.success(), "docker {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// Removes what a test made, even when it fails halfway.
#[derive(Default)]
struct Cleanup {
    containers: Vec<String>,
    volumes: Vec<String>,
    networks: Vec<String>,
    images: Vec<String>,
    projects: Vec<String>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.projects {
            let _ = docker(&["compose", "-p", p, "down", "-v"]);
        }
        for c in &self.containers {
            let _ = docker(&["rm", "-f", c]);
        }
        for n in &self.networks {
            let _ = docker(&["network", "rm", n]);
        }
        for v in &self.volumes {
            let _ = docker(&["volume", "rm", "-f", v]);
        }
        for i in &self.images {
            let _ = docker(&["rmi", i]);
        }
    }
}

async fn images_of(m: &ContainerManager, s: Uuid) -> Vec<parse::Image> {
    m.list(s, Runtime::Docker, false).await.unwrap().images
}

fn unique(tag: &str) -> String {
    format!("sshvault-ct-{}-{}", std::process::id(), tag)
}

fn ids(items: &[prune::PruneItem]) -> Vec<String> {
    items.iter().map(|i| i.id.clone()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_images_pull_remove_and_prune() {
    if !enabled() {
        return;
    }
    let m = Arc::new(ContainerManager::new());
    let (s, _) = m.open(None).await.unwrap();
    let mut cl = Cleanup::default();

    // Pulling streams progress and ends. (The image is already here, so nothing big is fetched; a host
    // with no route to the registry is skipped rather than failed.)
    let sink = Arc::new(Collect::default());
    m.start_pull(s, Runtime::Docker, "hello-world", sink.clone()).unwrap();
    until("the pull to end", || sink.ended().is_some()).await;
    match sink.ended() {
        Some(LogEvent::End { code: Some(0), error: None }) => assert!(sink.text().contains("hello-world") || sink.text().contains("Status:"), "{:?}", sink.text()),
        other => eprintln!("pull didn't succeed here (no registry access?): {other:?} {:?}", sink.text()),
    }
    // A reference that doesn't exist ends with the runtime's error, not a hang.
    let sink = Arc::new(Collect::default());
    m.start_pull(s, Runtime::Docker, "sshvault-ct-no-such-image-xyz:nope", sink.clone()).unwrap();
    until("the failed pull to end", || sink.ended().is_some()).await;
    assert!(matches!(sink.ended(), Some(LogEvent::End { code: Some(c), .. }) if c != 0), "{:?}", sink.ended());
    assert!(matches!(m.start_pull(s, Runtime::Docker, "--all-tags", Arc::new(Collect::default())), Err(ContainerError::Invalid(_))));

    // Extra tags of an image that is here, so nothing is downloaded and nothing of the owner's is touched.
    let (a, b, used) = (format!("{}:a", unique("img")), format!("{}:b", unique("img")), format!("{}:used", unique("img")));
    docker_ok(&["tag", "hello-world", &a]);
    docker_ok(&["tag", "hello-world", &b]);
    cl.images.extend([a.clone(), b.clone()]);
    assert!(images_of(&m, s).await.iter().any(|i| format!("{}:{}", i.repository, i.tag) == a));

    // Remove one tag; the image stays (hello-world still names it).
    m.remove(s, Runtime::Docker, ResourceKind::Image, &a, false).await.unwrap();
    assert!(!images_of(&m, s).await.iter().any(|i| format!("{}:{}", i.repository, i.tag) == a));
    assert!(images_of(&m, s).await.iter().any(|i| i.repository == "hello-world"));
    let err = m.remove(s, Runtime::Docker, ResourceKind::Image, "sshvault-ct-no-such-image-xyz:nope", false).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("no such image"), "{err}");

    // Prune preview: unused tags are listed, tags of an image a container uses are not.
    docker_ok(&["tag", "hello-world", &a]);
    let preview = m.prune_preview(s, Runtime::Docker, PruneKind::Images { all: true }).await.unwrap();
    assert!(ids(&preview).contains(&a) && ids(&preview).contains(&b), "{:?}", ids(&preview).iter().filter(|i| i.contains("sshvault-ct")).collect::<Vec<_>>());
    let dangling = m.prune_preview(s, Runtime::Docker, PruneKind::Images { all: false }).await.unwrap();
    assert!(!ids(&dangling).contains(&a), "tagged images aren't in the untagged-only preview");

    let chatty = Chatty::start("imgused");
    docker_ok(&["tag", &image(), &used]);
    cl.images.push(used.clone());
    let preview = m.prune_preview(s, Runtime::Docker, PruneKind::Images { all: true }).await.unwrap();
    assert!(!ids(&preview).contains(&used), "an image a running container uses is never a candidate");
    assert!(!ids(&preview).contains(&image()), "nor is the container's own tag");
    drop(chatty);

    // Only what is listed goes. hello-world and everything else here stays.
    let results = m.prune_run(s, Runtime::Docker, PruneKind::Images { all: true }, vec![a.clone(), b.clone()]).await.unwrap();
    assert!(results.iter().all(|r| r.ok), "{results:?}");
    let after = images_of(&m, s).await;
    assert!(!after.iter().any(|i| format!("{}:{}", i.repository, i.tag) == a || format!("{}:{}", i.repository, i.tag) == b));
    assert!(after.iter().any(|i| i.repository == "hello-world"), "the shared image itself was not removed");
    cl.images.retain(|i| *i != a && *i != b);

    // Something started using an image between the preview and the click: it is skipped, not forced.
    docker_ok(&["tag", "hello-world", &a]);
    cl.images.push(a.clone());
    let held = unique("imgheld");
    cl.containers.push(held.clone());
    docker_ok(&["create", "--name", &held, "hello-world"]);
    let results = m.prune_run(s, Runtime::Docker, PruneKind::Images { all: true }, vec![a.clone()]).await.unwrap();
    assert!(!results[0].ok && results[0].error.as_deref().unwrap().contains("left alone"), "{results:?}");
    assert!(images_of(&m, s).await.iter().any(|i| format!("{}:{}", i.repository, i.tag) == a), "it is still there");
    // An id the preview never offered is refused the same way.
    let results = m.prune_run(s, Runtime::Docker, PruneKind::Images { all: true }, vec!["postgres:16".into(), "sshvault-ct-ghost:1".into()]).await.unwrap();
    assert!(results.iter().all(|r| !r.ok), "{results:?}");
    m.close(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_volumes_and_networks_usage_remove_and_prune() {
    if !enabled() {
        return;
    }
    let m = Arc::new(ContainerManager::new());
    let (s, _) = m.open(None).await.unwrap();
    let mut cl = Cleanup::default();
    let (v_free, v_used, n_free, n_used, boxname) = (unique("vfree"), unique("vused"), unique("nfree"), unique("nused"), unique("box"));
    cl.containers.push(boxname.clone());
    cl.volumes.extend([v_free.clone(), v_used.clone()]);
    cl.networks.extend([n_free.clone(), n_used.clone()]);
    docker_ok(&["volume", "create", &v_free]);
    docker_ok(&["volume", "create", &v_used]);
    docker_ok(&["network", "create", &n_free]);
    docker_ok(&["network", "create", &n_used]);
    // Stopped, never started: a stopped container still holds its volume and network.
    docker_ok(&["create", "--name", &boxname, "--network", &n_used, "-v", &format!("{v_used}:/data"), "-v", "/tmp:/hostdir:ro", "hello-world"]);

    let r = m.resources(s, Runtime::Docker, false).await.unwrap();
    let vol = |n: &str| r.volumes.iter().find(|v| v.name == n).unwrap_or_else(|| panic!("{n} missing"));
    let net = |n: &str| r.networks.iter().find(|v| v.name == n).unwrap_or_else(|| panic!("{n} missing"));
    assert_eq!(vol(&v_used).used_by, vec![boxname.clone()], "a stopped container still uses its volume");
    assert!(vol(&v_free).used_by.is_empty());
    assert_eq!(net(&n_used).used_by, vec![boxname.clone()]);
    assert!(net(&n_free).used_by.is_empty());
    assert!(["bridge", "host", "none"].iter().all(|n| net(n).predefined));
    assert!(!net(&n_free).predefined);
    assert_eq!(vol(&v_free).driver, "local");
    assert!(vol(&v_free).size.is_none(), "no size unless asked");
    assert!(r.networks.windows(2).all(|w| (w[0].predefined, w[0].name.to_lowercase()) <= (w[1].predefined, w[1].name.to_lowercase())), "predefined networks sort last");

    // Sizes come from `system df`, only when asked for.
    let sized = m.resources(s, Runtime::Docker, true).await.unwrap();
    assert!(sized.sizes_error.is_none(), "{:?}", sized.sizes_error);
    assert!(sized.volumes.iter().find(|v| v.name == v_free).unwrap().size.is_some());

    // Previews offer the free ones, never the used ones, never bridge/host/none.
    let vp = ids(&m.prune_preview(s, Runtime::Docker, PruneKind::Volumes).await.unwrap());
    assert!(vp.contains(&v_free) && !vp.contains(&v_used));
    let np = ids(&m.prune_preview(s, Runtime::Docker, PruneKind::Networks).await.unwrap());
    assert!(np.contains(&n_free) && !np.contains(&n_used));
    assert!(!np.iter().any(|n| ["bridge", "host", "none"].contains(&n.as_str())));

    // Remove exactly the offered ones. A used one that is named anyway is skipped, not removed.
    let res = m.prune_run(s, Runtime::Docker, PruneKind::Volumes, vec![v_free.clone(), v_used.clone(), v_free.clone()]).await.unwrap();
    assert_eq!(res.iter().map(|r| r.ok).collect::<Vec<_>>(), vec![true, false, false], "{res:?}");
    assert!(res[1].error.as_deref().unwrap().contains("left alone"));
    let res = m.prune_run(s, Runtime::Docker, PruneKind::Networks, vec![n_free.clone(), n_used.clone(), "bridge".into()]).await.unwrap();
    assert_eq!(res.iter().map(|r| r.ok).collect::<Vec<_>>(), vec![true, false, false], "{res:?}");
    let r = m.resources(s, Runtime::Docker, false).await.unwrap();
    assert!(!r.volumes.iter().any(|v| v.name == v_free) && r.volumes.iter().any(|v| v.name == v_used));
    assert!(!r.networks.iter().any(|n| n.name == n_free) && r.networks.iter().any(|n| n.name == n_used) && r.networks.iter().any(|n| n.name == "bridge"));

    // Single removal: the runtime's refusal comes back in its own words.
    let err = m.remove(s, Runtime::Docker, ResourceKind::Volume, &v_used, false).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("in use"), "{err}");
    let err = m.remove(s, Runtime::Docker, ResourceKind::Network, "bridge", false).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("pre-defined") || err.to_lowercase().contains("predefined"), "{err}");
    docker_ok(&["rm", &boxname]);
    m.remove(s, Runtime::Docker, ResourceKind::Network, &n_used, false).await.unwrap();
    m.remove(s, Runtime::Docker, ResourceKind::Volume, &v_used, false).await.unwrap();
    assert!(matches!(m.resources(s, Runtime::Podman, false).await, Err(ContainerError::Invalid(_))), "Docker only for now");
    assert!(matches!(m.remove(s, Runtime::Podman, ResourceKind::Volume, "x", false).await, Err(ContainerError::Invalid(_))));
    m.close(s).await;
}

fn write_compose(project: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(project);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("compose.yaml");
    std::fs::write(
        &file,
        format!("services:\n  web:\n    image: {img}\n    command: [\"sleep\", \"1000\"]\n  cache:\n    image: {img}\n    command: [\"sleep\", \"1000\"]\n    volumes: [\"data:/data\"]\nvolumes:\n  data: {{}}\n", img = image()),
    )
    .unwrap();
    file
}

async fn project_states(m: &ContainerManager, s: Uuid, project: &str) -> Vec<(String, State)> {
    let mut v: Vec<_> = m
        .list(s, Runtime::Docker, false)
        .await
        .unwrap()
        .containers
        .into_iter()
        .filter(|c| c.labels.get("com.docker.compose.project").map(String::as_str) == Some(project))
        .map(|c| (c.name, c.state))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_compose_projects_act_by_name() {
    use crate::ssh::testutil::{spawn_sshd, target};
    if !enabled() {
        return;
    }
    let project = unique("proj").replace('_', "-");
    let mut cl = Cleanup::default();
    cl.projects.push(project.clone());
    let file = write_compose(&project);
    docker_ok(&["compose", "-p", &project, "-f", file.to_str().unwrap(), "up", "-d"]);

    let m = Arc::new(ContainerManager::new());
    let (s, _) = m.open(None).await.unwrap();
    let both = |st: State| vec![(format!("{project}-cache-1"), st), (format!("{project}-web-1"), st)];
    assert_eq!(project_states(&m, s, &project).await, both(State::Running));
    // The labels the view groups by are really there.
    let web = m.list(s, Runtime::Docker, false).await.unwrap().containers.into_iter().find(|c| c.name == format!("{project}-web-1")).unwrap();
    assert_eq!(web.labels.get("com.docker.compose.service").map(String::as_str), Some("web"));
    assert_eq!(web.labels.get("com.docker.compose.project.working_dir").map(String::as_str), Some(file.parent().unwrap().to_str().unwrap()));

    m.compose(s, &project, ComposeVerb::Stop).await.unwrap();
    assert_eq!(project_states(&m, s, &project).await, both(State::Exited));
    m.compose(s, &project, ComposeVerb::Start).await.unwrap();
    assert_eq!(project_states(&m, s, &project).await, both(State::Running));
    m.compose(s, &project, ComposeVerb::Restart).await.unwrap();
    assert_eq!(project_states(&m, s, &project).await, both(State::Running));

    // Down removes containers and the project's network, and keeps its volume.
    m.compose(s, &project, ComposeVerb::Down).await.unwrap();
    assert!(project_states(&m, s, &project).await.is_empty());
    let r = m.resources(s, Runtime::Docker, false).await.unwrap();
    assert!(!r.networks.iter().any(|n| n.name == format!("{project}_default")), "its network went with it");
    let data = r.volumes.iter().find(|v| v.name == format!("{project}_data")).expect("the volume is kept by down");
    assert!(data.used_by.is_empty() && data.labels.get("com.docker.compose.project") == Some(&project));
    // ...so it now shows up as unused, labelled with its project.
    let vp = m.prune_preview(s, Runtime::Docker, PruneKind::Volumes).await.unwrap();
    assert_eq!(vp.iter().find(|i| i.id == format!("{project}_data")).unwrap().project.as_deref(), Some(project.as_str()));

    // A name that isn't a project name never reaches the runtime.
    assert!(matches!(m.compose(s, "-f", ComposeVerb::Down).await, Err(ContainerError::Invalid(_))));
    assert!(matches!(m.compose(s, "a b", ComposeVerb::Stop).await, Err(ContainerError::Invalid(_))));

    // The same, and a listing of volumes and a pull, over SSH.
    let dir = tempfile::TempDir::new().unwrap();
    if let Some(sshd) = spawn_sshd(dir.path()) {
        docker_ok(&["compose", "-p", &project, "-f", file.to_str().unwrap(), "up", "-d"]);
        let via = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let (rs, _) = m.open(Some(via)).await.unwrap();
        m.compose(rs, &project, ComposeVerb::Stop).await.unwrap();
        assert_eq!(project_states(&m, rs, &project).await, both(State::Exited));
        m.compose(rs, &project, ComposeVerb::Start).await.unwrap();
        assert_eq!(project_states(&m, rs, &project).await, both(State::Running));
        let r = m.resources(rs, Runtime::Docker, false).await.unwrap();
        assert!(r.volumes.iter().any(|v| v.name == format!("{project}_data") && v.used_by.contains(&format!("{project}-cache-1"))));
        let sink = Arc::new(Collect::default());
        m.start_pull(rs, Runtime::Docker, "hello-world", sink.clone()).unwrap();
        until("the pull over ssh to end", || sink.ended().is_some()).await;
        assert!(!sink.text().is_empty() || matches!(sink.ended(), Some(LogEvent::End { error: Some(_), .. })), "{:?}", sink.ended());
        m.compose(rs, &project, ComposeVerb::Down).await.unwrap();
        m.close(rs).await;
    }
    m.close(s).await;
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}
