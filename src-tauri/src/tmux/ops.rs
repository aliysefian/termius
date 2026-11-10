//! Everything that changes a tmux server. One enum, one validation, one place that builds the command: nothing else in
//! the app composes a mutating tmux command line.
//!
//! Every target is a tmux id (`$N`, `@N`, `%N`) validated against its exact form and then quoted, so a selected pane is the
//! pane that is acted on (not "the active one"), and a name from the host can never be mistaken for an option. Each
//! operation is run against the server identity of the snapshot it was chosen from (see `script::operation`), and the
//! caller verifies the result against a fresh reading ([`verify`]) before the UI is told it worked.

use serde::{Deserialize, Serialize};

use super::model::{valid_pane_id, valid_session_id, valid_window_id, HostSnapshot, Socket};
use super::script::{operation, quote, tmux_cmd};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    /// New pane to the right of the target: `split-window -h`.
    Right,
    /// New pane below the target: `split-window -v`.
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeDir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    NewSession { name: Option<String>, cwd: Option<String>, command: Option<String> },
    RenameSession { session: String, name: String },
    KillSession { session: String },
    NewWindow { session: String, name: Option<String>, cwd: Option<String>, command: Option<String> },
    RenameWindow { window: String, name: String },
    SelectWindow { window: String },
    KillWindow { window: String },
    SplitPane { pane: String, dir: SplitDir, cwd: Option<String>, command: Option<String> },
    SelectPane { pane: String },
    SetPaneTitle { pane: String, title: String },
    ResizePane { pane: String, dir: ResizeDir, amount: u32 },
    ZoomPane { pane: String },
    KillPane { pane: String },
    /// Mouse mode for one session only. Server-wide options are never touched here.
    SetMouse { session: String, on: bool },
}

/// A name or title as one tmux argument that arrives exactly as typed.
///
/// * tmux expands formats in names, titles and folders: `#{pid}` becomes a number and `#(cmd)` *runs* `cmd` on the host.
///   `##` is a literal `#`, so every `#` is doubled.
/// * tmux reads an argument that ends in `;` as the end of a command (so `tmux a \; b` works), which would cut `deploy;`
///   short and run what follows as a second command. Written `\;` it is the character itself.
fn text(value: &str) -> String {
    let v = value.replace('#', "##");
    match v.strip_suffix(';') {
        Some(head) => quote(&format!("{head}\\;")),
        None => quote(&v),
    }
}

/// A shell command for a pane. tmux hands it to the shell as it is (no format expansion), but a trailing `;` is still the
/// end of a tmux command.
fn command_text(value: &str) -> String {
    match value.strip_suffix(';') {
        Some(head) => quote(&format!("{head}\\;")),
        None => quote(value),
    }
}

const MAX_NAME: usize = 100;
const MAX_TEXT: usize = 2000;

fn no_control(s: &str, what: &str) -> Result<(), String> {
    if s.chars().any(|c| c.is_control()) {
        Err(format!("{what} cannot contain control characters."))
    } else {
        Ok(())
    }
}

/// A session name tmux keeps as typed. It rewrites `.` and `:` to `_` without saying so, which would make the name
/// the user chose differ from the one that exists, so they are refused here.
pub fn valid_session_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("A session needs a name.".into());
    }
    if t != name {
        return Err("A session name cannot start or end with a space.".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("A name can be up to {MAX_NAME} characters."));
    }
    if name.contains('.') || name.contains(':') {
        return Err("A tmux session name cannot contain '.' or ':' (tmux would change them).".into());
    }
    if name.contains('\\') {
        return Err("A tmux name cannot contain a backslash (tmux stores it doubled).".into());
    }
    no_control(name, "A name")
}

fn valid_label(name: &str, what: &str) -> Result<(), String> {
    if name.chars().count() > MAX_NAME {
        return Err(format!("{what} can be up to {MAX_NAME} characters."));
    }
    if name.contains('\\') {
        return Err(format!("{what} cannot contain a backslash (tmux stores it doubled)."));
    }
    no_control(name, what)
}

fn valid_cwd(cwd: &Option<String>) -> Result<(), String> {
    match cwd {
        None => Ok(()),
        Some(c) if c.trim().is_empty() => Ok(()),
        Some(c) => {
            if c.len() > 1000 {
                return Err("The folder path is too long.".into());
            }
            no_control(c, "A folder path")?;
            if !(c.starts_with('/') || c.starts_with("~/") || c == "~") {
                return Err("The folder must be an absolute path (or start with ~/).".into());
            }
            Ok(())
        }
    }
}

