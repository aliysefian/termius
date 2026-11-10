//! Tests for the tmux engine. Most run the real scripts against a real tmux server that each test starts on a socket of its
//! own, in its own temporary folder. They never touch the user's tmux: `TMUX` is removed from the environment of every
//! child, the default socket folder (`TMUX_TMPDIR`) points into the test's folder, and every `kill-server` names its socket.
//! A machine without tmux (or without Unix) skips them.

// Windows builds the pure tests only; what the Unix-only tests alone use would be unused there.
#![cfg_attr(not(unix), allow(unused_imports, dead_code, unused_macros))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::manager::{Config, SearchTarget, TmuxError, TmuxManager, TmuxSink};
use super::model::*;
use super::ops::{valid_session_name, Op, ResizeDir, SplitDir};
use super::parse::{classify_error, parse_probe, records, sections, ParseError};
use super::script::{self, CaptureOpts};
use super::search::{find, Matcher, Query};
use crate::hostmetrics::manager::{BoxFuture, HostExec};

// -- pure pieces -------------------------------------------------------------------------------------------------------

#[test]
fn versions_parse_in_every_form_tmux_prints() {
    let v = |s: &str| Version::parse(s).unwrap();
    assert_eq!((v("tmux 3.4").major, v("tmux 3.4").minor), (3, 4));
    assert_eq!((v("tmux 3.3a").major, v("tmux 3.3a").minor), (3, 3));
    assert!(v("tmux next-3.5").dev && v("tmux next-3.5").at_least(3, 5));
    assert!(v("tmux master").at_least(99, 0));
    assert!(v("tmux openbsd-7.4").supported());
    assert!(!v("tmux 1.8").supported() && !v("tmux 1.9a").supported());
    assert!(v("tmux 2.0").supported() && v("tmux 2.9a").supported());
    assert!(v("tmux 10.1").at_least(3, 9));
    assert_eq!(Version::parse("zsh: command not found: tmux"), None);
    assert_eq!(Version::parse("tmux"), None);
    assert_eq!(Version::parse(""), None);
}

#[test]
fn ids_have_exactly_tmuxs_forms() {
    assert!(valid_session_id("$0") && valid_session_id("$123") && valid_window_id("@7") && valid_pane_id("%42"));
    for bad in ["", "$", "%", "0", "$-1", "$1 ", "$1;ls", "%1'", "$$1", "@1", " $1", "$12345678901234"] {
        assert!(!valid_session_id(bad), "{bad:?}");
    }
    assert!(!valid_pane_id("$1") && !valid_window_id("%1") && !valid_pane_id("backend"));
}

#[test]
fn text_from_the_host_loses_its_control_and_spoofing_characters() {
    assert_eq!(clean("a\x1b[31mb\x07c\nd\te"), "a\u{FFFD}[31mb\u{FFFD}c\u{FFFD}d\u{FFFD}e");
    // Right-to-left override: "gnp.exe" shown as "exe.png".
    assert!(!clean("report\u{202E}gnp.exe").contains('\u{202E}'));
    assert_eq!(clean("日本語 ünï"), "日本語 ünï");
}

#[test]
fn socket_names_and_paths_are_checked() {
    assert!(Socket::Default.validate().is_ok() && Socket::Name("work-1.x".into()).validate().is_ok());
    for bad in ["", "-x", "a b", "a/b", "a;b", "$(x)", &"x".repeat(65)] {
        assert!(Socket::Name(bad.into()).validate().is_err(), "{bad:?}");
    }
    assert!(Socket::Path("/run/user/1000/tmux-x".into()).validate().is_ok());
    assert!(Socket::Path("relative/x".into()).validate().is_err() && Socket::Path("/a\nb".into()).validate().is_err() && Socket::Path("/a/{{host}}".into()).validate().is_err());
    assert_eq!(Socket::Name("w".into()).key(), "L:w");
}

#[test]
fn quoting_survives_every_shell_metacharacter() {
    assert_eq!(script::quote("a b"), "'a b'");
    assert_eq!(script::quote("it's"), "'it'\\''s'");
    // Executed by a real sh, the quoted text is the original text, byte for byte.
    for s in ["plain", "a'b", "$(touch /tmp/pwn)", "`id`", "a\"b", "x;y|z&w", "\\n", "new\nline", "$3", "%1", "ünï 日本", "--", "-x", "''", "'"] {
        let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s {}", script::quote(s))).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), s, "{s:?}");
    }
}

#[test]
fn records_split_by_nonce_whatever_the_fields_contain() {
    let n = "abc123";
    let d = script::delim(n);
    let rec = |fields: &[&str]| format!("{d}{}{d}", fields.join(&d));
    // A field with a newline, a tab, a pipe, the look of a section header, and the look of a delimiter without the nonce.
    let evil = format!("x\n@@{n}x:sessions\n@@|y\t|z");
    let body = format!("{}\n{}\n", rec(&["$0", &evil, "1"]), rec(&["$1", "plain", "0"]));
    let r = records(&body, n, 3).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0][1], evil);
    assert_eq!(r[1], vec!["$1", "plain", "0"]);
    assert_eq!(records("", n, 3).unwrap(), Vec::<Vec<String>>::new());
    // Wrong shapes are errors, never shorter lists.
    assert!(records(&format!("{}\n", rec(&["$0", "a"])), n, 3).is_err());
    assert!(records(&format!("{}\n", rec(&["$0", "a", "b", "c"])), n, 3).is_err());
    assert!(records("garbage\n", n, 3).is_err());
}

#[test]
fn a_run_without_its_end_marker_is_an_error_not_a_short_list() {
    let n = "f00d";
    assert_eq!(sections(&format!("@@{n}:os\nLinux\n"), n).unwrap_err(), ParseError::Truncated);
    assert!(sections(&format!("@@{n}:os\nLinux\n@@{n}:end\n"), n).is_ok());
    // A marker with another run's nonce is just text.
    let text = format!("@@{n}:os\nx\n@@other:end\n@@{n}:end\n");
    let s = sections(&text, n).unwrap();
    assert_eq!(s["os"], "x\n@@other:end");
}

#[test]
fn tmux_errors_become_states() {
    let v = Version::parse("tmux 3.4");
    assert_eq!(classify_error("no server running on /tmp/tmux-1000/default", v.as_ref()), ServerState::NoServer);
    assert_eq!(classify_error("error connecting to /tmp/tmux-1000/default (No such file or directory)", v.as_ref()), ServerState::NoServer);
    assert!(matches!(classify_error("error connecting to /tmp/x (Permission denied)", v.as_ref()), ServerState::PermissionDenied { .. }));
    assert!(matches!(classify_error("something odd", v.as_ref()), ServerState::Failed { .. }));
}

