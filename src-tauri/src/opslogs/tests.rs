use super::*;
use crate::ssh::testutil::{spawn_sshd, target};
use std::io::Write;
use std::sync::mpsc as std_mpsc;

fn journal(unit: Option<&str>, history: History) -> Spec {
    Spec { source: Source::Journal { unit: unit.map(String::from), kernel: false, priority: None }, history, auto_reconnect: true, redact: true }
}

fn file(path: &str, last: u32) -> Spec {
    Spec { source: Source::File { path: path.into() }, history: History::Last(last), auto_reconnect: true, redact: true }
}

// -- what is run -----------------------------------------------------------------------------------

#[test]
fn the_script_follows_the_journal_as_json_with_each_filter_quoted() {
    let spec = Spec { source: Source::Journal { unit: Some("nginx.service".into()), kernel: true, priority: Some(4) }, history: History::Last(500), auto_reconnect: true, redact: true };
    assert_eq!(spec.script(None, false).unwrap(), "SYSTEMD_COLORS=0 SYSTEMD_URLIFY=0 exec journalctl --follow --no-pager -o json -k -u nginx.service -p 4 -n 500");
    // Carrying on after a cursor replaces the history.
    let s = spec.script(Some("s=abc;i=1;b=2;m=3;t=4;x=5"), true).unwrap();
    assert!(s.ends_with(&format!("--after-cursor {}", shell_quote("s=abc;i=1;b=2;m=3;t=4;x=5"))) && !s.contains(" -n "), "{s}");
    let since = journal(None, History::Since(1_700_000_000)).script(None, false).unwrap();
    assert!(since.ends_with(&format!("--since {}", shell_quote("2023-11-14 22:13:20 UTC"))), "{since}");
}

#[test]
fn input_that_could_reach_a_shell_is_refused_or_never_used() {
    for bad in ["x.service; rm -rf /", "$(id).service", "a b.service", "nginx", "nginx.sh", ".service", "a`b`.service", "a\nb.service", "a'b.service"] {
        assert!(!valid_unit(bad), "{bad}");
        assert!(journal(Some(bad), History::Last(10)).script(None, false).is_err(), "{bad}");
    }
    for ok in ["nginx.service", "ssh@host.service", "user@1000.service", "systemd-resolved.service", "dev-sda1.mount"].iter() {
        assert!(valid_unit(ok), "{ok}");
    }
    // A cursor that isn't one is never put in a script: the history is used instead.
    let s = journal(None, History::Last(5)).script(Some("x'; reboot; '"), true).unwrap();
    assert!(!s.contains("reboot") && s.ends_with("-n 5"), "{s}");
    assert!(valid_cursor("s=1a2b;i=4189a2;b=285c;m=c87c;t=65f6;x=78"));
    assert!(!valid_cursor("") && !valid_cursor("a b") && !valid_cursor("a;b|c") && !valid_cursor(&"a".repeat(600)));
    // Priorities, history sizes, paths and times.
    assert!(Spec { source: Source::Journal { unit: None, kernel: false, priority: Some(8) }, history: History::Last(1), auto_reconnect: true, redact: true }.check().is_err());
    assert!(journal(None, History::Last(MAX_HISTORY + 1)).check().is_err());
    assert!(journal(None, History::Since(chrono::Utc::now().timestamp() + 3600)).check().is_err());
    assert!(file("relative.log", 1).check().is_err());
    assert!(file("/var/log/a\nb", 1).check().is_err());
    assert!(Spec { history: History::Since(1_700_000_000), ..file("/var/log/a.log", 1) }.check().is_err());
    // A file path is quoted, and a reconnect follows only new lines.
    assert_eq!(file("/var/log/it's.log", 50).script(None, false).unwrap(), "exec tail -n 50 -F -- '/var/log/it'\\''s.log'");
    assert_eq!(file("/var/log/a.log", 50).script(None, true).unwrap(), "exec tail -n 0 -F -- /var/log/a.log");
}