fn valid_command(cmd: &Option<String>) -> Result<(), String> {
    match cmd {
        None => Ok(()),
        Some(c) => {
            if c.len() > MAX_TEXT {
                return Err("The command is too long.".into());
            }
            // A command is one line: a newline would be a second command typed by the pane's shell.
            no_control(c, "A command")
        }
    }
}

impl Op {
    /// Everything that can be checked without asking the host.
    pub fn validate(&self) -> Result<(), String> {
        let sid = |s: &str| if valid_session_id(s) { Ok(()) } else { Err("That is not a tmux session id.".to_string()) };
        let wid = |s: &str| if valid_window_id(s) { Ok(()) } else { Err("That is not a tmux window id.".to_string()) };
        let pid = |s: &str| if valid_pane_id(s) { Ok(()) } else { Err("That is not a tmux pane id.".to_string()) };
        match self {
            Op::NewSession { name, cwd, command } => {
                if let Some(n) = name {
                    valid_session_name(n)?;
                }
                valid_cwd(cwd)?;
                valid_command(command)
            }
            Op::RenameSession { session, name } => {
                sid(session)?;
                valid_session_name(name)
            }
            Op::KillSession { session } => sid(session),
            Op::NewWindow { session, name, cwd, command } => {
                sid(session)?;
                if let Some(n) = name {
                    valid_label(n, "A window name")?;
                }
                valid_cwd(cwd)?;
                valid_command(command)
            }
            Op::RenameWindow { window, name } => {
                wid(window)?;
                if name.is_empty() {
                    return Err("A window needs a name.".into());
                }
                valid_label(name, "A window name")
            }
            Op::SelectWindow { window } | Op::KillWindow { window } => wid(window),
            Op::SplitPane { pane, cwd, command, .. } => {
                pid(pane)?;
                valid_cwd(cwd)?;
                valid_command(command)
            }
            Op::SelectPane { pane } | Op::ZoomPane { pane } | Op::KillPane { pane } => pid(pane),
            Op::SetPaneTitle { pane, title } => {
                pid(pane)?;
                valid_label(title, "A pane title")
            }
            Op::ResizePane { pane, amount, .. } => {
                pid(pane)?;
                if (1..=500).contains(amount) {
                    Ok(())
                } else {
                    Err("Resize by 1 to 500 cells.".into())
                }
            }
            Op::SetMouse { session, .. } => sid(session),
        }
    }

    /// Creating something starts from nothing, so there is no server identity to check (the server may not exist yet).
    pub fn needs_server(&self) -> bool {
        !matches!(self, Op::NewSession { .. })
    }

    /// The tmux command line, for a validated operation.
    pub fn command(&self, socket: &Socket) -> String {
        let t = tmux_cmd(socket);
        let cwd_arg = |cwd: &Option<String>| match cwd {
            Some(c) if !c.trim().is_empty() => {
                // `~` is the remote shell's to expand; tmux's `-c` takes a path.
                let c = if c == "~" { "\"$HOME\"".to_string() } else if let Some(rest) = c.strip_prefix("~/") { format!("\"$HOME\"/{}", quote(&rest.replace('#', "##"))) } else { quote(&c.replace('#', "##")) };
                format!(" -c {c}")
            }
            _ => String::new(),
        };
        let cmd_arg = |cmd: &Option<String>| match cmd {
            Some(c) if !c.trim().is_empty() => format!(" -- {}", command_text(c)),
            _ => String::new(),
        };
        match self {
            Op::NewSession { name, cwd, command } => {
                let n = name.as_ref().map(|n| format!(" -s {}", text(n))).unwrap_or_default();
                format!("{t} new-session -d -P -F '#{{session_id}}'{n}{}{}", cwd_arg(cwd), cmd_arg(command))
            }
            Op::RenameSession { session, name } => format!("{t} rename-session -t {} -- {}", quote(session), text(name)),
            Op::KillSession { session } => format!("{t} kill-session -t {}", quote(session)),
            Op::NewWindow { session, name, cwd, command } => {
                let n = name.as_ref().filter(|n| !n.is_empty()).map(|n| format!(" -n {}", text(n))).unwrap_or_default();
                // `session:` with nothing after the colon is "the next free index in this session".
                format!("{t} new-window -P -F '#{{window_id}}' -t {}{n}{}{}", quote(&format!("{session}:")), cwd_arg(cwd), cmd_arg(command))
            }
            Op::RenameWindow { window, name } => format!("{t} rename-window -t {} -- {}", quote(window), text(name)),
            Op::SelectWindow { window } => format!("{t} select-window -t {}", quote(window)),
            Op::KillWindow { window } => format!("{t} kill-window -t {}", quote(window)),
            Op::SplitPane { pane, dir, cwd, command } => {
                let flag = match dir {
                    SplitDir::Right => "-h",
                    SplitDir::Down => "-v",
                };
                format!("{t} split-window {flag} -P -F '#{{pane_id}}' -t {}{}{}", quote(pane), cwd_arg(cwd), cmd_arg(command))
            }
            // Selecting a pane makes it the window's active pane; the window is selected too, so what the user picked is
            // what an attached client then shows.
            Op::SelectPane { pane } => format!("{t} select-window -t {p} && {t} select-pane -t {p}", p = quote(pane)),
            Op::SetPaneTitle { pane, title } => format!("{t} select-pane -t {} -T {}", quote(pane), text(title)),
            Op::ResizePane { pane, dir, amount } => {
                let f = match dir {
                    ResizeDir::Up => "-U",
                    ResizeDir::Down => "-D",
                    ResizeDir::Left => "-L",
                    ResizeDir::Right => "-R",
                };
                format!("{t} resize-pane -t {} {f} {amount}", quote(pane))
            }
            Op::ZoomPane { pane } => format!("{t} resize-pane -Z -t {}", quote(pane)),
            Op::KillPane { pane } => format!("{t} kill-pane -t {}", quote(pane)),
            Op::SetMouse { session, on } => format!("{t} set-option -t {} mouse {}", quote(session), if *on { "on" } else { "off" }),
        }
    }