fn probe_text(n: &str, parts: &[(&str, &str)]) -> String {
    let mut s = String::new();
    for (name, body) in parts {
        s.push_str(&format!("@@{n}:{name}\n{body}\n"));
    }
    s.push_str(&format!("@@{n}:end\n"));
    s
}

#[test]
fn a_host_without_tmux_is_not_installed_and_says_which_system() {
    let n = "aa";
    let out = probe_text(n, &[("os", "Linux\nubuntu|debian|Ubuntu 24.04"), ("missing", "")]);
    let p = parse_probe(&out, n, &Socket::Default).unwrap();
    assert_eq!(p.state, ServerState::NotInstalled { os: "Linux|ubuntu|debian|Ubuntu 24.04".into() });
}

#[test]
fn no_server_and_old_tmux_are_told_apart() {
    let n = "bb";
    let none = probe_text(n, &[("os", "Linux"), ("version", "tmux 3.4"), ("sockets", "work"), ("sessions-rc", "1"), ("sessions", "no server running on /tmp/tmux-1/default")]);
    let p = parse_probe(&none, n, &Socket::Default).unwrap();
    assert_eq!(p.state, ServerState::NoServer);
    assert_eq!(p.other_sockets, vec!["work".to_string()]);
    let old = probe_text(n, &[("os", "Linux"), ("version", "tmux 1.8"), ("sessions-rc", "0"), ("sessions", "")]);
    assert!(matches!(parse_probe(&old, n, &Socket::Default).unwrap().state, ServerState::Unsupported { .. }));
}

#[test]
fn search_finds_text_literally_or_by_pattern_and_bounds_what_it_returns() {
    let text = "alpha\nBeta ERROR one\nbeta error two\n(x) [y]\nlast";
    let m = |t: &str, cs: bool, re: bool| Matcher::new(&Query { text: t.into(), case_sensitive: cs, regex: re }).unwrap();
    assert_eq!(find(&m("error", false, false), text, 10, 0).len(), 2);
    assert_eq!(find(&m("error", true, false), text, 10, 0).len(), 1);
    assert_eq!(find(&m("ERROR", true, false), text, 10, 0)[0].line, 2);
    // Literal text is not a pattern: "(x) [y]" matches itself.
    assert_eq!(find(&m("(x) [y]", false, false), text, 10, 0).len(), 1);
    let h = find(&m(r"e\w+ (one|two)", false, true), text, 10, 1);
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].before, vec!["alpha".to_string()]);
    assert_eq!(h[0].after, vec!["beta error two".to_string()]);
    assert_eq!(find(&m("a", false, false), text, 2, 0).len(), 2);
    assert!(Matcher::new(&Query { text: "(".into(), case_sensitive: false, regex: true }).is_err());
    assert!(Matcher::new(&Query { text: "".into(), case_sensitive: false, regex: false }).is_err());
    // A pattern that would be catastrophic in a backtracking engine finishes at once.
    let long = "a".repeat(5000);
    let t = std::time::Instant::now();
    let _ = find(&m(r"(a+)+$", false, true), &format!("{long}!"), 10, 0);
    assert!(t.elapsed() < Duration::from_secs(2));
    // Spans are character offsets, also past multi-byte characters.
    let h = find(&m("日本", false, false), "ünï 日本 x", 5, 0);
    assert_eq!(h[0].spans, vec![(4, 6)]);
}

// -- operations: validation and the command that results -----------------------------------------------------------

#[test]
fn operations_are_refused_before_any_command_when_a_value_is_wrong() {
    assert!(valid_session_name("backend").is_ok() && valid_session_name("日本 語").is_ok());
    for bad in ["", " x", "x ", "a.b", "a:b", "a\nb", "a\x1bb", "a\\b", &"x".repeat(101)] {
        assert!(valid_session_name(bad).is_err(), "{bad:?}");
    }
    let bad: Vec<Op> = vec![
        Op::KillSession { session: "backend".into() },
        Op::KillSession { session: "$1; rm -rf /".into() },
        Op::KillWindow { window: "$1".into() },
        Op::SplitPane { pane: "%1".into(), dir: SplitDir::Right, cwd: Some("relative".into()), command: None },
        Op::SplitPane { pane: "%1".into(), dir: SplitDir::Right, cwd: None, command: Some("ls\nrm x".into()) },
        Op::ResizePane { pane: "%1".into(), dir: ResizeDir::Up, amount: 0 },
        Op::NewSession { name: Some("a.b".into()), cwd: None, command: None },
        Op::RenameWindow { window: "@1".into(), name: "".into() },
    ];
    for op in bad {
        assert!(op.validate().is_err(), "{op:?}");
    }
}

#[test]
fn split_right_is_h_and_split_down_is_v_and_every_target_is_quoted() {
    let s = Socket::Default;
    let right = Op::SplitPane { pane: "%7".into(), dir: SplitDir::Right, cwd: None, command: None }.command(&s);
    let down = Op::SplitPane { pane: "%7".into(), dir: SplitDir::Down, cwd: None, command: None }.command(&s);
    assert!(right.contains("split-window -h ") && right.contains("-t '%7'"), "{right}");
    assert!(down.contains("split-window -v ") && down.contains("-t '%7'"), "{down}");
    let kill = Op::KillSession { session: "$3".into() }.command(&Socket::Name("w".into()));
    assert_eq!(kill, "tmux -u -L 'w' kill-session -t '$3'");
    // A name that looks like an option or a shell command is one quoted argument after `--`.
    let rename = Op::RenameSession { session: "$3".into(), name: "-t $(id) 'x'".into() }.command(&s);
    assert!(rename.contains("-- '-t $(id) '\\''x'\\'''"), "{rename}");
}

// -- the manager, against a scripted host --------------------------------------------------------------------------

/// A host that answers from a list of canned replies and counts the calls.
struct Scripted {
    replies: Mutex<Vec<Result<String, String>>>,
    calls: AtomicUsize,
    delay: Duration,
    nonces: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(replies: Vec<Result<String, String>>, delay: Duration) -> Arc<Self> {
        Arc::new(Self { replies: Mutex::new(replies), calls: AtomicUsize::new(0), delay, nonces: Mutex::new(vec![]) })
    }
}