#[test]
fn a_stream_is_described_by_json_the_window_can_send() {
    let spec: Spec = serde_json::from_str(r#"{"source":{"kind":"journal","unit":"ssh.service","priority":3},"history":{"last":100}}"#).unwrap();
    assert!(spec.auto_reconnect && spec.redact, "both default to on");
    assert_eq!(spec.source, Source::Journal { unit: Some("ssh.service".into()), kernel: false, priority: Some(3) });
    let spec: Spec = serde_json::from_str(r#"{"source":{"kind":"file","path":"/var/log/syslog"},"history":{"since":1700000000},"redact":false}"#).unwrap();
    assert!(!spec.redact && spec.check().is_err());
}

// -- reading entries ---------------------------------------------------------------------------------

const SAMPLE: &str = r#"{"_HOSTNAME":"not-the-real-name","PRIORITY":"3","_SYSTEMD_UNIT":"nginx.service","SYSLOG_IDENTIFIER":"nginx","_PID":"812","MESSAGE":"worker process exited","__CURSOR":"s=a;i=1;b=2;m=3;t=4;x=5","__REALTIME_TIMESTAMP":"1793735427234086","_UID":"0","_COMM":"nginx","SYSLOG_RAW":"<27>raw"}"#;

#[test]
fn a_journal_line_becomes_an_entry_with_its_fields() {
    let r = parse_journal_line(SAMPLE).unwrap();
    assert_eq!((r.priority, r.pid, r.ts_us), (Some(3), Some(812), Some(1_793_735_427_234_086)));
    assert_eq!((r.unit.as_deref(), r.ident.as_deref(), r.message.as_str()), (Some("nginx.service"), Some("nginx"), "worker process exited"));
    assert_eq!(r.cursor.as_deref(), Some("s=a;i=1;b=2;m=3;t=4;x=5"));
    assert_eq!(r.journal_host.as_deref(), Some("not-the-real-name"));
    let names: Vec<&str> = r.fields.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"_UID") && names.contains(&"_COMM"), "{names:?}");
    assert!(!names.iter().any(|n| ["MESSAGE", "__CURSOR", "SYSLOG_RAW"].contains(n)), "{names:?}");
}

