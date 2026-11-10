//! The probe's output as a [`HostSnapshot`]'s content. Strict about structure and forgiving about content: a record
//! with the wrong number of fields, or output with no end marker, is an error and never a shorter list; names with
//! tabs, newlines, pipes, quotes or Unicode are just names.

use std::collections::HashMap;

use super::model::{clean, valid_pane_id, valid_session_id, valid_window_id, Pane, Server, ServerState, Session, Socket, Version, Window};
use super::script::{delim, marker, PANE_VARS, SESSION_VARS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No end marker: the run was cut short (a timeout, a dropped connection, output over the limit).
    Truncated,
    /// A record does not have the shape the format promised.
    Malformed(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Truncated => write!(f, "the host's answer was cut short"),
            ParseError::Malformed(m) => write!(f, "the host's answer was not understood: {m}"),
        }
    }
}

/// The sections of one run, by name. Each starts at a marker line carrying the run's nonce.
pub fn sections<'a>(out: &'a str, nonce: &str) -> Result<HashMap<String, &'a str>, ParseError> {
    let prefix = marker(nonce, "");
    let mut found: Vec<(String, usize, usize)> = Vec::new();
    let mut at = 0;
    while let Some(i) = out[at..].find(&prefix) {
        let start = at + i;
        // A marker is on a line of its own.
        let at_line_start = start == 0 || out.as_bytes()[start - 1] == b'\n';
        let name_start = start + prefix.len();
        let line_end = out[name_start..].find('\n').map(|n| name_start + n).unwrap_or(out.len());
        if at_line_start {
            found.push((out[name_start..line_end].trim_end_matches('\r').to_string(), start, (line_end + 1).min(out.len())));
        }
        at = line_end.max(start + 1);
    }
    if !found.iter().any(|(n, ..)| n == "end") {
        return Err(ParseError::Truncated);
    }
    let mut map = HashMap::new();
    for (i, (name, _, body_start)) in found.iter().enumerate() {
        let body_end = found.get(i + 1).map(|(_, s, _)| *s).unwrap_or(out.len());
        let body = &out[(*body_start).min(body_end)..body_end];
        map.insert(name.clone(), body.strip_suffix('\n').unwrap_or(body));
    }
    Ok(map)
}