impl HostExec for Scripted {
    fn exec<'a>(&'a self, _host: Uuid, script: &'a str, _limit: Duration) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            // The nonce this run was given is in the script text.
            let n = script.split("@@").nth(1).and_then(|s| s.split(':').next()).unwrap_or("").to_string();
            self.nonces.lock().unwrap().push(n.clone());
            let mut r = self.replies.lock().unwrap();
            let next = if r.len() > 1 { r.remove(0) } else { r[0].clone() };
            next.map(|t| t.replace("@@NONCE", &format!("@@{n}")))
        })
    }
}

fn ready_text() -> String {
    // One session, one window, one pane, written the way the script prints it, with the nonce filled in by Scripted.
    let d = "@@NONCE|";
    let s = format!("{d}$0{d}main{d}1{d}1700000000{d}1700000100{d}{d}4242{d}1699999999{d}/tmp/tmux-1/default{d}off{d}");
    let p = format!("{d}$0{d}@0{d}0{d}bash{d}1{d}layout{d}0{d}0{d}0{d}%0{d}0{d}1{d}title{d}bash{d}/home{d}4243{d}80{d}24{d}0{d}0{d}");
    format!("@@NONCE:os\nLinux\n@@NONCE:version\ntmux 3.4\n@@NONCE:sockets\n@@NONCE:sessions-rc\n0\n@@NONCE:sessions\n{s}\n@@NONCE:panes-rc\n0\n@@NONCE:panes\n{p}\n@@NONCE:end\n")
}

fn host() -> Uuid {
    Uuid::from_u128(7)
}

