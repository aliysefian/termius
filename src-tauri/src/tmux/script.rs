//! The shell that talks to a host's tmux. Plain POSIX `sh` text, run on a channel of a connection that is already open;
//! it installs nothing, and everything that changes tmux is a separate, named operation (see `ops.rs`).
//!
//! **Reading what tmux prints.** Window names and pane titles are free text and tmux does not escape tabs or newlines in
//! them, so no delimiter a name could contain is safe: a window named `x\n@@sessions` would forge a line. Every run
//! therefore gets a random nonce (`@@<32 hex>`) that the host cannot know in advance, and both the section markers
//! and the field delimiters are built from it. A name cannot contain the nonce, so it cannot end its field or start a
//! section. See `parse.rs`.
//!
//! **Quoting.** Every value that reaches a command line is validated first (ids against tmux's own forms, names against
//! control characters) and then quoted with [`quote`]; none is ever spliced in bare.

use super::model::{Server, Socket};

/// A fresh nonce for one run.
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `'…'` with embedded single quotes closed, escaped and reopened: valid in sh, bash, zsh and fish.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `tmux` and the options that choose the server, ready to put before a command: `tmux -u -L work`.
pub fn tmux_cmd(socket: &Socket) -> String {
    match socket {
        Socket::Default => "tmux -u".to_string(),
        Socket::Name(n) => format!("tmux -u -L {}", quote(n)),
        Socket::Path(p) => format!("tmux -u -S {}", quote(p)),
    }
}

/// The marker that starts a section: `@@<nonce>:name` on a line of its own.
pub fn marker(nonce: &str, name: &str) -> String {
    format!("@@{nonce}:{name}")
}

/// What separates the fields of one record.
pub fn delim(nonce: &str) -> String {
    format!("@@{nonce}|")
}

/// A tmux format with the given variables as nonce-delimited fields: `D#{a}D#{b}D`.
pub fn record_format(nonce: &str, vars: &[&str]) -> String {
    let d = delim(nonce);
    let mut f = String::new();
    for v in vars {
        f.push_str(&d);
        f.push_str("#{");
        f.push_str(v);
        f.push('}');
    }
    f.push_str(&d);
    f
}

pub const SESSION_VARS: &[&str] = &[
    "session_id",
    "session_name",
    "session_attached",
    "session_created",
    "session_activity",
    "session_group",
    "pid",
    "start_time",
    "socket_path",
    "mouse",
];

pub const PANE_VARS: &[&str] = &[
    "session_id",
    "window_id",
    "window_index",
    "window_name",
    "window_active",
    "window_layout",
    "window_zoomed_flag",
    "window_activity_flag",
    "window_bell_flag",
    "pane_id",
    "pane_index",
    "pane_active",
    "pane_title",
    "pane_current_command",
    "pane_current_path",
    "pane_pid",
    "pane_width",
    "pane_height",
    "pane_dead",
    "pane_in_mode",
];

pub const CAPTURE_VARS: &[&str] = &[
    "pane_id",
    "pane_width",
    "pane_height",
    "pane_current_command",
    "pane_dead",
    "alternate_on",
    "cursor_x",
    "cursor_y",
    "pane_in_mode",
    "history_size",
    "pane_pid",
    "pane_title",
];

const HEAD: &str = r#"PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:/snap/bin"; export PATH
"#;

/// The check that the server answering is the one the caller's ids came from. A restarted tmux hands out `$0`, `@0` and
/// `%0` again, so an id without its server is a different object on a different day.
fn guard(socket: &Socket, nonce: &str, expect: Option<&Server>) -> String {
    let Some(server) = expect else { return String::new() };
    let t = tmux_cmd(socket);
    format!(
        r#"cur=$({t} list-sessions -F '#{{pid}}:#{{start_time}}' 2>/dev/null | head -n 1)
case $cur in *:) cur="${{cur}}0";; esac
if [ "$cur" != {tok} ]; then echo '{stale}'; echo '{end}'; exit 0; fi
"#,
        tok = quote(&server.token()),
        stale = marker(nonce, "stale"),
        end = marker(nonce, "end"),
    )
}