/// Records of `k` fields, as `D f1 D f2 … D fk D`, one per line (but a field may contain newlines itself).
pub fn records(body: &str, nonce: &str, k: usize) -> Result<Vec<Vec<String>>, ParseError> {
    let d = delim(nonce);
    let body = body.trim_matches(|c| c == '\n' || c == '\r');
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let pieces: Vec<&str> = body.split(d.as_str()).collect();
    // ["", f1..fk, "\n", f1..fk, "\n", …, ""]: a leading empty piece, then groups of k fields each followed by a separator.
    if pieces[0].trim() != "" {
        return Err(ParseError::Malformed("text before the first record".into()));
    }
    let rest = &pieces[1..];
    // Every record is its k fields and then the separator that follows it ("\n", or nothing after the last record), so the
    // pieces come in groups of k + 1. Any other length means a record is short or long; it is never made to fit.
    if !rest.len().is_multiple_of(k + 1) {
        return Err(ParseError::Malformed(format!("a record does not have {k} fields")));
    }
    let mut out = Vec::new();
    for group in rest.chunks_exact(k + 1) {
        if group[k].trim() != "" {
            return Err(ParseError::Malformed("a record has more fields than expected".into()));
        }
        out.push(group[..k].iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
    Ok(out)
}

fn num(s: &str) -> u64 {
    s.trim().parse().unwrap_or(0)
}
fn flag(s: &str) -> bool {
    s.trim() == "1"
}

/// What a run found, before it becomes a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Probed {
    pub version: Option<Version>,
    pub state: ServerState,
    pub server: Option<Server>,
    pub sessions: Vec<Session>,
    pub other_sockets: Vec<String>,
}

pub fn parse_probe(out: &str, nonce: &str, socket: &Socket) -> Result<Probed, ParseError> {
    let sec = sections(out, nonce)?;
    let os_text = sec.get("os").copied().unwrap_or("");
    let mut os_lines = os_text.lines();
    let kernel = os_lines.next().unwrap_or("").trim().to_string();
    let distro = os_lines.next().unwrap_or("").trim().to_string();
    let os = if distro.is_empty() { kernel } else { format!("{kernel}|{distro}") };

    if sec.contains_key("missing") {
        return Ok(Probed { version: None, state: ServerState::NotInstalled { os: clean(&os) }, server: None, sessions: vec![], other_sockets: vec![] });
    }
    let version = sec.get("version").and_then(|v| Version::parse(v.lines().next().unwrap_or("")));
    let other_sockets = sec
        .get("sockets")
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|n| !n.is_empty() && Socket::Name(n.clone()).validate().is_ok())
                .filter(|n| !(n == "default" && *socket == Socket::Default) && !(*socket == Socket::Name(n.clone())))
                .take(32)
                .collect()
        })
        .unwrap_or_default();

    let rc: i32 = sec.get("sessions-rc").map(|s| s.trim().parse().unwrap_or(-1)).unwrap_or(-1);
    let sessions_body = sec.get("sessions").copied().unwrap_or("");
    if rc != 0 {
        let state = classify_error(sessions_body, version.as_ref());
        return Ok(Probed { version, state, server: None, sessions: vec![], other_sockets });
    }
    if let Some(v) = &version {
        if !v.supported() {
            return Ok(Probed { version: Some(v.clone()), state: ServerState::Unsupported { version: clean(&v.raw) }, server: None, sessions: vec![], other_sockets });
        }
    }
    let prc: i32 = sec.get("panes-rc").map(|s| s.trim().parse().unwrap_or(-1)).unwrap_or(-1);
    if prc != 0 {
        // The server answered the first question and not the second: it went away in between. That is a failure to
        // read, not an empty server.
        return Err(ParseError::Malformed(format!("tmux stopped answering: {}", clean(sec.get("panes").copied().unwrap_or("").trim()))));
    }

    let srows = records(sessions_body, nonce, SESSION_VARS.len())?;
    let prows = records(sec.get("panes").copied().unwrap_or(""), nonce, PANE_VARS.len())?;

    let mut server: Option<Server> = None;
    let mut sessions: Vec<Session> = Vec::new();
    for r in &srows {
        if !valid_session_id(&r[0]) {
            return Err(ParseError::Malformed(format!("session id {:?}", r[0])));
        }
        if server.is_none() {
            server = Some(Server { pid: num(&r[6]) as u32, started: num(&r[7]), socket_path: clean(&r[8]) });
        }
        sessions.push(Session {
            id: r[0].clone(),
            name: clean(&r[1]),
            attached: num(&r[2]) as u32,
            created: num(&r[3]),
            activity: num(&r[4]),
            group: Some(clean(&r[5])).filter(|g| !g.is_empty()),
            windows: Vec::new(),
        });
    }
    let by_id: HashMap<String, usize> = sessions.iter().enumerate().map(|(i, s)| (s.id.clone(), i)).collect();
    for r in &prows {
        let (sid, wid, pid_) = (&r[0], &r[1], &r[9]);
        if !valid_session_id(sid) || !valid_window_id(wid) || !valid_pane_id(pid_) {
            return Err(ParseError::Malformed(format!("ids {sid:?} {wid:?} {pid_:?}")));
        }
        // A pane of a session created after the session list was read: the next reading has it whole.
        let Some(&si) = by_id.get(sid) else { continue };
        let pane = Pane {
            id: pid_.clone(),
            index: num(&r[10]) as u32,
            active: flag(&r[11]),
            title: clean(&r[12]),
            command: clean(&r[13]),
            path: clean(&r[14]),
            pid: r[15].trim().parse().ok(),
            width: num(&r[16]) as u32,
            height: num(&r[17]) as u32,
            dead: flag(&r[18]),
            in_mode: flag(&r[19]),
        };
        let s = &mut sessions[si];
        if let Some(w) = s.windows.iter_mut().find(|w| &w.id == wid) {
            w.panes.push(pane);
        } else {
            s.windows.push(Window {
                id: wid.clone(),
                index: num(&r[2]) as u32,
                name: clean(&r[3]),
                active: flag(&r[4]),
                layout: clean(&r[5]),
                zoomed: flag(&r[6]),
                activity: flag(&r[7]),
                bell: flag(&r[8]),
                panes: vec![pane],
            });
        }
    }
    for s in &mut sessions {
        s.windows.sort_by_key(|w| w.index);
        for w in &mut s.windows {
            w.panes.sort_by_key(|p| p.index);
        }
    }
    sessions.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let state = if sessions.is_empty() { ServerState::Empty } else { ServerState::Ready };
    Ok(Probed { version, state, server, sessions, other_sockets })
}