#[test]
fn every_priority_maps_and_a_bad_one_is_none() {
    for p in 0..=7u8 {
        let r = parse_journal_line(&format!(r#"{{"PRIORITY":"{p}","MESSAGE":"x"}}"#)).unwrap();
        assert_eq!(r.priority, Some(p));
    }
    assert_eq!(parse_journal_line(r#"{"PRIORITY":"9","MESSAGE":"x"}"#).unwrap().priority, None);
    assert_eq!(parse_journal_line(r#"{"PRIORITY":"high","MESSAGE":"x"}"#).unwrap().priority, None);
    assert_eq!(parse_journal_line(r#"{"MESSAGE":"x"}"#).unwrap().priority, None);
    assert_eq!(["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"].to_vec(), (0..8).map(priority_name).collect::<Vec<_>>());
}

#[test]
fn odd_journal_values_are_read_without_failing() {
    // A message that isn't text is an array of bytes; a field written twice is a list; a missing message is empty.
    let bytes = parse_journal_line(r#"{"MESSAGE":[104,105,255]}"#).unwrap();
    assert_eq!(bytes.message, "hi\u{fffd}");
    let twice = parse_journal_line(r#"{"MESSAGE":"m","_SYSTEMD_UNIT":["a.service","b.service"]}"#).unwrap();
    assert_eq!(twice.unit.as_deref(), Some("a.service, b.service"));
    assert_eq!(parse_journal_line(r#"{"PRIORITY":"6"}"#).unwrap().message, "");
    // Not a log line: a hint, an error, half a line, a number.
    for junk in ["Hint: You are currently not seeing messages from other users", "-- No entries --", r#"{"MESSAGE":"cut off"#, "", "42", "[1,2]"] {
        assert!(parse_journal_line(junk).is_none(), "{junk}");
    }
    // A huge message is cut and says so; a cursor with odd characters is dropped rather than trusted.
    let long = format!(r#"{{"MESSAGE":"{}","__CURSOR":"a b;c"}}"#, "x".repeat(MAX_MESSAGE + 10));
    let r = parse_journal_line(&long).unwrap();
    assert!(r.truncated && r.message.len() <= MAX_MESSAGE + 4 && r.cursor.is_none());
}

#[test]
fn an_entry_belongs_to_the_host_it_was_read_from_not_the_one_the_journal_names() {
    let host = Uuid::new_v4();
    let e = finish(parse_journal_line(SAMPLE).unwrap(), host, 7, &mut Redactor::new(true), 0);
    assert_eq!((e.host_id, e.seq), (host, 7));
    assert_eq!(e.journal_host.as_deref(), Some("not-the-real-name"));
    // On the wire the identity is the host id, and the journal's name is a separate, plainly named field.
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(json["host_id"], host.to_string());
    assert_eq!(json["journal_host"], "not-the-real-name");
    // Without a journal time, the time it arrived is used.
    assert_eq!(finish(parse_journal_line(r#"{"MESSAGE":"m"}"#).unwrap(), host, 1, &mut Redactor::new(true), 42).ts_us, 42);
}

// -- secrets -----------------------------------------------------------------------------------------

#[test]
fn secrets_are_hidden_in_messages_and_fields_unless_turned_off() {
    let host = Uuid::new_v4();
    let line = r#"{"MESSAGE":"login with password=hunter2 ok","_CMDLINE":"curl -H 'Authorization: Bearer abcdef123' https://u:pw@x.test","DB_PASSWORD":"s3cret","PRIORITY":"6"}"#;
    let e = finish(parse_journal_line(line).unwrap(), host, 1, &mut Redactor::new(true), 0);
    let all = format!("{} {:?}", e.message, e.fields);
    for leaked in ["hunter2", "abcdef123", "s3cret", ":pw@"] {
        assert!(!all.contains(leaked), "{leaked} leaked: {all}");
    }
    assert!(e.message.contains("login with") && e.message.contains("ok"));
    let raw = finish(parse_journal_line(line).unwrap(), host, 1, &mut Redactor::new(false), 0);
    assert!(raw.message.contains("hunter2"));
}

#[test]
fn a_private_key_split_over_entries_is_hidden_to_its_end() {
    let mut r = Redactor::new(true);
    assert_eq!(r.message("-----BEGIN OPENSSH PRIVATE KEY-----"), "[private key hidden]");
    assert_eq!(r.message("b3BlbnNzaC1rZXktdjEAAAAA"), "[private key hidden]");
    assert_eq!(r.message("-----END OPENSSH PRIVATE KEY-----"), "[private key hidden]");
    assert_eq!(r.message("back to normal"), "back to normal");
}

#[test]
fn colour_and_link_escapes_are_removed_so_a_coloured_json_line_still_parses() {
    let coloured = "{\x1b[0;32m\"MESSAGE\"\x1b[0m:\x1b[0;32m\"hi\"\x1b[0m,\"PRIORITY\":\"4\"}";
    let plain = strip_ansi(coloured);
    assert_eq!(plain, r#"{"MESSAGE":"hi","PRIORITY":"4"}"#);
    assert_eq!(parse_journal_line(&plain).unwrap().priority, Some(4));
    assert_eq!(strip_ansi("\x1b]8;;http://x\x07link\x1b]8;;\x07 \x1b[1;31mred\x1b[m \x1b(Bok"), "link red ok");
    assert_eq!(strip_ansi("no escapes é"), "no escapes é");
    assert_eq!(strip_ansi("cut \x1b[31"), "cut ");
}

// -- the stream's parts -----------------------------------------------------------------------------------

#[test]
fn lines_are_cut_whole_and_a_line_too_long_is_skipped_and_counted() {
    let mut l = Lines::default();
    assert_eq!(l.push("one\r\ntw"), vec!["one"]);
    assert_eq!(l.push("o\nthree"), vec!["two"]);
    assert_eq!(l.push("\n\n"), vec!["three", ""]);
    let mut l = Lines::default();
    assert!(l.push(&"x".repeat(MAX_LINE + 5)).is_empty());
    assert!(l.push(&"y".repeat(100)).is_empty(), "still the same long line");
    assert_eq!(l.push("\nnext\n"), vec!["next"]);
    assert_eq!(l.overlong, 1);
}

#[test]
fn an_entry_seen_twice_is_refused_and_the_memory_is_bounded() {
    let mut s = Seen::default();
    assert!(s.new_cursor("a") && !s.new_cursor("a") && s.new_cursor("b"));
    for i in 0..(SEEN + 50) {
        s.new_cursor(&format!("c{i}"));
    }
    assert!(s.order.len() <= SEEN && s.set.len() <= SEEN);
    assert!(s.new_cursor("a"), "forgotten after a long while");
}

#[test]
fn a_noisy_host_is_limited_per_second() {
    let mut l = Limiter::new(3);
    let t = Instant::now();
    let allowed = (0..10).filter(|_| l.allow(t)).count();
    assert_eq!(allowed, 3);
    assert!(l.allow(t + Duration::from_millis(1100)), "a new second");
}

#[test]
fn waits_between_tries_double_up_to_a_limit() {
    let p = Policy { base: Duration::from_secs(2), max_wait: Duration::from_secs(60), attempts: 10, connecting: 6 };
    let waits: Vec<u64> = (1..=8).map(|n| p.wait(n).as_secs()).collect();
    assert_eq!(waits, vec![2, 4, 8, 16, 32, 60, 60, 60]);
    assert_eq!(p.wait(u32::MAX).as_secs(), 60);
}

// -- over a real connection ------------------------------------------------------------------------------------

struct Collect(std_mpsc::Sender<OpsEvent>);
impl OpsSink for Collect {
    fn event(&self, e: OpsEvent) {
        let _ = self.0.send(e);
    }
}

/// Everything a stream has said, read as it arrives.
struct Watch {
    rx: std_mpsc::Receiver<OpsEvent>,
    seen: Vec<OpsEvent>,
}

impl Watch {
    fn new(rx: std_mpsc::Receiver<OpsEvent>) -> Self {
        Self { rx, seen: Vec::new() }
    }

    /// Wait until `done` is true of what has been said so far.
    async fn until(&mut self, what: &str, done: impl Fn(&[OpsEvent]) -> bool) {
        let end = Instant::now() + Duration::from_secs(25);
        loop {
            while let Ok(e) = self.rx.try_recv() {
                self.seen.push(e);
            }
            if done(&self.seen) {
                return;
            }
            assert!(Instant::now() < end, "timed out waiting for {what}; saw {:#?}", self.seen.iter().filter(|e| !matches!(e, OpsEvent::Batch { .. })).collect::<Vec<_>>());
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}

fn messages(events: &[OpsEvent], host: Uuid) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            OpsEvent::Batch { host_id, entries, .. } if *host_id == host => Some(entries.iter().map(|e| e.message.clone())),
            _ => None,
        })
        .flatten()
        .collect()
}

fn state_of(events: &[OpsEvent], host: Uuid) -> Option<HostState> {
    events.iter().rev().find_map(|e| match e {
        OpsEvent::State { host_id, state, .. } if *host_id == host => Some(*state),
        _ => None,
    })
}

fn has_state(events: &[OpsEvent], host: Uuid, want: HostState) -> bool {
    events.iter().any(|e| matches!(e, OpsEvent::State { host_id, state, .. } if *host_id == host && *state == want))
}

fn job(host_id: Uuid, t: &Target) -> HostJob {
    HostJob { host_id, label: "h".into(), target: Ok(t.clone()) }
}

fn fast() -> Policy {
    Policy { base: Duration::from_millis(50), max_wait: Duration::from_millis(200), attempts: 5, connecting: 4 }
}

fn append(path: &std::path::Path, text: &str) {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn process_running(pattern: &str) -> bool {
    std::process::Command::new("pgrep").args(["-f", pattern]).output().map(|o| !o.stdout.is_empty()).unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_hosts_stream_with_their_own_identity_and_stopping_ends_the_remote_command() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let log = dir.path().join(format!("opslogs-{}.log", Uuid::new_v4().simple()));
    append(&log, "old line\n");
    let monitors = Arc::new(MonitorManager::new());
    let mgr = OpsLogManager::with_policy(Arc::clone(&monitors), fast());
    let (a, b, broken) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let (tx, rx) = std_mpsc::channel();
    let mut w = Watch::new(rx);
    // Two real hosts, and one that can't be logged in to without a password.
    let jobs = vec![job(a, &t), job(b, &t), HostJob { host_id: broken, label: "x".into(), target: Err("no saved credentials".into()) }];
    mgr.start("s1".into(), file(log.to_str().unwrap(), 10), jobs, Arc::new(Collect(tx))).unwrap();

    w.until("both hosts to stream their history", |ev| messages(ev, a).contains(&"old line".into()) && messages(ev, b).contains(&"old line".into())).await;
    append(&log, "fresh line\n");
    w.until("the new line on both hosts", |ev| messages(ev, a).contains(&"fresh line".into()) && messages(ev, b).contains(&"fresh line".into())).await;
    // The failed host is reported, and the others are not held back by it.
    assert_eq!(state_of(&w.seen, broken), Some(HostState::Failed));
    assert_eq!(state_of(&w.seen, a), Some(HostState::Streaming));
    // Every entry carries its own host, and numbers count up per host.
    let mut last = HashMap::new();
    for e in &w.seen {
        if let OpsEvent::Batch { host_id, entries, .. } = e {
            for en in entries {
                assert_eq!(en.host_id, *host_id);
                assert!(en.seq > last.insert(en.host_id, en.seq).unwrap_or(0));
            }
        }
    }
    assert_eq!(mgr.running(), 3);
    assert!(process_running(log.to_str().unwrap()), "the tail is running on the host");

    assert!(mgr.stop("s1"));
    w.until("every host to report stopped", |ev| [a, b, broken].iter().all(|h| state_of(ev, *h) == Some(HostState::Stopped))).await;
    // Released: no stream left, no connection kept, and the remote command is gone.
    let end = Instant::now() + Duration::from_secs(10);
    while (mgr.stream_count() > 0 || monitors.open_count() > 0 || monitors.stream_count() > 0 || process_running(log.to_str().unwrap())) && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!((mgr.stream_count(), monitors.open_count(), monitors.stream_count()), (0, 0, 0));
    assert!(!process_running(log.to_str().unwrap()), "the remote tail was left running");
    assert!(!mgr.stop("s1"), "already stopped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_connection_is_retried_and_only_that_host_is_affected() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let log = dir.path().join(format!("opslogs-{}.log", Uuid::new_v4().simple()));
    let monitors = Arc::new(MonitorManager::new());
    let mgr = OpsLogManager::with_policy(Arc::clone(&monitors), fast());
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let (tx, rx) = std_mpsc::channel();
    let mut w = Watch::new(rx);
    append(&log, "before\n");
    mgr.start("s2".into(), file(log.to_str().unwrap(), 10), vec![job(a, &t), job(b, &t)], Arc::new(Collect(tx))).unwrap();
    w.until("both streaming", |ev| state_of(ev, a) == Some(HostState::Streaming) && state_of(ev, b) == Some(HostState::Streaming)).await;
    w.until("before on both", |ev| messages(ev, a).contains(&"before".into()) && messages(ev, b).contains(&"before".into())).await;

    // Cut host a's connection from underneath it.
    let session = mgr.session_of("s2", a).expect("a has a connection");
    monitors.close(session).await;
    w.until("a to reconnect", |ev| has_state(ev, a, HostState::Reconnecting) && state_of(ev, a) == Some(HostState::Streaming)).await;
    assert!(w.seen.iter().any(|e| matches!(e, OpsEvent::Gap { host_id, .. } if *host_id == a)), "a file can't be resumed: a gap is reported");
    assert!(!has_state(&w.seen, b, HostState::Reconnecting), "b was never interrupted");
    // The command may still be starting: write until it is heard.
    for n in 0.. {
        append(&log, &format!("after-{n}\n"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        w.until("a pause", |_| true).await;
        if messages(&w.seen, a).iter().any(|m| m.starts_with("after-")) && messages(&w.seen, b).iter().any(|m| m.starts_with("after-")) {
            break;
        }
        assert!(n < 100, "never heard from the file again");
    }
    // Nothing was delivered twice.
    assert_eq!(messages(&w.seen, a).iter().filter(|m| *m == "before").count(), 1);
    assert_eq!(messages(&w.seen, b).iter().filter(|m| *m == "before").count(), 1);
    mgr.stop("s2");
    w.until("stopped", |ev| state_of(ev, a) == Some(HostState::Stopped) && state_of(ev, b) == Some(HostState::Stopped)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_are_bounded_and_a_failed_host_can_be_tried_again_by_hand() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let mut t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    // Nothing listens on this port.
    let good_port = t.port;
    t.port = 1;
    let monitors = Arc::new(MonitorManager::new());
    let mgr = OpsLogManager::with_policy(Arc::clone(&monitors), Policy { attempts: 2, ..fast() });
    let h = Uuid::new_v4();
    let (tx, rx) = std_mpsc::channel();
    let mut w = Watch::new(rx);
    let log = dir.path().join("x.log");
    append(&log, "hello\n");
    mgr.start("s3".into(), file(log.to_str().unwrap(), 5), vec![job(h, &t)], Arc::new(Collect(tx))).unwrap();
    w.until("it to give up", |ev| state_of(ev, h) == Some(HostState::Failed)).await;
    let tries = w.seen.iter().filter(|e| matches!(e, OpsEvent::State { state: HostState::Reconnecting, retry_in_secs: Some(_), .. })).count();
    assert_eq!(tries, 2, "two waits, then it gives up");
    let msg = w.seen.iter().rev().find_map(|e| match e { OpsEvent::State { state: HostState::Failed, message, .. } => message.clone(), _ => None }).unwrap();
    assert!(msg.contains("Gave up after 2"), "{msg}");
    // Stopping while failed still ends the host.
    mgr.stop("s3");
    w.until("stopped", |ev| state_of(ev, h) == Some(HostState::Stopped)).await;
    let _ = good_port;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hosts_of_a_running_stream_can_be_changed_without_restarting_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let log = dir.path().join(format!("opslogs-{}.log", Uuid::new_v4().simple()));
    let mgr = OpsLogManager::with_policy(Arc::new(MonitorManager::new()), fast());
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let (tx, rx) = std_mpsc::channel();
    let mut w = Watch::new(rx);
    mgr.start("s4".into(), file(log.to_str().unwrap(), 0), vec![job(a, &t)], Arc::new(Collect(tx))).unwrap();
    w.until("a streaming", |ev| state_of(ev, a) == Some(HostState::Streaming)).await;
    mgr.add_hosts("s4", vec![job(b, &t)]).unwrap();
    w.until("b streaming", |ev| state_of(ev, b) == Some(HostState::Streaming)).await;
    mgr.remove_hosts("s4", &[a]);
    w.until("a stopped", |ev| state_of(ev, a) == Some(HostState::Stopped)).await;
    for n in 0.. {
        append(&log, "only b now\n");
        tokio::time::sleep(Duration::from_millis(150)).await;
        w.until("a pause", |_| true).await;
        if messages(&w.seen, b).contains(&"only b now".into()) {
            break;
        }
        assert!(n < 100, "b never heard the file");
    }
    assert!(!messages(&w.seen, a).contains(&"only b now".into()));
    assert!(mgr.add_hosts("nope", vec![]).is_err());
    mgr.stop_all();
    w.until("b stopped", |ev| state_of(ev, b) == Some(HostState::Stopped)).await;
}

/// The journal is read by whoever the SSH user is; skip where it can't be read.
fn journal_readable() -> bool {
    std::process::Command::new("journalctl").args(["-n", "1", "-o", "json", "--no-pager"]).output().map(|o| o.status.success() && !o.stdout.is_empty()).unwrap_or(false)
        && std::process::Command::new("logger").arg("--version").output().is_ok()
}

fn logger(tag: &str, priority: &str, msg: &str) {
    assert!(std::process::Command::new("logger").args(["-t", tag, "-p", priority, msg]).status().unwrap().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_journal_is_followed_with_priorities_then_resumed_by_cursor_and_read_around_an_entry() {
    let dir = tempfile::TempDir::new().unwrap();
    let (Some(sshd), true) = (spawn_sshd(dir.path()), journal_readable()) else {
        eprintln!("skipping: no usable sshd or no readable journal on this machine");
        return;
    };
    let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
    let tag = format!("sshvault-t{}", &Uuid::new_v4().simple().to_string()[..10]);
    // Messages no other run could have written.
    let (one, two, three, four) = (format!("{tag} one"), format!("{tag} two"), format!("{tag} three"), format!("{tag} four"));
    let monitors = Arc::new(MonitorManager::new());
    let mgr = OpsLogManager::with_policy(Arc::clone(&monitors), fast());
    let h = Uuid::new_v4();
    let (tx, rx) = std_mpsc::channel();
    let mut w = Watch::new(rx);
    // From a few seconds ago, so entries written while the command is still starting are not missed.
    mgr.start("s5".into(), journal(None, History::Since(chrono::Utc::now().timestamp() - 3)), vec![job(h, &t)], Arc::new(Collect(tx))).unwrap();
    logger(&tag, "user.err", &one);
    logger(&tag, "user.warning", &two);
    logger(&tag, "user.info", &three);
    w.until("three entries", |ev| [&one, &two, &three].iter().all(|m| messages(ev, h).contains(m))).await;
    let mine = |seen: &[OpsEvent]| -> Vec<Entry> {
        seen.iter().filter_map(|e| match e { OpsEvent::Batch { entries, .. } => Some(entries.clone()), _ => None }).flatten().filter(|e| e.ident.as_deref() == Some(tag.as_str())).collect()
    };
    let got = mine(&w.seen);
    assert_eq!(got.iter().map(|e| (e.message.clone(), e.priority)).collect::<Vec<_>>(), vec![(one.clone(), Some(3)), (two.clone(), Some(4)), (three.clone(), Some(6))]);
    assert!(got.iter().all(|e| e.host_id == h && e.cursor.is_some() && e.pid.is_some() && e.ts_us > 0));

    // Lose the connection, write while it is down, and find that exactly the missing entries arrive.
    let session = mgr.session_of("s5", h).unwrap();
    monitors.close(session).await;
    logger(&tag, "user.notice", &four);
    w.until("it to come back with the fourth", |ev| messages(ev, h).contains(&four) && state_of(ev, h) == Some(HostState::Streaming)).await;
    let all = mine(&w.seen);
    for m in [&one, &two, &three, &four] {
        let n = all.iter().filter(|e| &e.message == m).count();
        assert_eq!(n, 1, "{m} delivered {n} times");
    }
    assert!(!w.seen.iter().any(|e| matches!(e, OpsEvent::Gap { .. })), "the journal could resume, so no gap is claimed");

    // Context around the third: the entries on either side, from the journal itself.
    let third = all.iter().find(|e| e.message == three).unwrap();
    let ctx = mgr.context("s5", h, None, Around { cursor: third.cursor.as_deref().unwrap(), before: 2, after: 1, unit: None, redact: true }).await.unwrap();
    let before: Vec<&str> = ctx.before.iter().map(|e| e.message.as_str()).collect();
    assert!(before.contains(&two.as_str()) && !before.contains(&three.as_str()), "{before:?}");
    assert_eq!(ctx.after.first().map(|e| e.message.as_str()), Some(four.as_str()));
    assert!(mgr.context("s5", h, None, Around { cursor: "x; reboot", before: 1, after: 1, unit: None, redact: true }).await.is_err());
    mgr.stop("s5");
    w.until("stopped", |ev| state_of(ev, h) == Some(HostState::Stopped)).await;
}

#[test]
fn a_replay_of_old_entries_after_a_resume_is_recognised() {
    let now = 1_793_735_427_000_000i64;
    assert!(!is_replay(Some(now), 0), "nothing shown yet");
    assert!(!is_replay(Some(now + 5), now) && !is_replay(Some(now - 1_000_000), now), "a little out of order is normal");
    assert!(is_replay(Some(now - 3_600_000_000), now));
    assert!(!is_replay(None, now));
}