/// Everything needed to draw the tree, in one run: whether tmux is there, its version, the sessions, every pane (with its
/// window's fields) and the other sockets in the default folder.
pub fn probe(socket: &Socket, nonce: &str) -> String {
    let t = tmux_cmd(socket);
    let m = |name: &str| marker(nonce, name);
    let sessions_fmt = quote(&record_format(nonce, SESSION_VARS));
    let panes_fmt = quote(&record_format(nonce, PANE_VARS));
    format!(
        r#"{HEAD}echo '{os}'
uname -s 2>/dev/null
( . /etc/os-release 2>/dev/null && printf '%s|%s|%s\n' "$ID" "$ID_LIKE" "$PRETTY_NAME" ) 2>/dev/null
if ! command -v tmux >/dev/null 2>&1; then echo '{missing}'; echo '{end}'; exit 0; fi
echo '{version}'; tmux -V 2>&1
echo '{sockets}'
d="${{TMUX_TMPDIR:-/tmp}}/tmux-$(id -u 2>/dev/null)"
for f in "$d"/*; do [ -S "$f" ] && printf '%s\n' "${{f##*/}}"; done 2>/dev/null
s=$({t} list-sessions -F {sessions_fmt} 2>&1); rc=$?
echo '{src}'; echo "$rc"
echo '{sessions}'; printf '%s\n' "$s"
if [ "$rc" = 0 ]; then
  p=$({t} list-panes -a -F {panes_fmt} 2>&1); prc=$?
  echo '{prc_m}'; echo "$prc"
  echo '{panes}'; printf '%s\n' "$p"
fi
echo '{end}'
"#,
        os = m("os"),
        missing = m("missing"),
        version = m("version"),
        sockets = m("sockets"),
        src = m("sessions-rc"),
        sessions = m("sessions"),
        prc_m = m("panes-rc"),
        panes = m("panes"),
        end = m("end"),
    )
}

/// How much of a pane to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOpts {
    /// Keep colours and attributes as escape sequences.
    pub ansi: bool,
    /// Join wrapped lines back into one.
    pub join: bool,
    /// Lines of history above the visible screen; 0 is the screen only.
    pub scrollback: u32,
}

/// The most history one capture may take, and the most bytes it may return.
pub const MAX_SCROLLBACK: u32 = 5000;
pub const MAX_CAPTURE_BYTES: usize = 512 * 1024;

pub fn capture(socket: &Socket, nonce: &str, pane: &str, opts: CaptureOpts, expect: Option<&Server>) -> String {
    debug_assert!(super::model::valid_pane_id(pane));
    let t = tmux_cmd(socket);
    let m = |name: &str| marker(nonce, name);
    let info_fmt = quote(&record_format(nonce, CAPTURE_VARS));
    let mut flags = String::from("-p");
    if opts.ansi {
        flags.push_str(" -e");
    }
    if opts.join {
        flags.push_str(" -J");
    }
    let scroll = opts.scrollback.min(MAX_SCROLLBACK);
    if scroll > 0 {
        flags.push_str(&format!(" -S -{scroll}"));
    }
    format!(
        r#"{HEAD}{guard}echo '{info}'
{t} display-message -p -t {pane} {info_fmt} 2>&1
rc=$?
echo '{rc}'; echo "$rc"
b=$({t} capture-pane {flags} -t {pane} 2>&1); crc=$?
echo '{crc_m}'; echo "$crc"
echo '{body}'; printf '%s' "$b" | head -c {max}; echo
echo '{end}'
"#,
        guard = guard(socket, nonce, expect),
        info = m("info"),
        pane = quote(pane),
        rc = m("info-rc"),
        crc_m = m("capture-rc"),
        body = m("body"),
        end = m("end"),
        max = MAX_CAPTURE_BYTES,
    )
}

/// Wrap one mutating command: check the server identity, run it, print its exit status and what it said.
pub fn operation(socket: &Socket, nonce: &str, expect: Option<&Server>, body: &str) -> String {
    let m = |name: &str| marker(nonce, name);
    format!(
        r#"{HEAD}{guard}out=$({body} 2>&1); rc=$?
echo '{result}'; echo "$rc"
echo '{output}'; printf '%s\n' "$out" | head -c 8192
echo '{end}'
"#,
        guard = guard(socket, nonce, expect),
        result = m("result"),
        output = m("output"),
        end = m("end"),
    )
}