    pub fn script(&self, socket: &Socket, nonce: &str, server: Option<&super::model::Server>) -> String {
        operation(socket, nonce, if self.needs_server() { server } else { None }, &self.command(socket))
    }
}

/// Did the operation do what it was meant to, judging by a reading taken afterwards? `created` is the id a creating
/// operation printed. `Err` carries what is wrong, so the UI says so instead of assuming.
pub fn verify(op: &Op, created: Option<&str>, after: &HostSnapshot) -> Result<(), String> {
    let session = |id: &str| after.sessions.iter().find(|s| s.id == id);
    let window = |id: &str| after.sessions.iter().flat_map(|s| &s.windows).find(|w| w.id == id);
    let pane = |id: &str| after.sessions.iter().flat_map(|s| &s.windows).flat_map(|w| &w.panes).find(|p| p.id == id);
    match op {
        Op::NewSession { name, .. } => {
            let id = created.ok_or("tmux did not report the new session.")?;
            let s = session(id).ok_or("The new session is not there.")?;
            match name {
                Some(n) if &s.name != n => Err(format!("The session was created as '{}', not '{}'.", s.name, n)),
                _ => Ok(()),
            }
        }
        Op::RenameSession { session: id, name } => match session(id) {
            Some(s) if &s.name == name => Ok(()),
            Some(s) => Err(format!("The session is still called '{}'.", s.name)),
            None => Err("The session no longer exists.".into()),
        },
        Op::KillSession { session: id } => if session(id).is_none() { Ok(()) } else { Err("The session is still running.".into()) },
        Op::NewWindow { .. } => {
            let id = created.ok_or("tmux did not report the new window.")?;
            window(id).map(|_| ()).ok_or_else(|| "The new window is not there.".into())
        }
        Op::RenameWindow { window: id, name } => match window(id) {
            Some(w) if &w.name == name => Ok(()),
            Some(w) => Err(format!("The window is still called '{}'.", w.name)),
            None => Err("The window no longer exists.".into()),
        },
        Op::SelectWindow { window: id } => match window(id) {
            Some(w) if w.active => Ok(()),
            Some(_) => Err("The window is not the active one.".into()),
            None => Err("The window no longer exists.".into()),
        },
        Op::KillWindow { window: id } => if window(id).is_none() { Ok(()) } else { Err("The window is still open.".into()) },
        Op::SplitPane { .. } => {
            let id = created.ok_or("tmux did not report the new pane.")?;
            pane(id).map(|_| ()).ok_or_else(|| "The new pane is not there.".into())
        }
        Op::SelectPane { pane: id } => match pane(id) {
            Some(p) if p.active => Ok(()),
            Some(_) => Err("The pane is not the active one.".into()),
            None => Err("The pane no longer exists.".into()),
        },
        Op::SetPaneTitle { pane: id, title } => match pane(id) {
            Some(p) if &p.title == title => Ok(()),
            Some(_) => Err("The pane kept its old title (a program may be setting it).".into()),
            None => Err("The pane no longer exists.".into()),
        },
        Op::ResizePane { pane: id, .. } | Op::ZoomPane { pane: id } => pane(id).map(|_| ()).ok_or_else(|| "The pane no longer exists.".into()),
        Op::KillPane { pane: id } => if pane(id).is_none() { Ok(()) } else { Err("The pane is still open.".into()) },
        Op::SetMouse { session: id, .. } => session(id).map(|_| ()).ok_or_else(|| "The session no longer exists.".into()),
    }
}