/// What `tmux list-sessions` said when it failed, as a state.
pub fn classify_error(text: &str, version: Option<&Version>) -> ServerState {
    let t = text.trim();
    let lower = t.to_lowercase();
    if lower.contains("permission denied") || lower.contains("operation not permitted") {
        return ServerState::PermissionDenied { message: clean(&t.chars().take(300).collect::<String>()) };
    }
    if lower.contains("no server running") || lower.contains("no such file or directory") || lower.contains("connection refused") || lower.contains("failed to connect") || lower.contains("error connecting") && lower.contains("no such file") {
        // An old tmux asked for sessions it does not know about says so the same way; the version tells them apart.
        if let Some(v) = version {
            if !v.supported() {
                return ServerState::Unsupported { version: clean(&v.raw) };
            }
        }
        return ServerState::NoServer;
    }
    if lower.contains("unknown option") || lower.contains("unknown command") || lower.contains("usage:") {
        return ServerState::Unsupported { version: clean(&version.map(|v| v.raw.clone()).unwrap_or_default()) };
    }
    ServerState::Failed { message: clean(&t.chars().take(300).collect::<String>()) }
}

/// An operation's answer: `Ok(output)` or the reason it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpOutcome {
    Done { output: String },
    Failed { message: String },
    /// The server that answered is not the one the ids came from.
    Stale,
}

pub fn parse_operation(out: &str, nonce: &str) -> Result<OpOutcome, ParseError> {
    let sec = sections(out, nonce)?;
    if sec.contains_key("stale") {
        return Ok(OpOutcome::Stale);
    }
    let rc: i32 = sec.get("result").map(|s| s.trim().parse().unwrap_or(-1)).ok_or_else(|| ParseError::Malformed("no result".into()))?;
    let output = clean(sec.get("output").copied().unwrap_or("").trim());
    Ok(if rc == 0 { OpOutcome::Done { output } } else { OpOutcome::Failed { message: if output.is_empty() { format!("tmux exited with status {rc}") } else { output } } })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub command: String,
    pub dead: bool,
    /// The program is using the alternate screen (vim, less, htop): the scrollback is not its output.
    pub alternate: bool,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub in_mode: bool,
    pub history: u32,
    pub pid: Option<u32>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    Pane { info: PaneInfo, text: String },
    Failed { message: String },
    Stale,
}

pub fn parse_capture(out: &str, nonce: &str) -> Result<Captured, ParseError> {
    let sec = sections(out, nonce)?;
    if sec.contains_key("stale") {
        return Ok(Captured::Stale);
    }
    let rc: i32 = sec.get("info-rc").map(|s| s.trim().parse().unwrap_or(-1)).unwrap_or(-1);
    let crc: i32 = sec.get("capture-rc").map(|s| s.trim().parse().unwrap_or(-1)).unwrap_or(-1);
    if rc != 0 || crc != 0 {
        let said = if rc != 0 { sec.get("info").copied().unwrap_or("") } else { sec.get("body").copied().unwrap_or("") };
        return Ok(Captured::Failed { message: clean(said.trim()) });
    }
    let rows = records(sec.get("info").copied().unwrap_or(""), nonce, super::script::CAPTURE_VARS.len())?;
    let r = rows.first().ok_or_else(|| ParseError::Malformed("no pane information".into()))?;
    let info = PaneInfo {
        id: r[0].clone(),
        width: num(&r[1]) as u32,
        height: num(&r[2]) as u32,
        command: clean(&r[3]),
        dead: flag(&r[4]),
        alternate: flag(&r[5]),
        cursor_x: num(&r[6]) as u32,
        cursor_y: num(&r[7]) as u32,
        in_mode: flag(&r[8]),
        history: num(&r[9]) as u32,
        pid: r[10].trim().parse().ok(),
        title: clean(&r[11]),
    };
    let body = sec.get("body").copied().unwrap_or("");
    // `head -c` may have cut a character in half.
    Ok(Captured::Pane { info, text: body.trim_end_matches('\n').to_string() })
}
