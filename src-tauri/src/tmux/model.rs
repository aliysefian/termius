//! What a host's tmux server looks like, as the app sees it. Everything here belongs to the remote *server*: none of
//! it knows about SSHVault's terminal tabs. A session can exist with no tab attached, and a tab can close without
//! touching a session.
//!
//! Identity is tmux's own: `$N` for sessions, `@N` for windows, `%N` for panes. Those are stable for the life of one
//! server process, so every snapshot also carries a [`Server`] identity (`pid` and start time): an id is only
//! meaningful together with the server it came from, and a mutation checks it before it acts.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which tmux server on a host. The default socket is `$TMUX_TMPDIR/tmux-UID/default`; `-L name` picks another one in
/// the same folder and `-S path` any socket at all. A non-interactive SSH command does not read the profile that may
/// set `TMUX_TMPDIR`, so a server the user's shell finds may need its socket named here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Socket {
    #[default]
    Default,
    /// `tmux -L name`.
    Name(String),
    /// `tmux -S /path/to/socket`.
    Path(String),
}

impl Socket {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Socket::Default => Ok(()),
            Socket::Name(n) => {
                if n.is_empty() || n.len() > 64 || !n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) || n.starts_with('-') {
                    Err("A tmux socket name may contain letters, digits, '.', '_' and '-' (up to 64).".into())
                } else {
                    Ok(())
                }
            }
            Socket::Path(p) => {
                // `{{` is the app's own placeholder syntax in commands typed into a terminal.
                if !p.starts_with('/') || p.len() > 400 || p.chars().any(|c| c.is_control()) || p.contains("{{") {
                    Err("A tmux socket path must be absolute and free of control characters and '{{'.".into())
                } else {
                    Ok(())
                }
            }
        }
    }

    /// A short stable key, for storing tags and settings against a socket: `default`, `L:name`, `S:/path`.
    pub fn key(&self) -> String {
        match self {
            Socket::Default => "default".into(),
            Socket::Name(n) => format!("L:{n}"),
            Socket::Path(p) => format!("S:{p}"),
        }
    }
}

/// `tmux -V`: `tmux 3.4`, `tmux 3.3a`, `tmux next-3.5`, `tmux master`, `tmux openbsd-7.4`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    /// Built from source ahead of a release: newer than any version this code knows by number.
    pub dev: bool,
    pub raw: String,
}

/// The oldest tmux the formats, ids and `capture-pane` options used here are known to work on.
pub const MIN_VERSION: (u32, u32) = (2, 0);