/// Find the tmux client that was started by the tab marked `tab` (the attach command exports `SSHVAULT_TAB`) and move
/// *that* client to `session`. Nothing is typed into any terminal, so it works whatever the terminal is running; and it
/// moves the right client, not "the most recent one".
pub fn switch_client(socket: &Socket, nonce: &str, expect: Option<&Server>, tab: &str, session: &str) -> String {
    let t = tmux_cmd(socket);
    let m = |name: &str| marker(nonce, name);
    format!(
        r#"{HEAD}{guard}list=$({t} list-clients -F '#{{client_pid}} #{{client_name}}' 2>/dev/null)
found=
IFS='
'
for l in $list; do
  pid=${{l%% *}}; name=${{l#* }}
  if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx {marker_eq}; then found=$name; break; fi
done
if [ -z "$found" ]; then echo '{notatt}'; echo '{end}'; exit 0; fi
out=$({t} switch-client -c "$found" -t {session} 2>&1); rc=$?
now=$({t} display-message -c "$found" -p '#{{session_id}}' 2>/dev/null)
echo '{result}'; echo "$rc"
echo '{now_m}'; echo "$now"
echo '{output}'; printf '%s\n' "$out" | head -c 4096
echo '{end}'
"#,
        guard = guard(socket, nonce, expect),
        marker_eq = quote(&format!("SSHVAULT_TAB={tab}")),
        session = quote(session),
        notatt = m("notattached"),
        result = m("result"),
        now_m = m("now"),
        output = m("output"),
        end = m("end"),
    )
}

/// The command to type into a terminal tab's shell to attach to `session`. The tab's own shell is the right place for it
/// (an interactive attach needs the tab's terminal), and it is written to be pasted into any shell: it is one `sh -c`
/// with every value quoted, so bash, zsh, fish and csh all read it the same way.
///
/// * Inside tmux on **this** server, `switch-client` moves the client the shell is running in; attaching would be refused.
/// * Inside tmux on another server, nothing is nested: it says so and stops.
/// * The server is checked against `server` first, so ids from a snapshot of a server that has since restarted do not
///   attach to whatever now has that number.
pub fn attach_command(socket: &Socket, session: &str, server: Option<&Server>, tab: &str, detach_others: bool) -> String {
    debug_assert!(super::model::valid_session_id(session));
    let t = tmux_cmd(socket);
    let env = format!("env SSHVAULT_TAB={}", quote(tab));
    let mut inner = String::new();
    if let Some(s) = server {
        inner.push_str(&format!(
            "cur=$({t} list-sessions -F '#{{pid}}:#{{start_time}}' 2>/dev/null | head -n 1); case $cur in *:) cur=\"${{cur}}0\";; esac; \
             if [ \"$cur\" != {tok} ]; then echo 'SSHVault: the tmux server on this host has changed since the list was read. Refresh it and try again.'; exit 1; fi; ",
            tok = quote(&s.token()),
        ));
        inner.push_str(&format!(
            "if [ -n \"$TMUX\" ]; then if [ \"${{TMUX%%,*}}\" = {sp} ]; then exec {env} {t} switch-client -t {sid}; else echo 'SSHVault: this shell is already inside another tmux server. Detach from it first, or open a new tab.'; exit 1; fi; fi; ",
            sp = quote(&s.socket_path),
            sid = quote(session),
        ));
    } else {
        inner.push_str("if [ -n \"$TMUX\" ]; then echo 'SSHVault: this shell is already inside tmux. Detach from it first, or open a new tab.'; exit 1; fi; ");
    }
    let d = if detach_others { " -d" } else { "" };
    inner.push_str(&format!("exec {env} {t} attach-session{d} -t {sid}", sid = quote(session)));
    format!("sh -c {}", quote(&inner))
}

/// `tmux new-session` for a terminal tab: attach to `name` if it exists, else create it, and never nest. Used by Auto
/// Tmux when a connection opens.
pub fn auto_command(socket: &Socket, name: &str, create_if_missing: bool, always_new: bool, mouse: bool, tab: &str) -> String {
    let t = tmux_cmd(socket);
    let env = format!("env SSHVAULT_TAB={}", quote(tab));
    let q = quote(name);
    let mut inner = String::from("if [ -n \"$TMUX\" ]; then exit 0; fi; ");
    inner.push_str("if ! command -v tmux >/dev/null 2>&1; then echo 'SSHVault: tmux is not installed on this host; using the normal shell.'; exit 0; fi; ");
    // `=name` is an exact match: plain `name` also matches a session whose name merely starts with it.
    let exact = quote(&format!("={name}"));
    let mouse_flag = if mouse { format!(" \\; set-option -t {exact} mouse on") } else { String::new() };
    if always_new {
        // A new session each time; the name is only a prefix, so it never collides with an existing one.
        inner.push_str(&format!("exec {env} {t} new-session"));
    } else if create_if_missing {
        inner.push_str(&format!("exec {env} {t} new-session -A -s {q}{mouse_flag}"));
    } else {
        inner.push_str(&format!("if {t} has-session -t {exact} 2>/dev/null; then exec {env} {t} attach-session -t {exact}{mouse_flag}; fi; echo 'SSHVault: no tmux session called {} was found; using the normal shell.'; exit 0", name.replace('\'', "")));
    }
    format!("sh -c {}", quote(&inner))
}
