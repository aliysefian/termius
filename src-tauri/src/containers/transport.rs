//! Running a runtime's command line, here or over SSH.
//!
//! Local runs spawn the program directly with an argument list, never
//! through a shell. Remote runs ride an SSH connection that stays open for
//! the whole session, one exec channel per command, so a refresh every few
//! seconds costs a channel, not a login. Nothing is installed on the host.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use russh::ChannelMsg;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;

use super::ContainerError;
use crate::ssh::{open_client, Client, Target};

/// Output kept per run. Listings and `inspect` are far below this.
pub const MAX_OUTPUT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    /// `None` when the process was ended by a signal.
    pub code: Option<i32>,
}

// -- quoting ------------------------------------------------------------------

/// One word for a POSIX shell. Plain words stay plain; anything else goes in
/// single quotes, with a quote inside written as `'\''`.
pub fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command line to hand to a remote login shell. It is wrapped in `sh -c`
/// so it parses the same under bash, zsh, fish or csh, and it adds the usual
/// install folders to PATH, which a non-interactive SSH command lacks (Homebrew
/// and Snap put `docker` outside the default).
pub fn remote_command(binary: &str, args: &[String]) -> String {
    let words: Vec<String> = std::iter::once(binary).chain(args.iter().map(String::as_str)).map(shell_quote).collect();
    let script = format!("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin:/snap/bin\"; export PATH; exec {}", words.join(" "));
    format!("sh -c {}", shell_quote(&script))
}

/// A script (not a command) run the same way, for detection.
fn remote_script(script: &str) -> String {
    format!("sh -c {}", shell_quote(&format!("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin:/snap/bin\"; export PATH; {script}")))
}

// -- UTF-8 across chunks -----------------------------------------------------------

/// Turns byte chunks into text without splitting a character that straddles
/// two chunks.
#[derive(Default)]
pub struct Utf8Chunker {
    held: Vec<u8>,
}

impl Utf8Chunker {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.held.extend_from_slice(bytes);
        match std::str::from_utf8(&self.held) {
            Ok(s) => {
                let out = s.to_string();
                self.held.clear();
                out
            }
            Err(e) => {
                let valid = e.valid_up_to();
                let mut out = String::from_utf8_lossy(&self.held[..valid]).into_owned();
                match e.error_len() {
                    // Genuinely invalid bytes: replace them and carry on.
                    Some(n) => {
                        out.push('\u{FFFD}');
                        let rest = self.held.split_off(valid + n);
                        self.held = rest;
                        out.push_str(&self.push(&[]));
                    }
                    // An incomplete character at the end: wait for the rest.
                    None => self.held = self.held.split_off(valid),
                }
                out
            }
        }
    }

    /// Whatever is left when the stream ends.
    pub fn finish(&mut self) -> String {
        let out = String::from_utf8_lossy(&self.held).into_owned();
        self.held.clear();
        out
    }
}

// -- the two transports ----------------------------------------------------------------

pub enum Transport {
    Local,
    /// Boxed: much larger than the local variant.
    Ssh(Box<SshShell>),
}

fn capped(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_OUTPUT)]).into_owned()
}

#[cfg(windows)]
fn no_window(cmd: &mut tokio::process::Command) {
    // CREATE_NO_WINDOW: don't flash a console for every refresh.
    cmd.creation_flags(0x0800_0000);
}
#[cfg(not(windows))]
fn no_window(_: &mut tokio::process::Command) {}

fn local_command(binary: &str, args: &[String]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    no_window(&mut cmd);
    cmd
}

fn spawn_error(binary: &str, e: std::io::Error) -> ContainerError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ContainerError::NotInstalled(binary.to_string())
    } else {
        ContainerError::Io(format!("could not run {binary}: {e}"))
    }
}

impl Transport {
    pub async fn exec(&self, binary: &str, args: &[String], timeout: Duration) -> Result<Output, ContainerError> {
        match self {
            Transport::Local => {
                let child = local_command(binary, args).spawn().map_err(|e| spawn_error(binary, e))?;
                let out = tokio::time::timeout(timeout, child.wait_with_output())
                    .await
                    .map_err(|_| ContainerError::Timeout(timeout.as_secs()))?
                    .map_err(|e| ContainerError::Io(e.to_string()))?;
                Ok(Output { stdout: capped(&out.stdout), stderr: capped(&out.stderr), code: out.status.code() })
            }
            Transport::Ssh(s) => s.exec(&remote_command(binary, args), timeout).await,
        }
    }

