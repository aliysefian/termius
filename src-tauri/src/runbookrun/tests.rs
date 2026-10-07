use super::*;
use crate::runbook::{parse, StepStatus};
use crate::ssh::testutil::{spawn_sshd, target};

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
    manager.start(
        Uuid::new_v4().to_string(),
        rb,
        HashMap::from([("word".into(), "it's fine".into())]),
        HashMap::from([("conf".into(), b"key = value\n".to_vec())]),
        vec![
            HostJob { host_id: host, label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) },
            HostJob { host_id: bad, label: "nobody".into(), hostname: "x".into(), target: Err("credentials are needed".into()) },
        ],
        false,
        Some(history.clone()),
        Arc::new(Collect(tx)),
    );
    let mut steps = Vec::new();
    let mut done = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("run event") {
            RunbookEvent::Step { host_id, result } if host_id == host => steps.push(result),
            RunbookEvent::HostDone { host_id, ok, error } => {
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
    manager.start(id.clone(), rb, HashMap::new(), HashMap::new(), vec![HostJob { host_id: Uuid::new_v4(), label: "box".into(), hostname: "127.0.0.1".into(), target: Ok(t) }], false, Some(history.clone()), Arc::new(Collect(tx)));
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
