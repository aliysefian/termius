//! Detail monitoring: one SSH connection kept open per host being watched, so
//! sampling every few seconds costs a channel, not a login. Tauri-agnostic, and
//! built on the same transport as the Containers view.
//!
//! What to run is the caller's: the sampling script and the "signal a process"
//! script both live in the app. Scripts are wrapped in `sh -c`, so they behave
//! the same whatever the host's login shell is.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use crate::containers::transport::{shell_quote, SshShell};
use crate::containers::ContainerError;
use crate::ssh::Target;

/// The longest a script may run.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

/// A script as one command for any login shell.
pub fn wrap(script: &str) -> String {
    format!("sh -c {}", shell_quote(script))
}

#[derive(Default)]
pub struct MonitorManager {
    sessions: Mutex<HashMap<Uuid, Arc<SshShell>>>,
}

impl MonitorManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connect to `target` and keep the connection. The connection is reopened by itself if it drops.
    pub async fn open(&self, target: Target) -> Result<Uuid, ContainerError> {
        let shell = SshShell::connect(target).await?;
        let id = Uuid::new_v4();
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(shell));
        Ok(id)
    }

    fn get(&self, id: Uuid) -> Result<Arc<SshShell>, ContainerError> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned().ok_or(ContainerError::NoSession)
    }

    pub async fn exec(&self, id: Uuid, script: &str, timeout: Duration) -> Result<ExecResult, ContainerError> {
        let out = self.get(id)?.exec(&wrap(script), timeout.min(MAX_TIMEOUT)).await?;
        Ok(ExecResult { stdout: out.stdout, stderr: out.stderr, code: out.code })
    }

    pub async fn close(&self, id: Uuid) {
        let s = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        if let Some(s) = s {
            s.close().await;
        }
    }

    pub async fn close_all(&self) {
        let all: Vec<Arc<SshShell>> = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).drain().map(|(_, s)| s).collect();
        for s in all {
            s.close().await;
        }
    }

    pub fn open_count(&self) -> usize {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// The script the app ships, and the hand-written fixtures that assume its layout.
    const SCRIPT: &str = include_str!("../../src/lib/hostdetail.sh");
    const DARWIN: &str = include_str!("../../src/lib/__tests__/hostdetail/darwin.txt");
    const FREEBSD: &str = include_str!("../../src/lib/__tests__/hostdetail/freebsd.txt");

    fn sh_available() -> bool {
        Command::new("sh").args(["-c", "true"]).status().map(|s| s.success()).unwrap_or(false)
    }

    /// The body of each `@@NAME` section of a fixture, in order, with the leading comment dropped.
    fn sections(text: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("@@") {
                out.push((name.trim().to_string(), String::new()));
            } else if let Some(last) = out.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
        out
    }

    #[test]
    fn scripts_are_one_quoted_command() {
        let w = wrap("echo 'hi'; kill -TERM 5");
        assert!(w.starts_with("sh -c '") && w.ends_with('\''), "{w}");
        assert!(w.contains("echo '\\''hi'\\''"), "{w}");
        assert_eq!(wrap(""), "sh -c ''");
    }

    /// Shell stubs that stand in for a BSD's tools, answering from a fixture's sections.
    fn stubs(os: &str, fixture: &str) -> String {
        let sec = sections(fixture);
        let nth = |name: &str, n: usize| -> String { sec.iter().filter(|(k, _)| k == name).nth(n).map(|(_, v)| v.clone()).unwrap_or_default() };
        // A stub that prints a fixture body. The delimiter can't appear in a fixture.
        // Whatever the checkout did to line endings, the body ends in a newline so the delimiter starts a line.
        let say = |s: String| {
            let s = s.replace("\r\n", "\n");
            let nl = if s.is_empty() || s.ends_with('\n') { "" } else { "\n" };
            format!("cat <<'__FIXTURE__'\n{s}{nl}__FIXTURE__\n")
        };
        // A stub that prints one body the first time it is called and another after.
        let twice = |name: &str, first: String, second: String| {
            format!("calls_{name}=0\n{name}() {{\n  calls_{name}=$((calls_{name}+1))\n  if [ \"$calls_{name}\" = 1 ]; then\n{}  else\n{}  fi\n}}\n", say(first), say(second))
        };
        // The fixture's port section: a `tool` line, then that tool's output.
        let ports = nth("PORTS", 0);
        let rest: Vec<&str> = ports.lines().skip(1).collect();
        // lsof is asked twice (TCP, then UDP): the fixture's second header line starts the UDP part.
        let split = rest.iter().skip(1).position(|l| l.starts_with("COMMAND")).map(|i| i + 1).unwrap_or(rest.len());
        let (tcp, udp) = (rest[..split].join("\n") + "\n", rest[split..].join("\n") + "\n");
        let single = |name: &str, body: String| format!("{name}() {{\n{}}}\n", say(body));
        [
            format!("uname() {{ echo {os}; }}\nsleep() {{ :; }}\n"),
            twice("date", nth("TA", 0), nth("TB", 0)),
            twice("netstat", nth("NETA", 0), nth("NETB", 0)),
            twice("lsof", tcp, udp),
            single("ps", nth("PROCPS", 0)),
            single("sysctl", nth("LOAD", 0)),
            single("df", nth("DISK", 0)),
            single("sockstat", rest.join("\n") + "\n"),
            single("ifconfig", nth("IFCONFIG", 0)),
        ]
        .concat()
    }

    /// Run the real script with a BSD's tools replaced by stubs. The fixtures were written in the
    /// script's layout by hand, so this checks the script really prints that layout.
    ///
    /// `hide` names tools this pretend host doesn't have. Removing a stub isn't enough (a real copy
    /// on this computer would be found, and its output, this computer's, would be printed), so the
    /// script's own `have` is made to answer "no" for them.
    fn run_with_stubs(os: &str, fixture: &str, hide: &[&str]) -> String {
        let original = "have() { command -v \"$1\" >/dev/null 2>&1; }";
        let patched = SCRIPT.replace("\r\n", "\n").replacen(original, "have() { case \" $HIDE \" in *\" $1 \"*) return 1;; esac; command -v \"$1\" >/dev/null 2>&1; }", 1);
        assert!(patched.contains("$HIDE"), "the script's `have` function changed; update this test's patch");
        let script = format!("HIDE='{}'\n{}\n{}", hide.join(" "), stubs(os, fixture), patched);
        // Through a file, not `sh -c`: on Windows the whole command line is cut off at about 8,000
        // characters, and this script with its stubs is longer, so the shell saw half a script.
        let file = tempfile::Builder::new().suffix(".sh").tempfile().expect("temp file");
        std::fs::write(file.path(), &script).expect("write the script");
        let out = Command::new("sh").arg(file.path()).output().expect("sh");
        assert!(out.status.success(), "script failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn assert_same_layout(got: &str, fixture: &str) {
        let (g, f) = (sections(got), sections(fixture));
        assert_eq!(g.iter().map(|s| &s.0).collect::<Vec<_>>(), f.iter().map(|s| &s.0).collect::<Vec<_>>(), "sections differ");
        for ((name, got_body), (_, want_body)) in g.iter().zip(f.iter()) {
            // Whitespace at the ends of a line depends on the heredoc; the content is what must match.
            let norm = |s: &str| s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string();
            assert_eq!(norm(got_body), norm(want_body), "section @@{name} differs");
        }
    }

    #[test]
    fn the_script_prints_the_layout_the_macos_fixture_assumes() {
        if !sh_available() {
            return;
        }
        let out = run_with_stubs("Darwin", DARWIN, &["sockstat"]);
        assert_same_layout(&out, DARWIN);
    }

    #[test]
    fn the_script_prints_the_layout_the_freebsd_fixture_assumes() {
        if !sh_available() {
            return;
        }
        let out = run_with_stubs("FreeBSD", FREEBSD, &["lsof"]);
        assert_same_layout(&out, FREEBSD);
    }

    #[test]
    fn a_bsd_host_with_no_port_tool_says_so() {
        if !sh_available() {
            return;
        }
        let out = run_with_stubs("OpenBSD", FREEBSD, &["lsof", "sockstat"]);
        let ports = sections(&out).into_iter().find(|(k, _)| k == "PORTS").unwrap().1;
        assert_eq!(ports.trim(), "tool none");
    }

    // -- over a real sshd ---------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_session_runs_the_real_script_over_one_reused_connection() {
        use crate::ssh::testutil::{spawn_sshd, target};
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        if !sh_available() {
            return;
        }
        let m = MonitorManager::new();
        let id = m.open(target(&sshd, &sshd.client_key, dir.path().join("kh"))).await.unwrap();
        assert_eq!(m.open_count(), 1);

        // Quoting survives: quotes, a dollar sign, a semicolon and a newline reach the host intact.
        let r = m.exec(id, "printf '%s|' 'it'\\''s' \"$HOME\" 'a;b'; echo done", Duration::from_secs(20)).await.unwrap();
        assert_eq!(r.code, Some(0));
        assert!(r.stdout.starts_with("it's|") && r.stdout.contains("|a;b|done"), "{:?}", r.stdout);

        // The real sampling script, twice, over the same connection.
        if cfg!(target_os = "linux") {
            for _ in 0..2 {
                let started = std::time::Instant::now();
                let r = m.exec(id, &SCRIPT.replace("\r\n", "\n"), Duration::from_secs(30)).await.unwrap();
                assert_eq!(r.code, Some(0), "{}", r.stderr);
                assert!(r.stderr.is_empty(), "{}", r.stderr);
                assert!(started.elapsed() >= Duration::from_millis(950), "it samples a second apart");
                let names: Vec<_> = sections(&r.stdout).into_iter().map(|(k, _)| k).collect();
                assert_eq!(&names[..3], ["OS", "CLK", "TA"], "{names:?}");
                for want in ["PROCA", "PROCB", "NETA", "NETB", "PORTS", "SYSNET"] {
                    assert!(names.iter().any(|n| n == want), "missing @@{want}");
                }
                assert!(r.stdout.starts_with("@@OS\nLinux\n"));
            }
        }

        // A failing script reports its own status and its own words.
        let r = m.exec(id, "echo oops >&2; exit 3", Duration::from_secs(20)).await.unwrap();
        assert_eq!((r.code, r.stderr.trim()), (Some(3), "oops"));
        // A script that runs too long is stopped with a timeout, and the session still works after.
        let err = m.exec(id, "sleep 5", Duration::from_secs(1)).await.unwrap_err();
        assert!(matches!(err, ContainerError::Timeout(1)), "{err}");
        assert_eq!(m.exec(id, "echo again", Duration::from_secs(20)).await.unwrap().stdout.trim(), "again");

        // Closing ends it; asking again says there's no such session.
        m.close(id).await;
        assert_eq!(m.open_count(), 0);
        assert!(matches!(m.exec(id, "true", Duration::from_secs(5)).await, Err(ContainerError::NoSession)));
        m.close_all().await;
    }
}