    /// Run a short shell script on the remote side, or, locally, report which
    /// of `binaries` start. Used for detection.
    pub async fn which(&self, binaries: &[&str]) -> Result<Vec<String>, ContainerError> {
        match self {
            Transport::Local => {
                let mut found = Vec::new();
                for b in binaries {
                    let ok = local_command(b, &["--version".to_string()])
                        .spawn()
                        .map_err(|e| spawn_error(b, e))
                        .map(|c| async move { c.wait_with_output().await.map(|o| o.status.success()).unwrap_or(false) });
                    if let Ok(fut) = ok {
                        if tokio::time::timeout(Duration::from_secs(10), fut).await.unwrap_or(false) {
                            found.push((*b).to_string());
                        }
                    }
                }
                Ok(found)
            }
            Transport::Ssh(s) => {
                let script = format!("for r in {}; do if command -v \"$r\" >/dev/null 2>&1; then echo \"$r\"; fi; done", binaries.join(" "));
                let out = s.exec(&remote_script(&script), Duration::from_secs(20)).await?;
                Ok(out.stdout.lines().map(|l| l.trim().to_string()).filter(|l| binaries.contains(&l.as_str())).collect())
            }
        }
    }

    /// Run until the process ends or `stop` fires, handing output to `on_data`
    /// as it arrives (stdout and stderr together). Returns the exit code.
    pub async fn stream(
        &self,
        binary: &str,
        args: &[String],
        mut on_data: impl FnMut(&[u8]) + Send,
        mut stop: oneshot::Receiver<()>,
    ) -> Result<Option<i32>, ContainerError> {
        match self {
            Transport::Local => {
                let mut child = local_command(binary, args).spawn().map_err(|e| spawn_error(binary, e))?;
                let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
                let (mut ob, mut eb) = (vec![0u8; 16 * 1024], vec![0u8; 16 * 1024]);
                let (mut out_open, mut err_open) = (out.is_some(), err.is_some());
                while out_open || err_open {
                    tokio::select! {
                        _ = &mut stop => {
                            let _ = child.kill().await;
                            return Ok(None);
                        }
                        n = async { out.as_mut().unwrap().read(&mut ob).await }, if out_open => match n {
                            Ok(0) | Err(_) => out_open = false,
                            Ok(n) => on_data(&ob[..n]),
                        },
                        n = async { err.as_mut().unwrap().read(&mut eb).await }, if err_open => match n {
                            Ok(0) | Err(_) => err_open = false,
                            Ok(n) => on_data(&eb[..n]),
                        },
                    }
                }
                Ok(child.wait().await.ok().and_then(|s| s.code()))
            }
            Transport::Ssh(s) => s.stream(&remote_command(binary, args), on_data, stop).await,
        }
    }

    pub async fn close(&self) {
        if let Transport::Ssh(s) = self {
            s.close().await;
        }
    }
}

// -- SSH ---------------------------------------------------------------------------------

pub struct SshShell {
    target: Target,
    client: tokio::sync::Mutex<Option<Arc<Client>>>,
}

impl SshShell {
    pub async fn connect(target: Target) -> Result<Self, ContainerError> {
        let (client, _) = open_client(&target, None).await?;
        Ok(Self { target, client: tokio::sync::Mutex::new(Some(Arc::new(client))) })
    }