impl Version {
    pub fn parse(line: &str) -> Option<Version> {
        let t = line.trim();
        let rest = t.strip_prefix("tmux")?.trim_start();
        if rest.is_empty() {
            return None;
        }
        if rest == "master" || rest.starts_with("master") {
            return Some(Version { major: u32::MAX, minor: 0, dev: true, raw: t.into() });
        }
        // OpenBSD's tmux is versioned by the OS release and is always recent.
        if rest.starts_with("openbsd-") {
            return Some(Version { major: 3, minor: 5, dev: false, raw: t.into() });
        }
        let (dev, rest) = match rest.strip_prefix("next-") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let mut parts = rest.splitn(2, '.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor_text = parts.next()?;
        let digits: String = minor_text.chars().take_while(|c| c.is_ascii_digit()).collect();
        let minor: u32 = digits.parse().ok()?;
        Some(Version { major, minor, dev, raw: t.into() })
    }

    pub fn at_least(&self, major: u32, minor: u32) -> bool {
        self.major > major || (self.major == major && self.minor >= minor)
    }

    pub fn supported(&self) -> bool {
        self.at_least(MIN_VERSION.0, MIN_VERSION.1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    /// `%N`.
    pub id: String,
    pub index: u32,
    pub active: bool,
    pub title: String,
    /// The program in the foreground (`#{pane_current_command}`).
    pub command: String,
    /// Empty when this tmux cannot say.
    pub path: String,
    pub pid: Option<u32>,
    pub width: u32,
    pub height: u32,
    /// The program ended and the pane is kept open (`remain-on-exit`).
    pub dead: bool,
    /// In copy mode or another mode: what is on screen is not the program's output.
    pub in_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// `@N`.
    pub id: String,
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub layout: String,
    pub zoomed: bool,
    /// Activity or a bell in a window other than the current one.
    pub activity: bool,
    pub bell: bool,
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// `$N`.
    pub id: String,
    pub name: String,
    /// How many clients are attached.
    pub attached: u32,
    /// Unix seconds; 0 when unknown.
    pub created: u64,
    /// Unix seconds of the last activity in the session; 0 when this tmux cannot say.
    pub activity: u64,
    /// Sessions in a group share their windows.
    pub group: Option<String>,
    /// The session's `mouse` option as tmux reports it; `None` when this tmux does not say.
    pub mouse: Option<bool>,
    pub windows: Vec<Window>,
}

impl Session {
    pub fn pane_count(&self) -> usize {
        self.windows.iter().map(|w| w.panes.len()).sum()
    }
}

/// One running tmux server. Ids are valid for this server only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub pid: u32,
    /// Unix seconds; 0 when this tmux cannot say.
    pub started: u64,
    pub socket_path: String,
}

impl Server {
    /// What a mutation compares before it acts: a restarted server can reuse `$0`, `@0` and `%0`.
    pub fn token(&self) -> String {
        format!("{}:{}", self.pid, self.started)
    }
}

/// Why there is nothing to show, or that there is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerState {
    /// tmux is not installed (or not on the PATH an SSH command gets).
    NotInstalled { os: String },
    /// Nothing is listening on the socket: no tmux server is running.
    NoServer,
    /// A server answers and has no sessions (`exit-empty off`).
    Empty,
    /// tmux is older than [`MIN_VERSION`].
    Unsupported { version: String },
    /// The SSH user may not use that socket.
    PermissionDenied { message: String },
    /// tmux answered with something this code does not understand, or the answer was damaged.
    Failed { message: String },
    Ready,
}

/// Why the data shown is not fresh. The last good data stays on screen with this beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stale {
    /// Since when (ms since 1970) the data has been the last good one.
    pub since_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSnapshot {
    pub host_id: Uuid,
    pub socket: Socket,
    /// Rises with every accepted reading of this host and socket: a view drops anything older than what it has.
    pub generation: u64,
    pub taken_at_ms: u64,
    pub version: Option<Version>,
    pub state: ServerState,
    pub server: Option<Server>,
    pub sessions: Vec<Session>,
    /// Other sockets found in the default socket folder (`tmux -L` names), so a server the default one does not
    /// show can be found.
    pub other_sockets: Vec<String>,
    pub stale: Option<Stale>,
}

// -- text that came from the host --------------------------------------------------------------------------------------

/// Free text from tmux (names, titles, paths) made safe to show and to keep: control characters become U+FFFD, and so do
/// the bidirectional overrides that make one name look like another.
pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}') {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// The id forms tmux uses. Anything else never reaches a command line.
pub fn valid_session_id(s: &str) -> bool {
    id_with(s, '$')
}
pub fn valid_window_id(s: &str) -> bool {
    id_with(s, '@')
}
pub fn valid_pane_id(s: &str) -> bool {
    id_with(s, '%')
}

fn id_with(s: &str, sigil: char) -> bool {
    let mut it = s.chars();
    it.next() == Some(sigil) && s.len() > 1 && s.len() <= 11 && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Every id in a snapshot is unique within it; used by tests and by the reconciler.
pub fn ids_unique(sessions: &[Session]) -> bool {
    let mut seen = HashSet::new();
    for s in sessions {
        if !seen.insert(s.id.clone()) {
            return false;
        }
        for w in &s.windows {
            if !seen.insert(w.id.clone()) {
                return false;
            }
            for p in &w.panes {
                if !seen.insert(p.id.clone()) {
                    return false;
                }
            }
        }
    }
    true
}