#[tokio::test(start_paused = true)]
async fn ten_views_asking_at_once_cause_one_reading() {
    let exec = Scripted::new(vec![Ok(ready_text())], Duration::from_millis(300));
    let m = Arc::new(TmuxManager::new(Arc::clone(&exec), Config::default(), None));
    let asks: Vec<_> = (0..10)
        .map(|_| {
            let m = Arc::clone(&m);
            tokio::spawn(async move { m.snapshot(host(), &Socket::Default, Duration::from_secs(2)).await })
        })
        .collect();
    let mut gens = Vec::new();
    for a in asks {
        gens.push(a.await.unwrap().unwrap().generation);
    }
    assert_eq!(exec.calls.load(Ordering::SeqCst), 1);
    assert!(gens.iter().all(|g| *g == 1));
    // Still fresh a second later; stale after the age passes.
    tokio::time::advance(Duration::from_secs(1)).await;
    m.snapshot(host(), &Socket::Default, Duration::from_secs(2)).await.unwrap();
    assert_eq!(exec.calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(2)).await;
    assert_eq!(m.snapshot(host(), &Socket::Default, Duration::from_secs(2)).await.unwrap().generation, 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_reading_keeps_the_last_good_list_marked_stale_and_recovers() {
    let exec = Scripted::new(vec![Ok(ready_text()), Err("connection reset".into()), Ok(ready_text())], Duration::from_millis(10));
    let m = Arc::new(TmuxManager::new(Arc::clone(&exec), Config::default(), None));
    let a = m.refresh(host(), &Socket::Default).await.unwrap();
    assert!(a.stale.is_none() && a.sessions.len() == 1);
    let b = m.refresh(host(), &Socket::Default).await.unwrap();
    // The list is still there, with the reason, and a higher generation so views take it.
    assert_eq!(b.sessions, a.sessions);
    assert!(b.stale.as_ref().is_some_and(|s| s.message.contains("connection reset")));
    assert!(b.generation > a.generation);
    // While it is cooling down, asking does not hit the host again.
    let before = exec.calls.load(Ordering::SeqCst);
    let c = m.snapshot(host(), &Socket::Default, Duration::ZERO).await.unwrap();
    assert!(c.stale.is_some());
    assert_eq!(exec.calls.load(Ordering::SeqCst), before);
    let d = m.refresh(host(), &Socket::Default).await.unwrap();
    assert!(d.stale.is_none() && d.generation > b.generation);
}

#[tokio::test(start_paused = true)]
async fn a_host_never_read_reports_the_error_instead_of_an_empty_list() {
    let exec = Scripted::new(vec![Err("no route to host".into())], Duration::from_millis(1));
    let m = Arc::new(TmuxManager::new(exec, Config::default(), None));
    let e = m.snapshot(host(), &Socket::Default, Duration::ZERO).await.unwrap_err();
    assert!(matches!(e, TmuxError::Exec { ref message } if message.contains("no route")), "{e:?}");
}

#[tokio::test(start_paused = true)]
async fn a_damaged_answer_is_an_error_not_a_shorter_list() {
    let cut = ready_text().replace("@@NONCE:end\n", "");
    let exec = Scripted::new(vec![Ok(cut)], Duration::from_millis(1));
    let m = Arc::new(TmuxManager::new(exec, Config::default(), None));
    assert!(matches!(m.snapshot(host(), &Socket::Default, Duration::ZERO).await.unwrap_err(), TmuxError::Unreadable { .. }));
}

struct Count(AtomicUsize);
impl TmuxSink for Count {
    fn changed(&self, _s: &Arc<HostSnapshot>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn the_sink_hears_only_real_changes() {
    let exec = Scripted::new(vec![Ok(ready_text())], Duration::from_millis(1));
    let sink = Arc::new(Count(AtomicUsize::new(0)));
    let m = Arc::new(TmuxManager::new(exec, Config::default(), Some(Arc::clone(&sink) as Arc<dyn TmuxSink>)));
    for _ in 0..5 {
        m.refresh(host(), &Socket::Default).await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    assert_eq!(sink.0.load(Ordering::SeqCst), 1, "five identical readings are one change");
}

#[tokio::test(start_paused = true)]
async fn one_poll_loop_serves_every_watcher_and_ends_with_the_last() {
    let exec = Scripted::new(vec![Ok(ready_text())], Duration::from_millis(1));
    let m = Arc::new(TmuxManager::new(Arc::clone(&exec), Config::default(), None));
    m.watch(host(), Socket::Default, "monitor", Duration::from_secs(3)).unwrap();
    m.watch(host(), Socket::Default, "panel:a", Duration::from_secs(5)).unwrap();
    m.watch(host(), Socket::Default, "panel:b", Duration::from_secs(5)).unwrap();
    assert_eq!(m.watching(), 1, "three views, one loop");
    tokio::time::sleep(Duration::from_secs(10)).await;
    let calls = exec.calls.load(Ordering::SeqCst);
    // At the fastest watcher's pace (3 s) over 10 s: a handful of readings, not three loops' worth.
    assert!((3..=5).contains(&calls), "{calls}");
    m.unwatch("monitor");
    assert_eq!(m.watching(), 1, "the panels still watch");
    m.unwatch("panel:a");
    m.unwatch("panel:b");
    assert_eq!(m.watching(), 0);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let stopped_at = exec.calls.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(exec.calls.load(Ordering::SeqCst), stopped_at, "a hidden view costs nothing");
}

#[tokio::test(start_paused = true)]
async fn locking_the_vault_stops_polling_and_forgets_everything() {
    let exec = Scripted::new(vec![Ok(ready_text())], Duration::from_millis(1));
    let m = Arc::new(TmuxManager::new(Arc::clone(&exec), Config::default(), None));
    m.watch(host(), Socket::Default, "monitor", Duration::from_secs(3)).unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(m.latest(host(), &Socket::Default).is_some());
    m.clear();
    assert!(m.latest(host(), &Socket::Default).is_none() && m.watching() == 0);
    let n = exec.calls.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(exec.calls.load(Ordering::SeqCst), n);
}

#[tokio::test(start_paused = true)]
async fn an_operation_without_a_snapshot_to_stand_on_is_refused() {
    let exec = Scripted::new(vec![Ok(ready_text())], Duration::from_millis(1));
    let m = Arc::new(TmuxManager::new(Arc::clone(&exec), Config::default(), None));
    let e = m.operate(host(), &Socket::Default, &Op::KillSession { session: "$0".into() }).await.unwrap_err();
    assert!(matches!(e, TmuxError::Invalid { .. }), "{e:?}");
    assert_eq!(exec.calls.load(Ordering::SeqCst), 0, "nothing was sent to the host");
    let e = m.operate(host(), &Socket::Default, &Op::KillSession { session: "backend".into() }).await.unwrap_err();
    assert!(matches!(e, TmuxError::Invalid { .. }));
}

// -- a real tmux server --------------------------------------------------------------------------------------------

#[cfg(unix)]
mod real {
    use super::*;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    pub fn have_tmux() -> bool {
        Command::new("tmux").arg("-V").output().is_ok_and(|o| o.status.success())
    }

    /// Runs the manager's scripts on this machine, with tmux confined to a folder of the test's own.
    pub struct Local {
        pub dir: tempfile::TempDir,
    }

    impl Local {
        pub fn new() -> Arc<Self> {
            Arc::new(Self { dir: tempfile::TempDir::new().unwrap() })
        }
        pub fn sock(&self) -> PathBuf {
            self.dir.path().join("sock")
        }
        pub fn socket(&self) -> Socket {
            Socket::Path(self.sock().to_string_lossy().into_owned())
        }
        /// Plain tmux on the test's socket.
        pub fn tmux(&self, args: &[&str]) -> String {
            let out = Command::new("tmux").arg("-S").arg(self.sock()).args(args).env_remove("TMUX").env("TMUX_TMPDIR", self.dir.path()).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim_end().to_string()
        }
        pub fn kill_server(&self) {
            let _ = Command::new("tmux").arg("-S").arg(self.sock()).arg("kill-server").env_remove("TMUX").output();
        }
        pub fn sh(&self, script: &str) -> std::process::Output {
            Command::new("sh").arg("-c").arg(script).env_remove("TMUX").env("TMUX_TMPDIR", self.dir.path()).output().unwrap()
        }
    }

    impl Drop for Local {
        fn drop(&mut self) {
            self.kill_server();
        }
    }

    impl HostExec for Local {
        fn exec<'a>(&'a self, _host: Uuid, script: &'a str, _limit: Duration) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                let out = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(script)
                    .env_remove("TMUX")
                    .env("TMUX_TMPDIR", self.dir.path())
                    .stdin(Stdio::null())
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            })
        }
    }

    pub fn manager(l: &Arc<Local>) -> Arc<TmuxManager<Local>> {
        Arc::new(TmuxManager::new(Arc::clone(l), Config { min_interval: Duration::from_millis(1), ..Config::default() }, None))
    }

    pub fn find_session<'a>(s: &'a HostSnapshot, name: &str) -> &'a Session {
        s.sessions.iter().find(|x| x.name == name).unwrap_or_else(|| panic!("no session {name:?} in {:?}", s.sessions.iter().map(|s| &s.name).collect::<Vec<_>>()))
    }

    pub fn wait_for(mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..100 {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

macro_rules! need_tmux {
    () => {
        if !real::have_tmux() {
            eprintln!("skipped: tmux is not installed here");
            return;
        }
    };
}

#[cfg(unix)]
#[tokio::test]
async fn no_server_is_reported_as_none_running_not_as_an_error() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let s = m.snapshot(host(), &l.socket(), Duration::ZERO).await.unwrap();
    assert_eq!(s.state, ServerState::NoServer);
    assert!(s.sessions.is_empty() && s.stale.is_none());
    assert!(s.version.as_ref().is_some_and(|v| v.supported()));
}

#[cfg(unix)]
#[tokio::test]
async fn hostile_names_come_through_exactly_and_forge_nothing() {
    need_tmux!();
    let l = real::Local::new();
    // A window whose name is a newline, a fake section header, a tab and a pipe; a pane title likewise.
    l.tmux(&["new-session", "-d", "-s", "ünï 日本 \"q\" 'q'", "-n", "w1\n@@fake:sessions\n|x\t|y"]);
    l.tmux(&["new-session", "-d", "-s", "second"]);
    l.tmux(&["select-pane", "-t", "second", "-T", "title with|pipe \"q\" ünï"]);
    l.tmux(&["new-window", "-t", "second", "-n", "$(touch pwned)`id`"]);
    let m = real::manager(&l);
    let s = m.refresh(host(), &l.socket()).await.unwrap();
    assert_eq!(s.state, ServerState::Ready);
    assert_eq!(s.sessions.len(), 2, "{:?}", s.sessions);
    assert!(ids_unique(&s.sessions));
    let u = real::find_session(&s, "ünï 日本 \"q\" 'q'");
    assert_eq!(u.windows.len(), 1);
    // Control characters become U+FFFD, everything else is exact.
    assert_eq!(u.windows[0].name, "w1\u{FFFD}@@fake:sessions\u{FFFD}|x\u{FFFD}|y");
    let second = real::find_session(&s, "second");
    assert_eq!(second.windows.len(), 2);
    assert!(second.windows.iter().any(|w| w.name == "$(touch pwned)`id`"));
    assert!(second.windows.iter().flat_map(|w| &w.panes).any(|p| p.title == "title with|pipe \"q\" ünï"));
    assert!(!l.dir.path().join("pwned").exists());
    let srv = s.server.as_ref().unwrap();
    assert!(srv.pid > 0 && srv.socket_path.ends_with("/sock"));
    assert!(u.id.starts_with('$') && u.windows[0].id.starts_with('@') && u.windows[0].panes[0].id.starts_with('%'));
}

#[cfg(unix)]
#[tokio::test]
async fn the_whole_session_lifecycle_works_against_a_real_server_and_is_verified() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    let dir = l.dir.path().to_string_lossy().into_owned();

    // Create. The server does not exist yet: creating starts it.
    let r = m.operate(host(), &sock, &Op::NewSession { name: Some("backend".into()), cwd: Some(dir.clone()), command: None }).await.unwrap();
    let sid = r.created.clone().unwrap();
    assert!(valid_session_id(&sid));
    let s = real::find_session(&r.snapshot, "backend");
    assert_eq!(s.id, sid);
    assert_eq!(s.windows[0].panes[0].path, std::fs::canonicalize(&dir).unwrap().to_string_lossy());

    // A second session with the same name is tmux's refusal, passed on, and nothing else changes.
    let dup = m.operate(host(), &sock, &Op::NewSession { name: Some("backend".into()), cwd: None, command: None }).await.unwrap_err();
    assert!(matches!(dup, TmuxError::Rejected { .. }), "{dup:?}");
    assert_eq!(m.latest(host(), &sock).unwrap().sessions.len(), 1);

    // Rename by id; the old name is gone.
    let r = m.operate(host(), &sock, &Op::RenameSession { session: sid.clone(), name: "api".into() }).await.unwrap();
    assert_eq!(r.snapshot.sessions[0].name, "api");

    // Windows.
    let r = m.operate(host(), &sock, &Op::NewWindow { session: sid.clone(), name: Some("logs".into()), cwd: None, command: Some("sleep 300".into()) }).await.unwrap();
    let wid = r.created.clone().unwrap();
    let w = r.snapshot.sessions[0].windows.iter().find(|w| w.id == wid).unwrap();
    assert_eq!(w.name, "logs");
    assert!(w.active, "a new window is the current one");
    let first = r.snapshot.sessions[0].windows.iter().find(|w| w.id != wid).unwrap().id.clone();
    let r = m.operate(host(), &sock, &Op::SelectWindow { window: first.clone() }).await.unwrap();
    assert!(r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap().active);
    let r = m.operate(host(), &sock, &Op::RenameWindow { window: wid.clone(), name: "journal".into() }).await.unwrap();
    assert!(r.snapshot.sessions[0].windows.iter().any(|w| w.name == "journal"));
    // tmux reads a trailing `;` as a command separator and expands `#{...}` and `#(...)` (which runs a command on the host)
    // in names; names written either way still arrive exactly as typed, and nothing runs.
    for name in ["deploy;", "semi;colon;", ";", "#{pid}", "##", "a#b#", "#(touch pwned)"] {
        let r = m.operate(host(), &sock, &Op::RenameWindow { window: wid.clone(), name: name.into() }).await.unwrap();
        assert!(r.snapshot.sessions[0].windows.iter().any(|w| w.name == name), "{name:?}");
    }
    assert!(!l.dir.path().join("pwned").exists() && !std::path::Path::new("pwned").exists());
    m.operate(host(), &sock, &Op::RenameWindow { window: wid.clone(), name: "journal".into() }).await.unwrap();

    // Panes: Split Right is side by side (same height, narrower), Split Down is stacked (same width, shorter).
    let p0 = r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap().panes[0].clone();
    let r = m.operate(host(), &sock, &Op::SplitPane { pane: p0.id.clone(), dir: SplitDir::Right, cwd: None, command: None }).await.unwrap();
    let right = r.created.clone().unwrap();
    let win = r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap();
    let (a, b) = (win.panes.iter().find(|p| p.id == p0.id).unwrap(), win.panes.iter().find(|p| p.id == right).unwrap());
    assert_eq!(a.height, b.height, "side by side");
    assert!(a.width < p0.width && b.width < p0.width);
    let r = m.operate(host(), &sock, &Op::SplitPane { pane: right.clone(), dir: SplitDir::Down, cwd: None, command: None }).await.unwrap();
    let down = r.created.clone().unwrap();
    let win = r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap();
    let (c, d) = (win.panes.iter().find(|p| p.id == right).unwrap(), win.panes.iter().find(|p| p.id == down).unwrap());
    assert_eq!(c.width, d.width, "stacked");
    assert!(c.height < b.height);
    assert_eq!(win.panes.len(), 3);

    // Selecting a pane that is not the active one acts on *that* pane.
    assert!(win.panes.iter().find(|p| p.id == down).unwrap().active);
    let r = m.operate(host(), &sock, &Op::SelectPane { pane: p0.id.clone() }).await.unwrap();
    let win = r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap();
    assert!(win.panes.iter().find(|p| p.id == p0.id).unwrap().active && !win.panes.iter().find(|p| p.id == down).unwrap().active);
    let r = m.operate(host(), &sock, &Op::SetPaneTitle { pane: right.clone(), title: "web".into() }).await.unwrap();
    assert!(r.snapshot.sessions[0].windows.iter().flat_map(|w| &w.panes).any(|p| p.id == right && p.title == "web"));
    let before = r.snapshot.sessions[0].windows.iter().flat_map(|w| &w.panes).find(|p| p.id == p0.id).unwrap().width;
    let r = m.operate(host(), &sock, &Op::ResizePane { pane: p0.id.clone(), dir: ResizeDir::Right, amount: 5 }).await.unwrap();
    let after = r.snapshot.sessions[0].windows.iter().flat_map(|w| &w.panes).find(|p| p.id == p0.id).unwrap().width;
    assert_eq!(after, before + 5);
    let r = m.operate(host(), &sock, &Op::ZoomPane { pane: p0.id.clone() }).await.unwrap();
    assert!(r.snapshot.sessions[0].windows.iter().find(|w| w.id == first).unwrap().zoomed);

    // Kills.
    let r = m.operate(host(), &sock, &Op::KillPane { pane: down.clone() }).await.unwrap();
    assert!(!r.snapshot.sessions[0].windows.iter().flat_map(|w| &w.panes).any(|p| p.id == down));
    let r = m.operate(host(), &sock, &Op::KillWindow { window: wid.clone() }).await.unwrap();
    assert!(!r.snapshot.sessions[0].windows.iter().any(|w| w.id == wid));
    // Mouse: this session only, and the server's own setting is untouched.
    assert_eq!(m.latest(host(), &sock).unwrap().sessions[0].mouse, Some(false));
    let r = m.operate(host(), &sock, &Op::SetMouse { session: sid.clone(), on: true }).await.unwrap();
    assert_eq!(r.snapshot.sessions[0].mouse, Some(true), "the reading after the change shows it");
    assert_eq!(l.tmux(&["show-options", "-t", &sid, "-v", "mouse"]), "on");
    assert_eq!(l.tmux(&["show-options", "-g", "-v", "mouse"]), "off");
    let r = m.operate(host(), &sock, &Op::KillSession { session: sid.clone() }).await;
    // The last session ends the server, so there is nothing left to read: the kill is confirmed by the server's absence.
    match r {
        Ok(r) => assert!(r.snapshot.sessions.is_empty()),
        Err(e) => panic!("{e:?}"),
    }
    assert_eq!(m.latest(host(), &sock).unwrap().state, ServerState::NoServer);
}

#[cfg(unix)]
#[tokio::test]
async fn ids_from_a_restarted_server_are_refused_instead_of_hitting_another_session() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    l.tmux(&["new-session", "-d", "-s", "precious"]);
    let old = m.refresh(host(), &sock).await.unwrap();
    let old_id = old.sessions[0].id.clone();
    // The server restarts; the new one gives `$0` to a different session.
    l.kill_server();
    assert!(real::wait_for(|| l.tmux(&["list-sessions"]).is_empty()));
    std::thread::sleep(Duration::from_millis(1100)); // start_time has a one-second grain
    l.tmux(&["new-session", "-d", "-s", "somebody-elses"]);
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_id}"]), old_id, "the number was reused");
    let e = m.operate(host(), &sock, &Op::KillSession { session: old_id.clone() }).await.unwrap_err();
    assert_eq!(e, TmuxError::ServerChanged);
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "somebody-elses", "nothing was killed");
    // The refresh that follows shows what is true now.
    assert_eq!(m.latest(host(), &sock).unwrap().sessions[0].name, "somebody-elses");
}