    /// The live connection, or a fresh one if it dropped.
    async fn client(&self, force_new: bool) -> Result<Arc<Client>, ContainerError> {
        let mut guard = self.client.lock().await;
        if let Some(c) = guard.as_ref() {
            if !force_new && !c.is_closed() {
                return Ok(Arc::clone(c));
            }
        }
        let (client, _) = open_client(&self.target, None).await?;
        let client = Arc::new(client);
        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    /// A new exec channel, reconnecting once if the connection died quietly.
    async fn channel(&self) -> Result<russh::Channel<russh::client::Msg>, ContainerError> {
        let c = self.client(false).await?;
        match c.channel_open_session().await {
            Ok(ch) => Ok(ch),
            Err(_) => {
                let c = self.client(true).await?;
                c.channel_open_session().await.map_err(|e| ContainerError::Ssh(e.into()))
            }
        }
    }

    pub async fn exec(&self, command: &str, timeout: Duration) -> Result<Output, ContainerError> {
        let work = async {
            let mut channel = self.channel().await?;
            channel.exec(true, command.as_bytes()).await.map_err(|e| ContainerError::Ssh(e.into()))?;
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let mut code = None;
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => out.extend_from_slice(&data[..data.len().min(MAX_OUTPUT.saturating_sub(out.len()))]),
                    ChannelMsg::ExtendedData { data, ext: 1 } => err.extend_from_slice(&data[..data.len().min(MAX_OUTPUT.saturating_sub(err.len()))]),
                    ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status as i32),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            Ok::<_, ContainerError>(Output { stdout: capped(&out), stderr: capped(&err), code })
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| ContainerError::Timeout(timeout.as_secs()))?
    }

    /// Follow a command's output. A terminal is requested so that closing the
    /// channel hangs the remote process up; without one, `docker logs -f` on a
    /// quiet container would keep running after we stop listening.
    pub async fn stream(
        &self,
        command: &str,
        mut on_data: impl FnMut(&[u8]) + Send,
        mut stop: oneshot::Receiver<()>,
    ) -> Result<Option<i32>, ContainerError> {
        let mut channel = self.channel().await?;
        channel.request_pty(true, "xterm", 200, 50, 0, 0, &[]).await.map_err(|e| ContainerError::Ssh(e.into()))?;
        channel.exec(true, command.as_bytes()).await.map_err(|e| ContainerError::Ssh(e.into()))?;
        let mut code = None;
        loop {
            tokio::select! {
                _ = &mut stop => {
                    let _ = channel.close().await;
                    return Ok(None);
                }
                msg = channel.wait() => match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => on_data(&data),
                    Some(ChannelMsg::ExitStatus { exit_status }) => code = Some(exit_status as i32),
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
            }
        }
        Ok(code)
    }

    pub async fn close(&self) {
        if let Some(c) = self.client.lock().await.take() {
            c.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_keeps_plain_words_and_protects_the_rest() {
        assert_eq!(shell_quote("ps"), "ps");
        assert_eq!(shell_quote("--format"), "--format");
        assert_eq!(shell_quote("{{json .}}"), "'{{json .}}'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_quote("a;b"), "'a;b'");
        assert_eq!(shell_quote("`x`"), "'`x`'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    #[test]
    fn the_remote_command_is_one_quoted_script() {
        let c = remote_command("docker", &["ps".into(), "-a".into(), "--format".into(), "{{json .}}".into()]);
        assert!(c.starts_with("sh -c '") && c.ends_with('\''), "{c}");
        assert!(c.contains("exec docker ps -a --format"), "{c}");
        assert!(c.contains("/opt/homebrew/bin"), "{c}");
        // A hostile argument stays one word inside the script.
        let c = remote_command("docker", &["rm".into(), "x; reboot".into()]);
        assert!(c.contains("rm '\\''x; reboot'\\''"), "{c}");
    }

    #[test]
    fn utf8_split_across_chunks_is_reassembled() {
        let mut c = Utf8Chunker::default();
        let bytes = "héllo wörld ✓".as_bytes();
        let mut out = String::new();
        // Feed one byte at a time: the worst case.
        for b in bytes {
            out.push_str(&c.push(&[*b]));
        }
        out.push_str(&c.finish());
        assert_eq!(out, "héllo wörld ✓");
        // Two chunks splitting the three-byte ✓.
        let mut c = Utf8Chunker::default();
        let ok = "✓".as_bytes();
        assert_eq!(c.push(&ok[..1]), "");
        assert_eq!(c.push(&ok[1..]), "✓");
    }

    #[test]
    fn invalid_bytes_are_replaced_not_stuck() {
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(b"a\xFFb"), "a\u{FFFD}b");
        assert_eq!(c.push(b"ok"), "ok", "the stream carries on afterwards");
        // A truncated character at the very end is flushed, not lost.
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(&"é".as_bytes()[..1]), "");
        assert_eq!(c.finish(), "\u{FFFD}");
        assert_eq!(c.finish(), "");
    }
}
