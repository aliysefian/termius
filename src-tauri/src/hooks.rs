//! Hooks: a command this computer runs before a connection opens or after it closes (start a VPN, open a tunnel,
//! update a status page). The command is the person's own text, approved by them in the window before it is run;
//! all this does is run one line through the shell with no input, a time limit and a cap on what it can print back.

use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncReadExt;

pub const MAX_COMMAND: usize = 500;
pub const MAX_OUTPUT: usize = 4096;
pub const MAX_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HookResult {
    pub exit_code: Option<i32>,
    /// The first part of what it printed (standard output and error together).
    pub output: String,
    pub timed_out: bool,
}

pub fn check(command: &str) -> Result<(), String> {
    let c = command.trim();
    if c.is_empty() {
        return Err("the command is empty".into());
    }
    if c.len() > MAX_COMMAND {
        return Err(format!("the command is longer than {MAX_COMMAND} characters"));
    }
    if c.contains(['\n', '\r', '\0']) {
        return Err("the command must be one line".into());
    }
    Ok(())
}

#[cfg(windows)]
fn shell(command: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("cmd");
    c.arg("/C").arg(command);
    c.creation_flags(0x0800_0000);
    c
}

#[cfg(not(windows))]
fn shell(command: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("sh");
    c.arg("-c").arg(command);
    c
}

/// Keep the first `MAX_OUTPUT` bytes of a stream and drain the rest, so the command can't block on a full pipe.
async fn keep_first<R: tokio::io::AsyncRead + Unpin>(mut r: R) -> Vec<u8> {
    let mut keep = Vec::new();
    let _ = (&mut r).take(MAX_OUTPUT as u64).read_to_end(&mut keep).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    keep
}

/// Run `command` and wait for it, up to `timeout`; a command that doesn't finish is killed.
pub async fn run(command: &str, timeout: Duration) -> Result<HookResult, String> {
    check(command)?;
    let mut cmd = shell(command.trim());
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("couldn't run it: {e}"))?;
    let out = tokio::spawn(keep_first(child.stdout.take().expect("piped")));
    let err = tokio::spawn(keep_first(child.stderr.take().expect("piped")));
    let limit = timeout.min(Duration::from_secs(MAX_TIMEOUT_SECS));
    match tokio::time::timeout(limit, child.wait()).await {
        Err(_) => {
            let _ = child.kill().await;
            out.abort();
            err.abort();
            Ok(HookResult { exit_code: None, output: String::new(), timed_out: true })
        }
        Ok(status) => {
            let status = status.map_err(|e| e.to_string())?;
            let mut text = String::from_utf8_lossy(&out.await.unwrap_or_default()).into_owned();
            text.push_str(&String::from_utf8_lossy(&err.await.unwrap_or_default()));
            let end = text.char_indices().nth(MAX_OUTPUT).map_or(text.len(), |(i, _)| i);
            text.truncate(end);
            Ok(HookResult { exit_code: status.code(), output: text, timed_out: false })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_one_non_empty_line_of_reasonable_length() {
        assert!(check("echo hi").is_ok());
        for bad in ["", "   ", "a\nb", "a\rb", "a\0b"] {
            assert!(check(bad).is_err(), "{bad:?}");
        }
        assert!(check(&"x".repeat(MAX_COMMAND + 1)).is_err());
        assert!(check(&"x".repeat(MAX_COMMAND)).is_ok());
    }

    #[tokio::test]
    async fn what_it_prints_and_its_exit_code_come_back() {
        let r = run("echo out && echo err 1>&2 && exit 3", Duration::from_secs(10)).await.unwrap();
        assert_eq!(r.exit_code, Some(3));
        assert!(r.output.contains("out") && r.output.contains("err"), "{:?}", r.output);
        assert!(!r.timed_out);
        assert_eq!(run("exit 0", Duration::from_secs(10)).await.unwrap().exit_code, Some(0));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_slow_command_is_killed_and_has_no_input() {
        let started = std::time::Instant::now();
        let r = run("sleep 5", Duration::from_millis(200)).await.unwrap();
        assert!(r.timed_out && r.exit_code.is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
        // Reading standard input finds it closed instead of waiting.
        assert_eq!(run("cat; echo done", Duration::from_secs(5)).await.unwrap().output.trim(), "done");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_lot_of_output_is_cut_and_does_not_block() {
        let r = run("head -c 3000000 /dev/zero | tr '\\0' 'x'", Duration::from_secs(20)).await.unwrap();
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.output.len(), MAX_OUTPUT);
    }

    #[tokio::test]
    async fn refuses_what_check_refuses() {
        assert!(run("a\nb", Duration::from_secs(1)).await.is_err());
    }
}