#[cfg(unix)]
#[tokio::test]
async fn panes_are_captured_without_attaching_and_searched_across_hosts_panes() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    l.tmux(&["new-session", "-d", "-s", "one", "-x", "100", "-y", "30"]);
    l.tmux(&["new-session", "-d", "-s", "two", "-x", "100", "-y", "30"]);
    l.tmux(&["send-keys", "-t", "one", "printf 'needle-%s\\n' alpha; for i in $(seq 1 120); do echo line$i; done; printf '\\033[31mred\\033[0m\\n'; echo END$((6*7))", "Enter"]);
    l.tmux(&["send-keys", "-t", "two", "echo Needle-beta; echo nothing here; echo END$((6*7))", "Enter"]);
    assert!(real::wait_for(|| l.tmux(&["capture-pane", "-p", "-t", "two"]).contains("END42") && l.tmux(&["capture-pane", "-p", "-t", "one"]).contains("END42")));
    let snap = m.refresh(host(), &sock).await.unwrap();
    let one = real::find_session(&snap, "one");
    let pane = &one.windows[0].panes[0];

    let screen = m.capture(host(), &sock, &pane.id, CaptureOpts { ansi: false, join: true, scrollback: 0 }, None).await.unwrap();
    assert_eq!((screen.info.width, screen.info.height), (100, 30));
    assert!(screen.text.contains("line120") && !screen.text.contains("needle-alpha"), "only the screen: the early lines scrolled off: {:?}", screen.text);
    let deep = m.capture(host(), &sock, &pane.id, CaptureOpts { ansi: false, join: true, scrollback: 500 }, None).await.unwrap();
    assert!(deep.text.contains("needle-alpha") && deep.info.history > 0);
    let colour = m.capture(host(), &sock, &pane.id, CaptureOpts { ansi: true, join: true, scrollback: 0 }, None).await.unwrap();
    assert!(colour.text.contains("\x1b[31mred"), "{:?}", colour.text);
    assert!(!screen.text.contains('\x1b'));
    // A pane that does not exist is tmux's refusal, not an empty preview.
    assert!(matches!(m.capture(host(), &sock, "%999", CaptureOpts { ansi: false, join: false, scrollback: 0 }, None).await.unwrap_err(), TmuxError::Rejected { .. }));
    assert!(matches!(m.capture(host(), &sock, "bash", CaptureOpts { ansi: false, join: false, scrollback: 0 }, None).await.unwrap_err(), TmuxError::Invalid { .. }));

    let targets: Vec<SearchTarget> = snap
        .sessions
        .iter()
        .flat_map(|s| s.windows.iter().flat_map(move |w| w.panes.iter().map(move |p| SearchTarget { session_id: s.id.clone(), window_id: w.id.clone(), pane_id: p.id.clone() })))
        .collect();
    let q = |t: &str, cs: bool, re: bool| Query { text: t.into(), case_sensitive: cs, regex: re };
    let cancel = CancellationToken::new();
    let r = m.search(host(), &sock, targets.clone(), &q("needle", false, false), 500, &cancel).await.unwrap();
    assert_eq!(r.searched, 2);
    assert_eq!(r.results.len(), 2, "found in both sessions' panes (case-insensitively)");
    let r = m.search(host(), &sock, targets.clone(), &q("needle", true, false), 500, &cancel).await.unwrap();
    assert_eq!(r.results.len(), 1);
    assert_eq!(r.results[0].session_id, one.id);
    let r = m.search(host(), &sock, targets.clone(), &q(r"Needle-b\w+", true, true), 500, &cancel).await.unwrap();
    assert_eq!(r.results.len(), 1);
    assert!(r.results[0].hits.iter().any(|h| h.text.contains("Needle-beta")));
    // With no scrollback only the screen is searched, and the early line is gone.
    let r = m.search(host(), &sock, targets.clone(), &q("needle-alpha", false, false), 0, &cancel).await.unwrap();
    assert!(r.results.is_empty());
    // A cancelled search stops and says so.
    cancel.cancel();
    let r = m.search(host(), &sock, targets, &q("needle", false, false), 500, &cancel).await.unwrap();
    assert!(r.cancelled && r.results.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn a_tmux_server_is_found_only_on_the_socket_asked_for() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    // `-L work` lives in $TMUX_TMPDIR/tmux-UID/work, which is where the default server's folder is.
    let named = Socket::Name("work".into());
    let st = m.operate(host(), &named, &Op::NewSession { name: Some("on-work".into()), cwd: None, command: None }).await.unwrap();
    assert_eq!(st.snapshot.sessions[0].name, "on-work");
    let default = m.snapshot(host(), &Socket::Default, Duration::ZERO).await.unwrap();
    assert_eq!(default.state, ServerState::NoServer, "the default socket has no server");
    assert_eq!(default.other_sockets, vec!["work".to_string()], "but it shows that another socket exists");
    let _ = l.sh("tmux -L work kill-server");
}

