//! Finding tmux and GNU screen sessions on a host, for the multiplexer bar.
//!
//! One fixed, read-only script runs over an extra channel of the pane's own connection, so the interactive shell
//! never sees it. The frontend parses the sections (`src/lib/tmux/discovery.ts`); nothing here takes input.

use std::time::Duration;

use crate::containers::transport::shell_quote;
use crate::ssh::Client;

const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BYTES: usize = 64 * 1024;

/// POSIX `sh`. Each section starts with an `@@name` line. Errors (no server running, no tmux, no screen) are
/// silenced: an empty section means "none".
pub const PROBE_SCRIPT: &str = concat!(
    r##"echo @@tmux-version; tmux -V 2>/dev/null; "##,
    r##"echo @@tmux-sessions; "##,
    r##"tmux list-sessions -F "#{session_id}|#{session_name}|#{session_windows}|#{session_attached}|#{session_created}" 2>/dev/null; "##,
    r##"echo @@tmux-windows; "##,
    r##"tmux list-windows -a -F "#{session_name}|#{window_index}|#{window_name}|#{pane_current_command}|#{session_last_attached}" 2>/dev/null; "##,
    r##"echo @@screen; screen -ls 2>/dev/null; true"##,
);

fn command() -> String {
    // Install folders a non-interactive SSH command lacks (Homebrew, Snap), as for completion lookups.
    let full = format!("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin:/snap/bin\"; export PATH; {PROBE_SCRIPT}");
    format!("sh -c {}", shell_quote(&full))
}

/// Run the probe; the raw text comes back for the frontend to parse. A host that won't run commands answers `None`.
pub async fn probe(client: &Client) -> Option<String> {
    let out = client.exec(&command(), None, TIMEOUT, MAX_BYTES).await.ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_is_one_quoted_argument_and_writes_nothing() {
        let c = command();
        assert!(c.starts_with("sh -c '"));
        // Read-only: no attach, kill or new session, whatever tmux or screen are asked.
        // ("session_attached" is a format field, so a bare "attach" would match it.)
        for verb in ["attach-session", " attach ", "kill-", "new-session", " new ", "send-keys", " -X", " -S "] {
            assert!(!PROBE_SCRIPT.contains(verb), "{verb}");
        }
        assert!(PROBE_SCRIPT.contains("list-sessions"));
        assert!(PROBE_SCRIPT.contains("screen -ls"));
    }
}
