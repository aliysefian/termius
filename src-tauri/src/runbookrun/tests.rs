use super::*;
use crate::runbook::{parse, StepStatus};
use crate::runbookhistory::HostStatus;
use crate::ssh::testutil::{spawn_sshd, target};

/// Start a run the way a command does.
#[allow(clippy::too_many_arguments)]
fn begin(manager: &Arc<RunbookManager>, id: String, rb: crate::runbook::Runbook, values: HashMap<String, String>, files: HashMap<String, Vec<u8>>, hosts: Vec<HostJob>, strategy: Strategy, history: Option<History>, sink: Arc<dyn RunbookSink>) {
    manager.start(Start { run_id: id, runbook: rb, values, files, hosts, scheduled: false, rollback: false, strategy, from_step: None, retry_of: None, history }, sink);
}

struct Collect(std::sync::mpsc::Sender<RunbookEvent>);
impl RunbookSink for Collect {
    fn event(&self, e: RunbookEvent) {
        let _ = self.0.send(e);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runbook_runs_over_ssh_with_conditions_a_wait_and_an_upload() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let out = dir.path().join("uploaded.conf");
    let marker = dir.path().join("marker");
    let text = format!(
        r#"{{"name":"Try it","params":[{{"name":"word","default":"hello"}},{{"name":"conf","kind":"file"}}],"steps":[
            {{"name":"Say","id":"say","run":"echo {{{{word|q}}}}; echo oops >&2; exit 2","on_error":"continue"}},
            {{"name":"Only after a failure","run":"touch {m}","when":{{"step":"say","exit_not":0}}}},
            {{"name":"Not run","run":"echo never","when":{{"step":"say","exit":0}}}},
            {{"name":"Ready","wait":{{"run":"test -e {m}","every_secs":1,"timeout_secs":10}}}},
            {{"name":"Upload","upload":{{"local":"{{{{conf}}}}","remote":"{o}","mode":"0640"}}}}
        ]}}"#,
        m = marker.display(),
        o = out.display()
    );
    let rb = parse(&text).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let history = History::new(dir.path().join("history"));
    let host = Uuid::new_v4();
    let bad = Uuid::new_v4();
    begin(
        &manager,
        Uuid::new_v4().to_string(),
        rb,
        HashMap::from([("word".into(), "it's fine".into())]),
        HashMap::from([("conf".into(), b"key = value\n".to_vec())]),
        vec![
            HostJob { host_id: host, label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) },
            HostJob { host_id: bad, label: "nobody".into(), hostname: "x".into(), target: Err("credentials are needed".into()) },
        ],
        Strategy::Parallel,
        Some(history.clone()),
        Arc::new(Collect(tx)),
    );
    let mut steps = Vec::new();
    let mut done = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("run event") {
            RunbookEvent::Step { host_id, result } if host_id == host => steps.push(result),
            RunbookEvent::HostDone { host_id, ok, error, .. } => {
                done.insert(host_id, (ok, error));
            }
            RunbookEvent::Done { cancelled } => {
                assert!(!cancelled);
                break;
            }
            _ => {}
        }
    }
    let status: Vec<StepStatus> = steps.iter().map(|s| s.status).collect();
    assert_eq!(status, [StepStatus::Failed, StepStatus::Ok, StepStatus::Skipped, StepStatus::Ok, StepStatus::Ok]);
    assert_eq!(steps[0].output.stdout, "it's fine\n", "a value with a quote went through |q intact");
    assert_eq!(steps[0].output.stderr, "oops\n");
    assert_eq!(steps[0].output.exit_code, Some(2));
    assert!(marker.exists());
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "key = value\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o640);
    }
    assert!(!dir.path().join("uploaded.conf.sshvault-upload").exists(), "no half-written file is left");
    assert_eq!(done[&host], (true, None));
    assert_eq!(done[&bad], (false, Some("credentials are needed".into())));

    // The record of the run was kept.
    let list = history.list();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].hosts, list[0].ok, list[0].failed), (2, 1, 1));
    let rec = history.get(&list[0].id).unwrap();
    assert_eq!(rec.params["conf"], "(a file was chosen)");
    assert_eq!(rec.hosts[0].steps.len(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_keeps_what_happened_and_says_so() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let rb = parse(r#"{"name":"Slow","steps":[{"name":"Quick","run":"echo hi"},{"name":"Slow","run":"sleep 30"}]}"#).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let history = History::new(dir.path().join("history"));
    let id = Uuid::new_v4().to_string();
    begin(&manager, id.clone(), rb, HashMap::new(), HashMap::new(), vec![HostJob { host_id: Uuid::new_v4(), label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) }], Strategy::Parallel, Some(history.clone()), Arc::new(Collect(tx)));
    // Wait for the slow step to start.
    loop {
        if matches!(rx.recv_timeout(Duration::from_secs(20)).expect("event"), RunbookEvent::StepStarted { index: 1, .. }) {
            break;
        }
    }
    assert!(manager.cancel(&id));
    assert!(!manager.cancel(&id));
    let rec = history.get(&id).expect("the record was kept");
    assert!(rec.cancelled && rec.finished_at.is_some());
    assert_eq!(rec.hosts[0].steps.len(), 1, "the finished step is there, the slow one is not");
    assert_eq!(rec.hosts[0].error.as_deref(), Some("cancelled"));
}

fn run_order(order: Strategy, targets: Vec<Result<crate::ssh::Target, String>>) -> Vec<(bool, Option<String>)> {
    run_strategy(order, targets).into_iter().map(|(ok, e, _)| (ok, e)).collect()
}

fn run_strategy(order: Strategy, targets: Vec<Result<crate::ssh::Target, String>>) -> Vec<(bool, Option<String>, HostStatus)> {
    let rb = parse(r#"{"name":"Roll","steps":[{"name":"Go","run":"true"}]}"#).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let ids: Vec<Uuid> = targets.iter().map(|_| Uuid::new_v4()).collect();
    let jobs = ids.iter().zip(targets).map(|(id, target)| HostJob { host_id: *id, label: "h".into(), hostname: "x".into(), target }).collect();
    begin(&manager, Uuid::new_v4().to_string(), rb, HashMap::new(), HashMap::new(), jobs, order, None, Arc::new(Collect(tx)));
    let mut done = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("run event") {
            RunbookEvent::HostDone { host_id, ok, error, status, .. } => {
                done.insert(host_id, (ok, error, status));
            }
            RunbookEvent::Done { .. } => break,
            _ => {}
        }
    }
    ids.iter().map(|i| done[i].clone()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_at_a_time_can_stop_at_the_first_failure() {
    let bad = || Err::<crate::ssh::Target, String>("credentials are needed".into());
    let stop = tokio::task::spawn_blocking(move || run_order(Strategy::Sequential { stop_on_failure: true }, vec![bad(), bad(), bad()])).await.unwrap();
    assert_eq!(stop[0], (false, Some("credentials are needed".into())));
    assert_eq!(stop[1].1.as_deref(), Some("skipped: an earlier host failed"));
    assert_eq!(stop[2].1.as_deref(), Some("skipped: an earlier host failed"));
    let carry = tokio::task::spawn_blocking(move || run_order(Strategy::Sequential { stop_on_failure: false }, vec![bad(), bad()])).await.unwrap();
    assert!(carry.iter().all(|c| c.1.as_deref() == Some("credentials are needed")), "no host was skipped");
}

#[test]
fn secret_values_are_taken_out_of_what_is_shown_and_kept() {
    let r = StepResult {
        phase: crate::runbook::Phase::Run,
        index: 0,
        name: "Login".into(),
        status: StepStatus::Failed,
        output: Output { stdout: "welcome hunter2\n".into(), stderr: "bad password hunter2".into(), exit_code: Some(1), ..Default::default() },
        note: "tried hunter2".into(),
    };
    let r = redacted(&r, &["hunter2".to_string()]);
    assert_eq!((r.output.stdout.as_str(), r.output.stderr.as_str(), r.note.as_str()), ("welcome [hidden]\n", "bad password [hidden]", "tried [hidden]"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_does_not_leave_the_remote_command_running() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let pidfile = dir.path().join("pid");
    let text = format!(r#"{{"name":"Slow","steps":[{{"name":"Slow","run":"echo $$ > {}; exec sleep 300"}}]}}"#, pidfile.display());
    let rb = parse(&text).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let id = Uuid::new_v4().to_string();
    begin(&manager, id.clone(), rb, HashMap::new(), HashMap::new(), vec![HostJob { host_id: Uuid::new_v4(), label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) }], Strategy::Parallel, None, Arc::new(Collect(tx)));
    loop {
        if matches!(rx.recv_timeout(Duration::from_secs(20)).expect("event"), RunbookEvent::StepStarted { .. }) {
            break;
        }
    }
    let pid: i32 = loop {
        if let Some(p) = std::fs::read_to_string(&pidfile).ok().and_then(|t| t.trim().parse().ok()) {
            break p;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(manager.cancel(&id));
    let alive = || std::path::Path::new(&format!("/proc/{pid}")).exists();
    for _ in 0..60 {
        if !alive() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the remote command {pid} is still running six seconds after cancel");
}

fn bad() -> Result<crate::ssh::Target, String> {
    Err("credentials are needed".into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_that_stops_everything_skips_hosts_not_started_and_says_why() {
    // One at a time through a limit of one, so the order is known: the first fails, the rest never start.
    let r = tokio::task::spawn_blocking(|| run_strategy(Strategy::Concurrent { limit: 1, stop_on_failure: true }, vec![bad(), bad(), bad()])).await.unwrap();
    assert_eq!((r[0].2, r[0].1.as_deref()), (HostStatus::Failed, Some("credentials are needed")));
    for host in &r[1..] {
        assert_eq!(host.2, HostStatus::Skipped);
        assert!(host.1.as_deref().unwrap().starts_with("skipped: another host failed"));
    }
    // Without it, every host is tried.
    let r = tokio::task::spawn_blocking(|| run_strategy(Strategy::Concurrent { limit: 2, stop_on_failure: false }, vec![bad(), bad(), bad()])).await.unwrap();
    assert!(r.iter().all(|h| h.2 == HostStatus::Failed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rolling_run_stops_after_a_batch_with_a_failure_when_asked() {
    let strategy = |stop| Strategy::Rolling { batch_size: 2, concurrency: None, stop_on_failure: stop };
    let r = tokio::task::spawn_blocking(move || run_strategy(strategy(true), vec![bad(), bad(), bad(), bad(), bad()])).await.unwrap();
    // The first batch of two ran (and failed); the other three were not contacted.
    assert_eq!(r.iter().map(|h| h.2).collect::<Vec<_>>(), [HostStatus::Failed, HostStatus::Failed, HostStatus::Skipped, HostStatus::Skipped, HostStatus::Skipped]);
    assert_eq!(r[2].1.as_deref(), Some("skipped: an earlier batch had a failed host"));
    let r = tokio::task::spawn_blocking(move || run_strategy(strategy(false), vec![bad(), bad(), bad()])).await.unwrap();
    assert!(r.iter().all(|h| h.2 == HostStatus::Failed), "every batch ran");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_concurrency_limit_and_rolling_batches_are_really_kept() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    // Every step writes when it starts and when it ends; the overlap of those intervals is the concurrency that happened.
    let log = dir.path().join("log");
    let text = format!(r#"{{"name":"Overlap","steps":[{{"name":"Hold","run":"echo \"s $(date +%s%N)\" >> {l}; sleep 0.6; echo \"e $(date +%s%N)\" >> {l}"}}]}}"#, l = log.display());
    let max_overlap = |log: &std::path::Path| {
        let mut events: Vec<(u128, i32)> = std::fs::read_to_string(log).unwrap().lines().map(|l| {
            let (k, t) = l.split_once(' ').unwrap();
            (t.trim().parse().unwrap(), if k == "s" { 1 } else { -1 })
        }).collect();
        events.sort();
        let (mut cur, mut max) = (0, 0);
        for (_, d) in events {
            cur += d;
            max = max.max(cur);
        }
        max
    };
    let go = |strategy: Strategy, hosts: usize| {
        let _ = std::fs::remove_file(&log);
        let rb = parse(&text).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let manager = Arc::new(RunbookManager::new());
        let jobs = (0..hosts)
            .map(|i| HostJob { host_id: Uuid::new_v4(), label: format!("h{i}"), hostname: "127.0.0.1".into(), target: Ok(target(&sshd, &sshd.client_key, dir.path().join(format!("kh{i}")))) })
            .collect();
        let started = Instant::now();
        begin(&manager, Uuid::new_v4().to_string(), rb, HashMap::new(), HashMap::new(), jobs, strategy, None, Arc::new(Collect(tx)));
        loop {
            if matches!(rx.recv_timeout(Duration::from_secs(60)).expect("event"), RunbookEvent::Done { .. }) {
                break;
            }
        }
        (max_overlap(&log), started.elapsed())
    };
    let (two, _) = tokio::task::block_in_place(|| go(Strategy::Concurrent { limit: 2, stop_on_failure: false }, 5));
    assert_eq!(two, 2, "never more than the limit, and the limit was used");
    let (one, _) = tokio::task::block_in_place(|| go(Strategy::Sequential { stop_on_failure: false }, 3));
    assert_eq!(one, 1);
    let (batch, _) = tokio::task::block_in_place(|| go(Strategy::Rolling { batch_size: 3, concurrency: Some(2), stop_on_failure: false }, 6));
    assert_eq!(batch, 2, "a batch of three, two at a time inside it");
    let (all, _) = tokio::task::block_in_place(|| go(Strategy::Rolling { batch_size: 3, concurrency: None, stop_on_failure: false }, 6));
    assert_eq!(all, 3, "batches don't overlap each other");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_steps_output_arrives_while_it_runs_with_secrets_hidden_and_whole_lines() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let rb = parse(r#"{"name":"Live","params":[{"name":"pw","kind":"secret"}],"steps":[{"name":"Talk","run":"echo first; echo token={{pw|q}} >&2; sleep 2; printf 'no newline'; echo; echo last"}]}"#).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let host = Uuid::new_v4();
    begin(&manager, Uuid::new_v4().to_string(), rb, HashMap::from([("pw".into(), "s3cret-value".into())]), HashMap::new(), vec![HostJob { host_id: host, label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) }], Strategy::Parallel, None, Arc::new(Collect(tx)));
    let begun = Instant::now();
    let mut lines: Vec<(Stream, String, u64, Duration)> = Vec::new();
    let mut step_done_at = None;
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("event") {
            RunbookEvent::Output { stream, text, seq, .. } => lines.push((stream, text, seq, begun.elapsed())),
            RunbookEvent::Step { .. } => step_done_at = Some(begun.elapsed()),
            RunbookEvent::Done { .. } => break,
            _ => {}
        }
    }
    let all: String = lines.iter().map(|l| l.1.as_str()).collect();
    assert!(all.contains("first\n") && all.contains("no newline\n") && all.contains("last\n"), "{all:?}");
    assert!(all.contains("token=[hidden]") && !all.contains("s3cret-value"), "{all:?}");
    assert!(lines.iter().any(|l| l.0 == Stream::Stderr && l.1.contains("token=")));
    // "first" was shown long before the step ended (the sleep is two seconds).
    let first_at = lines.iter().find(|l| l.1.contains("first")).unwrap().3;
    assert!(step_done_at.unwrap() - first_at > Duration::from_millis(1000), "first at {first_at:?}, step done at {step_done_at:?}");
    // The pieces are numbered in order, none missing.
    assert!(lines.windows(2).all(|w| w[1].2 == w[0].2 + 1));
    assert_eq!(lines[0].2, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_marks_every_unfinished_host_and_tells_the_window() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let rb = parse(r#"{"name":"Slow","steps":[{"name":"Slow","run":"sleep 30"}]}"#).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let history = History::new(dir.path().join("history"));
    let id = Uuid::new_v4().to_string();
    let hosts: Vec<HostJob> = (0..3).map(|i| HostJob { host_id: Uuid::new_v4(), label: format!("h{i}"), hostname: "127.0.0.1".into(), target: Ok(target(&sshd, &sshd.client_key, dir.path().join(format!("kh{i}")))) }).collect();
    // One at a time: the first runs, the other two are waiting.
    begin(&manager, id.clone(), rb, HashMap::new(), HashMap::new(), hosts, Strategy::Sequential { stop_on_failure: false }, Some(history.clone()), Arc::new(Collect(tx)));
    loop {
        if matches!(rx.recv_timeout(Duration::from_secs(20)).expect("event"), RunbookEvent::StepStarted { .. }) {
            break;
        }
    }
    assert!(manager.cancel(&id));
    let mut done = Vec::new();
    let mut cancelled = false;
    while let Ok(e) = rx.recv_timeout(Duration::from_secs(5)) {
        match e {
            RunbookEvent::HostDone { status, error, .. } => done.push((status, error)),
            RunbookEvent::Done { cancelled: c } => {
                cancelled = c;
                break;
            }
            _ => {}
        }
    }
    assert!(cancelled);
    assert_eq!(done.len(), 3, "every host was reported");
    assert!(done.iter().all(|d| d.0 == HostStatus::Cancelled));
    assert_eq!(done.iter().filter(|d| d.1.as_deref() == Some("cancelled before it started")).count(), 2);
    let rec = history.get(&id).unwrap();
    assert!(rec.hosts.iter().all(|h| h.status == Some(HostStatus::Cancelled) && h.finished_ms.is_some()));
    assert_eq!(manager.running(), 0, "nothing is left running");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_run_records_how_it_was_started_and_how_long_each_host_took() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let rb = parse(r#"{"name":"Rec","steps":[{"name":"One","run":"true"},{"name":"Two","run":"true"}]}"#).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let history = History::new(dir.path().join("history"));
    let id = Uuid::new_v4().to_string();
    let first = Uuid::new_v4().to_string();
    manager.start(
        Start { run_id: id.clone(), runbook: rb, values: HashMap::new(), files: HashMap::new(), hosts: vec![HostJob { host_id: Uuid::new_v4(), label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(target(&sshd, &sshd.client_key, dir.path().join("kh"))) }], scheduled: false, rollback: false, strategy: Strategy::Concurrent { limit: 2, stop_on_failure: false }, from_step: Some(1), retry_of: Some(first.clone()), history: Some(history.clone()) },
        Arc::new(Collect(tx)),
    );
    let mut order = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("event") {
            RunbookEvent::HostStarted { .. } => order.push("started"),
            RunbookEvent::Connected { .. } => order.push("connected"),
            RunbookEvent::HostDone { status, .. } => {
                assert_eq!(status, HostStatus::Success);
                order.push("done");
            }
            RunbookEvent::Done { .. } => break,
            _ => {}
        }
    }
    assert_eq!(order, ["started", "connected", "done"]);
    let rec = history.get(&id).unwrap();
    assert_eq!((rec.trigger.as_deref(), rec.retry_of.as_deref(), rec.from_step), (Some("retry"), Some(first.as_str()), Some(1)));
    assert_eq!(rec.strategy, Some(Strategy::Concurrent { limit: 2, stop_on_failure: false }));
    assert_eq!(rec.step_names, ["One", "Two"]);
    let h = &rec.hosts[0];
    assert_eq!(h.status, Some(HostStatus::Success));
    assert!(h.finished_ms.unwrap() >= h.started_ms.unwrap());
    assert_eq!(h.steps.iter().map(|s| s.status).collect::<Vec<_>>(), [StepStatus::Skipped, StepStatus::Ok]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_over_ssh_saves_the_file_and_a_connection_test_really_connects() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let remote = dir.path().join("report.txt");
    std::fs::write(&remote, "line one\nline two\n").unwrap();
    let save_to = dir.path().join("saved");
    std::fs::create_dir(&save_to).unwrap();
    let rb = parse(&format!(r#"{{"name":"Fetch","params":[{{"name":"dest","kind":"dir"}}],"steps":[{{"name":"Get","download":{{"remote":"{}","to":"{{{{dest}}}}"}}}},{{"name":"Missing","download":{{"remote":"{}/nope.txt","to":"{{{{dest}}}}"}}}}]}}"#, remote.display(), dir.path().display())).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let manager = Arc::new(RunbookManager::new());
    let mut steps = Vec::new();
    begin(&manager, Uuid::new_v4().to_string(), rb, HashMap::from([("dest".into(), save_to.display().to_string())]), HashMap::new(), vec![HostJob { host_id: Uuid::new_v4(), label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(target(&sshd, &sshd.client_key, dir.path().join("kh"))) }], Strategy::Parallel, None, Arc::new(Collect(tx)));
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("event") {
            RunbookEvent::Step { result, .. } => steps.push(result),
            RunbookEvent::Done { .. } => break,
            _ => {}
        }
    }
    assert_eq!(steps[0].status, StepStatus::Ok, "{:?}", steps[0]);
    assert_eq!(std::fs::read_to_string(save_to.join("box__report.txt")).unwrap(), "line one\nline two\n");
    assert_eq!(steps[1].status, StepStatus::Error);
    assert!(steps[1].note.contains("No such file"), "{}", steps[1].note);

    let checks = connect_check(vec![
        HostJob { host_id: Uuid::new_v4(), label: "up".into(), hostname: "127.0.0.1".into(), target: Ok(target(&sshd, &sshd.client_key, dir.path().join("kh2"))) },
        HostJob { host_id: Uuid::new_v4(), label: "none".into(), hostname: "x".into(), target: Err("credentials are needed".into()) },
    ])
    .await;
    assert_eq!((checks[0].label.as_str(), checks[0].ok, checks[0].error.is_none()), ("up", true, true));
    assert_eq!((checks[1].ok, checks[1].error.as_deref()), (false, Some("credentials are needed")));
}

#[test]
fn live_lines_are_whole_and_capped() {
    #[derive(Default)]
    struct Got(Mutex<Vec<String>>);
    impl OutputSink for Got {
        fn chunk(&self, _: Stream, text: &str) {
            self.0.lock().unwrap().push(text.to_string());
        }
    }
    let got = Got::default();
    let mut l = Lines::new();
    l.push(b"par", Stream::Stdout, &got);
    l.push(b"tial\nsecond\nthi", Stream::Stdout, &got);
    l.push(b"rd", Stream::Stdout, &got);
    l.flush(Stream::Stdout, &got);
    assert_eq!(*got.0.lock().unwrap(), ["partial\nsecond\n", "third"]);
    // A very long output stops being shown live, once, with a note.
    let big = Got::default();
    let mut l = Lines::new();
    for _ in 0..(LIVE_BYTES_PER_STEP / 1024 + 10) {
        l.push(&[b'x'; 1023].iter().chain(b"\n").copied().collect::<Vec<u8>>(), Stream::Stdout, &big);
    }
    let all = big.0.lock().unwrap();
    assert!(all.last().unwrap().contains("more output than is shown live"));
    assert_eq!(all.iter().filter(|c| c.contains("more output than is shown live")).count(), 1);
    assert!(all.iter().map(String::len).sum::<usize>() < LIVE_BYTES_PER_STEP + 1024);
}