/// Attach in a real pseudo-terminal (the tab's PTY), using `script` to provide one.
#[cfg(unix)]
fn spawn_in_pty(l: &real::Local, command: &str) -> std::process::Child {
    use std::process::{Command, Stdio};
    Command::new("script")
        .args(["-qfec", command, "/dev/null"])
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", l.dir.path())
        .env("TERM", "xterm-256color")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn attach_switch_and_closing_the_tab_keep_the_session_alive() {
    need_tmux!();
    if !command_exists("script") {
        eprintln!("skipped: no `script` to give tmux a terminal");
        return;
    }
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    l.tmux(&["new-session", "-d", "-s", "alpha"]);
    l.tmux(&["new-session", "-d", "-s", "beta"]);
    let snap = m.refresh(host(), &sock).await.unwrap();
    let (a, b) = (real::find_session(&snap, "alpha").clone(), real::find_session(&snap, "beta").clone());
    assert_ne!(a.id, b.id);
    let tab = "11111111-2222-3333-4444-555555555555";

    // One click: the command typed into the new tab's shell attaches by session id.
    let cmd = script::attach_command(&sock, &a.id, snap.server.as_ref(), tab, false);
    let mut child = spawn_in_pty(&l, &cmd);
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]) == "alpha"), "attached to the chosen session");
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}:#{session_attached}"]).lines().collect::<Vec<_>>().join(","), "alpha:1,beta:0");

    // Switch: that client, found by the tab's marker, moves to beta. Nothing is typed into the terminal.
    assert!(m.switch_client(host(), &sock, tab, &b.id).await.unwrap());
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]) == "beta"));
    // Switching a tab that is not attached is reported, not guessed at: no other client is moved.
    assert!(!m.switch_client(host(), &sock, "99999999-0000-0000-0000-000000000000", &a.id).await.unwrap());
    assert_eq!(l.tmux(&["list-clients", "-F", "#{session_name}"]), "beta");

    // The tab closes (its PTY goes away): the client is gone, both sessions are alive.
    let _ = child.kill();
    let _ = child.wait();
    assert!(real::wait_for(|| l.tmux(&["list-clients"]).is_empty()), "the client detached");
    let after = m.refresh(host(), &sock).await.unwrap();
    assert_eq!(after.sessions.len(), 2, "closing the tab did not kill the sessions");
    assert!(after.sessions.iter().all(|s| s.attached == 0));

    // Reattach works.
    let mut again = spawn_in_pty(&l, &script::attach_command(&sock, &b.id, after.server.as_ref(), tab, false));
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]) == "beta"));
    let _ = again.kill();
    let _ = again.wait();
}

#[cfg(unix)]
fn command_exists(name: &str) -> bool {
    std::process::Command::new("sh").arg("-c").arg(format!("command -v {name}")).output().is_ok_and(|o| o.status.success())
}

#[cfg(unix)]
#[tokio::test]
async fn the_attach_command_will_not_nest_and_will_not_follow_a_stale_server() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    l.tmux(&["new-session", "-d", "-s", "alpha"]);
    let snap = m.refresh(host(), &sock).await.unwrap();
    let id = snap.sessions[0].id.clone();
    let cmd = script::attach_command(&sock, &id, snap.server.as_ref(), "t-1", false);
    // Inside another tmux (TMUX names a different socket) it says so and does nothing.
    let out = std::process::Command::new("sh").arg("-c").arg(&cmd).env("TMUX", "/elsewhere/sock,1,0").env("TMUX_TMPDIR", l.dir.path()).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("already inside another tmux"), "{}", String::from_utf8_lossy(&out.stdout));
    // With the server restarted since the snapshot, it refuses rather than attach to whatever has the id now.
    l.kill_server();
    assert!(real::wait_for(|| l.tmux(&["list-sessions"]).is_empty()));
    std::thread::sleep(Duration::from_millis(1100));
    l.tmux(&["new-session", "-d", "-s", "other"]);
    let out = l.sh(&cmd);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("server on this host has changed"), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn the_tab_marker_appears_in_the_command_in_the_form_the_interface_reads() {
    // src/lib/stores/tmuxmonitor.svelte.ts `markerOf` finds a tab's tmux client by this exact text: the marker is quoted for
    // `env`, and the whole command is quoted again for `sh -c`, so each quote around it comes out as '\''.
    let cmd = script::attach_command(&Socket::Default, "$3", None, "11111111-2222-3333-4444-555555555555", false);
    assert!(cmd.contains(r"SSHVAULT_TAB='\''11111111-2222-3333-4444-555555555555'\''"), "{cmd}");
    let auto = script::auto_command(&Socket::Default, "main", true, false, false, "abc-1");
    assert!(auto.contains(r"SSHVAULT_TAB='\''abc-1'\''"), "{auto}");
}

#[cfg(unix)]
#[test]
fn the_typed_command_is_one_quoted_word_for_every_shell() {
    // A fake tmux that prints its arguments, one per line, shows what each shell hands over.
    let dir = tempfile::TempDir::new().unwrap();
    let fake = dir.path().join("tmux");
    std::fs::write(&fake, "#!/bin/sh\nprintf '[tab=%s]\\n' \"$SSHVAULT_TAB\"\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\"; done\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let sock = Socket::Path("/tmp/it's a \"sock\"".into());
    let cmd = script::attach_command(&sock, "$12", None, "tab-1", true);
    for shell in ["sh", "bash", "dash", "zsh"] {
        if !command_exists(shell) {
            continue;
        }
        let out = std::process::Command::new(shell).arg("-c").arg(&cmd).env("PATH", format!("{}:/usr/bin:/bin", dir.path().display())).env_remove("TMUX").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).replace('\n', " ");
        assert!(text.contains("[tab=tab-1]") && text.contains("[-S] [/tmp/it's a \"sock\"] [attach-session] [-d] [-t] [$12]"), "{shell}: {text}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sessions_get_cpu_and_memory_from_the_process_trees_of_their_panes() {
    need_tmux!();
    let l = real::Local::new();
    let m = real::manager(&l);
    let sock = l.socket();
    // `busy` burns a core in a child of the pane's shell; `idle` only sleeps.
    l.tmux(&["new-session", "-d", "-s", "busy", "sh -c 'while :; do :; done & while :; do :; done'"]);
    l.tmux(&["new-session", "-d", "-s", "idle", "sleep 300"]);
    std::thread::sleep(Duration::from_millis(300));
    let snap = m.refresh(host(), &sock).await.unwrap();
    let r1 = m.resources(host(), &sock).await.unwrap();
    assert!(r1.estimate);
    let busy = r1.sessions.iter().find(|s| s.session_id == real::find_session(&snap, "busy").id).unwrap();
    let idle = r1.sessions.iter().find(|s| s.session_id == real::find_session(&snap, "idle").id).unwrap();
    let (bu, iu) = (busy.usage.as_ref().unwrap(), idle.usage.as_ref().unwrap());
    assert_eq!((busy.panes_seen, busy.panes_total), (1, 1));
    assert!(bu.processes >= 2 && bu.rss > 0, "{bu:?}");
    assert_eq!(iu.processes, 1, "{iu:?}");
    // Two spinning loops: well over one core's worth, measured over the run's own one-second gap.
    let bc = bu.cpu_pct.expect("the first call takes two readings itself");
    assert!(bc > 60.0, "{bc}");
    assert!(iu.cpu_pct.unwrap() < 20.0);
    // A process is counted once even though it descends from the pane's shell and the tmux server's tree.
    assert!(bu.top.iter().any(|(n, _)| n == "sh"));
    l.tmux(&["kill-server"]);
}

// -- Auto tmux ---------------------------------------------------------------------------------------------------------

#[cfg(unix)]
fn run_auto(l: &real::Local, cmd: &str, tmux_env: Option<&str>) -> std::process::Output {
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(cmd).env("TMUX_TMPDIR", l.dir.path());
    match tmux_env {
        Some(v) => c.env("TMUX", v),
        None => c.env_remove("TMUX"),
    };
    c.output().unwrap()
}

#[cfg(unix)]
#[test]
fn auto_tmux_attaches_creating_the_session_once_and_sets_mouse_for_that_session_only() {
    need_tmux!();
    if !command_exists("script") {
        eprintln!("skipped: no `script` to give tmux a terminal");
        return;
    }
    let l = real::Local::new();
    let sock = l.socket();
    let cmd = script::auto_command(&sock, "main", true, false, true, "auto-1");
    // The first terminal creates `main` and attaches to it.
    let mut a = spawn_in_pty(&l, &cmd);
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]) == "main"));
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "main");
    assert_eq!(l.tmux(&["show-options", "-t", "main", "-v", "mouse"]), "on", "mouse is on for that session");
    assert_eq!(l.tmux(&["show-options", "-g", "-v", "mouse"]), "off", "and not for the server");
    // A second terminal (a reconnect, another tab) attaches to the same session: no duplicate.
    let mut b = spawn_in_pty(&l, &cmd);
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]).lines().count() == 2));
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "main", "still one session");
    let _ = a.kill();
    let _ = b.kill();
    let _ = (a.wait(), b.wait());
    assert!(real::wait_for(|| l.tmux(&["list-clients"]).is_empty()));
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "main", "the session outlives its terminals");
}

#[cfg(unix)]
#[test]
fn auto_tmux_matches_the_session_name_exactly_not_by_prefix() {
    need_tmux!();
    if !command_exists("script") {
        return;
    }
    let l = real::Local::new();
    let sock = l.socket();
    // `main2` exists, `main` does not. Attaching "main" must not land in `main2`.
    l.tmux(&["new-session", "-d", "-s", "main2"]);
    let mut c = spawn_in_pty(&l, &script::auto_command(&sock, "main", false, false, false, "auto-2"));
    std::thread::sleep(Duration::from_millis(800));
    assert!(l.tmux(&["list-clients"]).is_empty(), "nothing attached: {}", l.tmux(&["list-clients"]));
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "main2");
    let _ = c.kill();
    let _ = c.wait();
    // Created on demand, `main` is a session of its own next to `main2`.
    let mut d = spawn_in_pty(&l, &script::auto_command(&sock, "main", true, false, false, "auto-3"));
    assert!(real::wait_for(|| l.tmux(&["list-clients", "-F", "#{session_name}"]) == "main"));
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]).lines().count(), 2);
    let _ = d.kill();
    let _ = d.wait();
}

#[cfg(unix)]
#[test]
fn auto_tmux_attach_only_leaves_the_plain_shell_when_the_session_is_missing() {
    need_tmux!();
    let l = real::Local::new();
    let out = run_auto(&l, &script::auto_command(&l.socket(), "nope", false, false, false, "auto-4"), None);
    assert!(out.status.success(), "the shell goes on");
    assert!(String::from_utf8_lossy(&out.stdout).contains("no tmux session called nope was found"), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(l.tmux(&["list-sessions"]), "", "nothing was created");
}

#[cfg(unix)]
#[test]
fn auto_tmux_never_nests_inside_a_tmux_client() {
    need_tmux!();
    let l = real::Local::new();
    l.tmux(&["new-session", "-d", "-s", "outer"]);
    let out = run_auto(&l, &script::auto_command(&l.socket(), "main", true, false, false, "auto-5"), Some("/anything,1,0"));
    assert!(out.status.success());
    assert_eq!(l.tmux(&["list-sessions", "-F", "#{session_name}"]), "outer", "no new session, no attach");
}

#[cfg(unix)]
#[test]
fn auto_tmux_without_tmux_on_the_host_says_so_and_leaves_the_shell_alone() {
    // A PATH that holds `sh` and nothing else: no tmux.
    let bin = tempfile::TempDir::new().unwrap();
    std::os::unix::fs::symlink("/bin/sh", bin.path().join("sh")).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script::auto_command(&Socket::Default, "main", true, false, false, "auto-6"))
        .env("PATH", bin.path())
        .env_remove("TMUX")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("tmux is not installed on this host"), "{}", String::from_utf8_lossy(&out.stdout));
}

#[cfg(unix)]
#[test]
fn auto_tmux_session_names_cannot_inject_commands() {
    need_tmux!();
    let l = real::Local::new();
    let marker = l.dir.path().join("pwned");
    // Not a valid session name for the app (the form and the command refuse it), and still harmless if one reached the script.
    let name = format!("x'; touch {}; echo '", marker.display());
    let out = run_auto(&l, &script::auto_command(&l.socket(), &name, false, false, false, "auto-7"), None);
    assert!(out.status.success());
    assert!(!marker.exists(), "the name was one quoted word");
}
