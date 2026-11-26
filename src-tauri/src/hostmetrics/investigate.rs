//! "Why is this running?": who owns a process or a listening port, what started it, and which systemd unit manages it.
//!
//! One read-only script per question, run over the connection the engine already keeps. It reads `/proc` (`stat`,
//! `cmdline`, `status`, `cgroup`), `ss` for sockets and `systemctl show` for a unit the process's own cgroup names,
//! and nothing else: no `sudo`, no signal, no write. Everything that goes into the script is a number or a value from a
//! closed list, so there is no text of the user's to quote.
//!
//! Every line the host sends back as data is tagged by the script (`S `, `C `, `L `…), and only the script's own markers
//! start with `@@`. A process that names itself `@@END` or puts a newline in its name therefore cannot forge the frame.
//!
//! What is *observed* (the process, its parent chain, the cgroup it sits in) is kept apart from what is *concluded* (a
//! unit is said to manage a process only when the process's own cgroup is inside it; having PID 1 as an ancestor says
//! nothing). Anything that could not be read is a warning or a missing value, never a made-up one.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::model::Proto;
use super::parse::{ParseError, MAX_OUTPUT_BYTES};
use super::ports::{is_local_only, split_addr};

/// The most ancestors followed above a process.
pub const MAX_DEPTH: usize = 32;
/// The most processes investigated for one port.
const MAX_SUBJECTS: usize = 16;
const MAX_COMMAND: usize = 1000;
const MAX_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    Pid { pid: u32 },
    Port { port: u16, protocol: Proto },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetError {
    BadPid,
    BadPort,
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetError::BadPid => write!(f, "a process id is a whole number from 1 up"),
            TargetError::BadPort => write!(f, "a port is a whole number from 1 to 65535"),
        }
    }
}

impl Target {
    pub fn pid(pid: u64) -> Result<Target, TargetError> {
        u32::try_from(pid).ok().filter(|p| *p >= 1).map(|pid| Target::Pid { pid }).ok_or(TargetError::BadPid)
    }
    pub fn port(port: u64, protocol: Proto) -> Result<Target, TargetError> {
        u16::try_from(port).ok().filter(|p| *p >= 1).map(|port| Target::Port { port, protocol }).ok_or(TargetError::BadPort)
    }
}

/// The script. `Target`'s numbers are the only thing interpolated.
pub fn script(target: Target) -> String {
    let find = match target {
        Target::Pid { pid } => format!("pids={pid}\n"),
        Target::Port { port, protocol } => {
            let t = if protocol == Proto::Tcp { 't' } else { 'u' };
            format!(
                r#"echo "@@SS"
if command -v ss >/dev/null 2>&1; then
  echo "T ss"
  sockets=$(ss -l{t}npe 2>/dev/null | awk -v p={port} '{{ n = split($4, a, ":"); if (a[n] == p) print }}' | head -n 50)
  printf '%s\n' "$sockets" | sed 's/^/L /'
else
  echo "T none"; sockets=""
fi
pids=$(printf '%s\n' "$sockets" | grep -o 'pid=[0-9][0-9]*' | cut -d= -f2 | sort -un | head -n {MAX_SUBJECTS})
"#
            )
        }
    };
    format!(
        r#"LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin:/bin"; export PATH
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"
if [ "$os" = Linux ]; then
echo "@@CLK"; getconf CLK_TCK 2>/dev/null
echo "@@BTIME"; awk '/^btime/ {{ print $2 }}' /proc/stat 2>/dev/null
sd=no
echo "@@SYSTEMD"
if [ -d /run/systemd/system ]; then
  if command -v systemctl >/dev/null 2>&1; then sd=ok; echo ok; else echo no-systemctl; fi
else echo not-running; fi
{find}seen=" "; queried=" "
for pid in $pids; do
  p=$pid; d=0
  while [ "$p" -ge 1 ] 2>/dev/null && [ "$d" -lt {depth} ]; do
    case "$seen" in *" $p "*) break ;; esac
    seen="$seen$p "; d=$((d + 1))
    echo "@@P $p"
    line=$(tr '\n\r\000' '   ' < /proc/$p/stat 2>/dev/null | head -c 1024)
    if [ -z "$line" ]; then
      if [ -d /proc/$p ]; then echo "X denied"; else echo "X gone"; fi
      break
    fi
    printf 'S %s\n' "$line"
    printf 'C '; tr '\000\n\r' '\037  ' < /proc/$p/cmdline 2>/dev/null | head -c 4096; echo
    uid=$(awk '/^Uid:/ {{ print $2; exit }}' /proc/$p/status 2>/dev/null)
    printf 'U %s\n' "$uid"
    if [ -n "$uid" ]; then
      name=$(awk -F: -v u="$uid" '$3 == u {{ print $1; exit }}' /etc/passwd 2>/dev/null | head -c 64)
      printf 'O %s\n' "$name"
    fi
    sed 's/^/G /' /proc/$p/cgroup 2>/dev/null | head -n 20
    cg=$(awk -F: '($1 == "0" && $2 == "") || $2 == "name=systemd" {{ print $3; exit }}' /proc/$p/cgroup 2>/dev/null)
    unit=$(printf '%s\n' "$cg" | awk -F/ '{{ for (i = NF; i > 0; i--) if ($i ~ /\.(service|scope)$/) {{ print $i; exit }} }}')
    case "$unit" in "" | *[!A-Za-z0-9:_.@\\-]*) unit="" ;; esac
    if [ -n "$unit" ]; then
      printf 'N %s\n' "$unit"
      case "$cg" in */user@*) ;; *)
        if [ "$sd" = ok ]; then
          case "$queried" in *" $unit "*) ;; *)
            queried="$queried$unit "
            echo "@@UNIT $unit"
            systemctl show -p LoadState -p ActiveState -p SubState -p MainPID -- "$unit" 2>/dev/null | head -n 8 | sed 's/^/V /'
          ;; esac
        fi
      ;; esac
    fi
    rest=${{line##*) }}
    set -- $rest
    p=${{2:-0}}
  done
done
fi
echo "@@END"
"#,
        depth = MAX_DEPTH + 4,
    )
}

// ---------------------------------------------------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Clock ticks after boot at which it started (`/proc/<pid>/stat` field 22). Two processes with one pid differ here.
    pub start_ticks: Option<u64>,
    /// The same moment as milliseconds since 1970, when the host's boot time and clock rate were both read.
    pub started_ms: Option<u64>,
    pub name: String,
    /// The command line, arguments joined by a space. It can hold secrets: never logged, shown only on request.
    pub command: Option<String>,
    pub owner: Option<String>,
    pub uid: Option<u32>,
    /// `R`, `S`, `D`, `Z`, `T`… as the kernel reports it.
    pub state: Option<String>,
    /// The unit the process's own cgroup is inside, when it names one.
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AncestryEnd {
    /// The chain reached a process with no parent (PID 1, or a kernel thread's parent).
    Root,
    /// The next parent no longer exists.
    ParentGone { pid: u32 },
    /// The next parent exists but this user may not read it.
    ParentDenied { pid: u32 },
    /// The recorded parent started after its child: that pid was reused, so the real parent is gone.
    ParentReused { pid: u32 },
    /// The chain came back to a process already in it. Cannot happen on a healthy host.
    Cycle { pid: u32 },
    /// More than [`MAX_DEPTH`] levels; the rest is not followed.
    DepthLimit,
    /// The parent was not read at all (the host's answer was cut short).
    Unread { pid: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// systemd itself answered about the unit the cgroup names.
    Confirmed,
    /// Only the cgroup names it: systemd could not be asked.
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitType {
    Service,
    Scope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceAttribution {
    pub unit_name: String,
    pub unit_type: UnitType,
    /// `active (running)`, `inactive (dead)`… as systemd reported it; `None` when it was not asked.
    pub state: Option<String>,
    pub main_pid: Option<u32>,
    pub evidence: Vec<String>,
    pub confidence: Confidence,
}

/// One process of the answer, with what started it and what manages it. A port with several owners has several.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subject {
    pub process: ProcInfo,
    /// Root first, the process itself last. Only what was read: a missing parent is not invented.
    pub ancestry: Vec<ProcInfo>,
    pub ancestry_end: AncestryEnd,
    pub service: Option<ServiceAttribution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocketOwner {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocketInfo {
    pub protocol: Proto,
    pub state: String,
    pub address: String,
    pub port: u16,
    pub ipv6: bool,
    pub local_only: bool,
    pub owners: Vec<SocketOwner>,
    /// The user the socket belongs to, which `ss -e` shows even when it will not name the process.
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Found,
    ProcessNotFound,
    PortNotFound,
    /// Only Linux hosts are read.
    UnsupportedOs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    /// `owners_hidden`, `ss_missing`, `systemd_absent`, `systemd_no_systemctl`, `process_denied`.
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Investigation {
    pub host_id: Uuid,
    pub target: Target,
    pub status: Status,
    pub taken_at_ms: u64,
    /// The host's system name when it is not Linux.
    pub os: Option<String>,
    pub sockets: Vec<SocketInfo>,
    pub subjects: Vec<Subject>,
    pub warnings: Vec<Warning>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------------------------------

/// Text from a host as one line of printable characters.
fn clean(s: &str, max: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(max).collect::<String>().trim().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub name: String,
    pub state: String,
    pub ppid: u32,
    pub start: u64,
}

/// `1820 (my (odd) name) S 1250 …`: the name is everything between the first `(` and the last `)`.
pub fn parse_stat(line: &str) -> Option<(u32, Stat)> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = line[..open].trim().parse().ok()?;
    let rest: Vec<&str> = line[close + 1..].split_whitespace().collect();
    // state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime stime cutime cstime priority nice
    // threads itrealvalue starttime: the 20th after the name.
    Some((pid, Stat { name: clean(&line[open + 1..close], MAX_NAME), state: rest.first()?.to_string(), ppid: rest.get(1)?.parse().ok()?, start: rest.get(19)?.parse().ok()? }))
}

/// `a\x1fb\x1f` (NUL turned into 0x1f by the script) to `a b`. An empty command line (a kernel thread) is `None`.
pub fn parse_cmdline(raw: &str) -> Option<String> {
    let args: Vec<String> = raw.split('\u{1f}').map(|a| clean(a, MAX_COMMAND)).filter(|a| !a.is_empty()).collect();
    if args.is_empty() {
        return None;
    }
    Some(args.join(" ").chars().take(MAX_COMMAND).collect())
}

/// The unit a cgroup path is inside: its deepest `.service` or `.scope` part. `/user.slice/user-1000.slice/session-3.scope`
/// is `session-3.scope`; `/system.slice/ssh.service/` is `ssh.service`; `/` and `/user.slice` name none.
pub fn unit_of(cgroup_path: &str) -> Option<String> {
    cgroup_path.rsplit('/').find(|c| c.ends_with(".service") || c.ends_with(".scope")).map(str::to_string)
}

/// The cgroup path that systemd manages: `0::/…` on the unified hierarchy, or the `name=systemd` line on the old one.
fn systemd_path(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|l| {
        let mut p = l.splitn(3, ':');
        let (id, ctl, path) = (p.next()?, p.next()?, p.next()?);
        ((id == "0" && ctl.is_empty()) || ctl == "name=systemd").then(|| path.to_string())
    })
}

#[derive(Debug, Default)]
struct RawProc {
    stat: Option<(u32, Stat)>,
    absent: Option<bool>, // Some(true) = exists but unreadable, Some(false) = gone
    command: Option<String>,
    uid: Option<u32>,
    owner: Option<String>,
    cgroup: Vec<String>,
    unit: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct UnitProps {
    load: Option<String>,
    active: Option<String>,
    sub: Option<String>,
    main_pid: Option<u32>,
}

/// `users:(("node",pid=4231,fd=19),("node",pid=4232,fd=19))`
fn socket_owners(line: &str) -> Vec<SocketOwner> {
    let Some(at) = line.find("users:(") else { return Vec::new() };
    let mut out = Vec::new();
    for seg in line[at + 7..].split("),(") {
        let Some(i) = seg.rfind(",pid=") else { continue };
        let digits: String = seg[i + 5..].chars().take_while(char::is_ascii_digit).collect();
        let Ok(pid) = digits.parse::<u32>() else { continue };
        let name = seg[..i].trim_start_matches(['(', '"']).trim_end_matches('"');
        if out.len() < MAX_SUBJECTS && !out.iter().any(|o: &SocketOwner| o.pid == pid) {
            out.push(SocketOwner { pid, name: clean(name, MAX_NAME) });
        }
    }
    out
}

fn parse_socket(protocol: Proto, line: &str) -> Option<SocketInfo> {
    let t: Vec<&str> = line.split_whitespace().collect();
    // State Recv-Q Send-Q Local Peer [users:(…)] [uid:N] …
    if t.len() < 5 {
        return None;
    }
    let (address, port) = split_addr(t[3])?;
    Some(SocketInfo {
        protocol,
        state: clean(t[0], 16),
        ipv6: address.contains(':'),
        local_only: is_local_only(&address),
        address,
        port,
        owners: socket_owners(line),
        uid: t.iter().find_map(|x| x.strip_prefix("uid:")).and_then(|u| u.parse().ok()),
    })
}

fn to_info(pid: u32, r: &RawProc, clk: Option<f64>, btime: Option<u64>) -> Option<ProcInfo> {
    let (_, s) = r.stat.as_ref()?;
    let started_ms = match (clk, btime) {
        (Some(c), Some(b)) if c > 0.0 => Some(b * 1000 + (s.start as f64 / c * 1000.0) as u64),
        _ => None,
    };
    Some(ProcInfo {
        pid,
        ppid: Some(s.ppid),
        start_ticks: Some(s.start),
        started_ms,
        name: s.name.clone(),
        command: r.command.clone(),
        owner: r.owner.clone().filter(|o| !o.is_empty()),
        uid: r.uid,
        state: Some(s.state.clone()),
        unit: r.unit.clone(),
    })
}

/// Follows parents from `pid` through what was read. Root first.
pub fn build_ancestry(pid: u32, procs: &HashMap<u32, ProcInfo>, absent: &HashMap<u32, bool>) -> (Vec<ProcInfo>, AncestryEnd) {
    let mut chain: Vec<ProcInfo> = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = pid;
    let end = loop {
        let Some(p) = procs.get(&cur) else {
            break match absent.get(&cur) {
                Some(true) => AncestryEnd::ParentDenied { pid: cur },
                Some(false) => AncestryEnd::ParentGone { pid: cur },
                None => AncestryEnd::Unread { pid: cur },
            };
        };
        if !seen.insert(cur) {
            break AncestryEnd::Cycle { pid: cur };
        }
        if chain.len() >= MAX_DEPTH {
            break AncestryEnd::DepthLimit;
        }
        chain.push(p.clone());
        match p.ppid {
            None | Some(0) => break AncestryEnd::Root,
            Some(pp) if pp == cur => break AncestryEnd::Root,
            Some(pp) => {
                // A parent that started after its child is a pid that was handed out again.
                if let (Some(ps), Some(cs)) = (procs.get(&pp).and_then(|x| x.start_ticks), p.start_ticks) {
                    if ps > cs {
                        break AncestryEnd::ParentReused { pid: pp };
                    }
                }
                cur = pp;
            }
        }
    };
    chain.reverse();
    (chain, end)
}

fn attribute(p: &ProcInfo, cgroup: &[String], props: &HashMap<String, UnitProps>, systemd: &str) -> Option<ServiceAttribution> {
    let unit = p.unit.clone()?;
    let path = systemd_path(cgroup).unwrap_or_default();
    let unit_type = if unit.ends_with(".scope") { UnitType::Scope } else { UnitType::Service };
    let mut evidence = vec![format!("its cgroup is {path}")];
    let asked = props.get(&unit).filter(|p| p.load.as_deref() == Some("loaded"));
    let (state, main_pid, confidence) = match asked {
        Some(u) => {
            let state = match (&u.active, &u.sub) {
                (Some(a), Some(s)) => Some(format!("{a} ({s})")),
                (Some(a), None) => Some(a.clone()),
                _ => None,
            };
            evidence.push(format!("systemd reports {unit} as {}", state.as_deref().unwrap_or("loaded")));
            match u.main_pid {
                Some(m) if m == p.pid => evidence.push("it is the unit's main process".into()),
                Some(m) if m > 0 => evidence.push(format!("it runs inside the unit; the unit's main process is {m}")),
                _ => {}
            }
            (state, u.main_pid.filter(|m| *m > 0), Confidence::Confirmed)
        }
        None => {
            evidence.push(match systemd {
                "ok" if path.contains("/user@") => "this is inside a user's systemd instance, which was not asked".to_string(),
                "ok" => "systemd did not know a loaded unit by that name".to_string(),
                "no-systemctl" => "systemctl is not installed, so systemd was not asked".to_string(),
                _ => "systemd is not running here, so it was not asked".to_string(),
            });
            (None, None, Confidence::Inferred)
        }
    };
    Some(ServiceAttribution { unit_name: unit, unit_type, state, main_pid, evidence, confidence })
}

pub fn parse(host_id: Uuid, taken_at_ms: u64, target: Target, out: &str) -> Result<Investigation, ParseError> {
    if out.len() > MAX_OUTPUT_BYTES {
        return Err(ParseError::TooLarge);
    }
    if out.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    if !out.lines().any(|l| l.trim_end() == "@@END") {
        return Err(ParseError::Incomplete);
    }
    #[derive(PartialEq)]
    enum In {
        Nothing,
        Os,
        Clk,
        Btime,
        Systemd,
        Ss,
        Proc(u32),
        Unit(String),
    }
    let mut at = In::Nothing;
    let (mut os, mut clk, mut btime, mut systemd) = (String::new(), None, None, String::new());
    let (mut ss_tool, mut sockets) = (String::new(), Vec::new());
    let mut raws: HashMap<u32, RawProc> = HashMap::new();
    let mut order: Vec<u32> = Vec::new();
    let mut units: HashMap<String, UnitProps> = HashMap::new();
    let proto = match target {
        Target::Port { protocol, .. } => protocol,
        Target::Pid { .. } => Proto::Tcp,
    };
    for raw in out.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(m) = line.strip_prefix("@@") {
            at = match m.split_once(' ') {
                None => match m {
                    "OS" => In::Os,
                    "CLK" => In::Clk,
                    "BTIME" => In::Btime,
                    "SYSTEMD" => In::Systemd,
                    "SS" => In::Ss,
                    _ => In::Nothing,
                },
                Some(("P", n)) => match n.trim().parse::<u32>() {
                    Ok(pid) => {
                        if !raws.contains_key(&pid) {
                            order.push(pid);
                        }
                        raws.entry(pid).or_default();
                        In::Proc(pid)
                    }
                    Err(_) => In::Nothing,
                },
                Some(("UNIT", n)) => {
                    units.entry(n.trim().to_string()).or_default();
                    In::Unit(n.trim().to_string())
                }
                _ => In::Nothing,
            };
            continue;
        }
        match &at {
            In::Os if os.is_empty() => os = line.trim().to_string(),
            In::Clk if clk.is_none() => clk = line.trim().parse::<f64>().ok(),
            In::Btime if btime.is_none() => btime = line.trim().parse::<u64>().ok(),
            In::Systemd if systemd.is_empty() => systemd = line.trim().to_string(),
            In::Ss => {
                if let Some(t) = line.strip_prefix("T ") {
                    ss_tool = t.trim().to_string();
                } else if let Some(l) = line.strip_prefix("L ") {
                    sockets.extend(parse_socket(proto, l));
                }
            }
            In::Proc(pid) => {
                let r = raws.entry(*pid).or_default();
                let Some((tag, body)) = line.split_once(' ').or_else(|| (line.len() == 1).then_some((line, ""))) else { continue };
                match tag {
                    "S" => r.stat = parse_stat(body),
                    "X" => r.absent = Some(body.trim() == "denied"),
                    "C" => r.command = parse_cmdline(body),
                    "U" => r.uid = body.trim().parse().ok(),
                    "O" => r.owner = Some(clean(body, MAX_NAME)),
                    "G" => r.cgroup.push(body.trim().to_string()),
                    "N" => r.unit = Some(body.trim().to_string()),
                    _ => {}
                }
            }
            In::Unit(name) => {
                if let Some(v) = line.strip_prefix("V ") {
                    let u = units.entry(name.clone()).or_default();
                    match v.split_once('=') {
                        Some(("LoadState", x)) => u.load = Some(x.trim().to_string()),
                        Some(("ActiveState", x)) => u.active = Some(x.trim().to_string()),
                        Some(("SubState", x)) => u.sub = Some(x.trim().to_string()),
                        Some(("MainPID", x)) => u.main_pid = x.trim().parse().ok(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let mut res = Investigation { host_id, target, status: Status::Found, taken_at_ms, os: None, sockets: Vec::new(), subjects: Vec::new(), warnings: Vec::new() };
    if os != "Linux" {
        res.status = Status::UnsupportedOs;
        res.os = (!os.is_empty()).then_some(os);
        return Ok(res);
    }
    let warn = |res: &mut Investigation, code: &str, message: &str| res.warnings.push(Warning { code: code.into(), message: message.into() });
    match systemd.as_str() {
        "not-running" => warn(&mut res, "systemd_absent", "systemd is not running on this host, so no unit is named."),
        "no-systemctl" => warn(&mut res, "systemd_no_systemctl", "systemctl is not installed, so unit names come from the cgroup alone."),
        _ => {}
    }

    let mut procs: HashMap<u32, ProcInfo> = HashMap::new();
    let mut absent: HashMap<u32, bool> = HashMap::new();
    for pid in &order {
        let r = &raws[pid];
        match to_info(*pid, r, clk, btime) {
            Some(i) => {
                procs.insert(*pid, i);
            }
            None => {
                absent.insert(*pid, r.absent.unwrap_or(false));
            }
        }
    }

    let subject_pids: Vec<u32> = match target {
        Target::Pid { pid } => vec![pid],
        Target::Port { port, protocol } => {
            sockets.retain(|s: &SocketInfo| s.port == port && s.protocol == protocol);
            if ss_tool == "none" {
                warn(&mut res, "ss_missing", "`ss` is not installed on this host, so its sockets cannot be listed.");
            }
            if sockets.is_empty() {
                res.status = Status::PortNotFound;
            }
            let mut pids: Vec<u32> = Vec::new();
            for o in sockets.iter().flat_map(|s| &s.owners) {
                if !pids.contains(&o.pid) {
                    pids.push(o.pid);
                }
            }
            if !sockets.is_empty() && sockets.iter().any(|s| s.owners.is_empty()) {
                warn(&mut res, "owners_hidden", "Some sockets do not name their process: a user can only see their own processes without root.");
            }
            pids
        }
    };
    if let Target::Pid { pid } = target {
        if !procs.contains_key(&pid) {
            if absent.get(&pid) == Some(&true) {
                warn(&mut res, "process_denied", "That process exists, but this user may not read it.");
            }
            res.status = Status::ProcessNotFound;
        }
    }
    for pid in subject_pids {
        let Some(process) = procs.get(&pid).cloned() else { continue };
        let (ancestry, ancestry_end) = build_ancestry(pid, &procs, &absent);
        let service = attribute(&process, &raws[&pid].cgroup, &units, &systemd);
        res.subjects.push(Subject { process, ancestry, ancestry_end, service });
    }
    res.sockets = sockets;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, start: u64) -> ProcInfo {
        ProcInfo { pid, ppid: Some(ppid), start_ticks: Some(start), started_ms: None, name: format!("p{pid}"), command: None, owner: None, uid: None, state: None, unit: None }
    }

    fn stat_line(pid: u32, name: &str, ppid: u32, start: u64) -> String {
        format!("{pid} ({name}) S {ppid} 1 1 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 1 0 {start} 1000 100")
    }

    #[test]
    fn inputs_are_validated() {
        assert_eq!(Target::pid(0), Err(TargetError::BadPid));
        assert_eq!(Target::pid(4_294_967_296), Err(TargetError::BadPid));
        assert_eq!(Target::pid(4231), Ok(Target::Pid { pid: 4231 }));
        assert_eq!(Target::port(0, Proto::Tcp), Err(TargetError::BadPort));
        assert_eq!(Target::port(65536, Proto::Udp), Err(TargetError::BadPort));
        assert_eq!(Target::port(8080, Proto::Udp), Ok(Target::Port { port: 8080, protocol: Proto::Udp }));
    }

    #[test]
    fn the_script_holds_only_numbers_and_never_writes() {
        let s = script(Target::Port { port: 8080, protocol: Proto::Udp });
        assert!(s.contains("ss -lunpe") && s.contains("-v p=8080"));
        let s = script(Target::Pid { pid: 4231 });
        assert!(s.contains("pids=4231"));
        for bad in ["kill ", "sudo", "rm ", "systemctl restart", "systemctl stop", "> /proc", "chmod"] {
            assert!(!s.contains(bad), "{bad}");
        }
    }

    #[test]
    fn stat_names_with_spaces_and_parentheses_are_read() {
        let (pid, s) = parse_stat(&stat_line(1820, "my (odd) name", 1250, 9999)).unwrap();
        assert_eq!((pid, s.name.as_str(), s.state.as_str(), s.ppid, s.start), (1820, "my (odd) name", "S", 1250, 9999));
        assert!(parse_stat("garbage").is_none());
        assert!(parse_stat("12 (x) S").is_none());
    }

    #[test]
    fn command_lines_are_split_on_nul_and_cleaned() {
        assert_eq!(parse_cmdline("/usr/bin/node\u{1f}server.js\u{1f}").as_deref(), Some("/usr/bin/node server.js"));
        assert_eq!(parse_cmdline("a\u{1b}[31mb").as_deref(), Some("a [31mb"));
        assert_eq!(parse_cmdline(""), None);
        assert_eq!(parse_cmdline("\u{1f}\u{1f}"), None);
    }

    #[test]
    fn units_come_from_the_deepest_service_or_scope() {
        assert_eq!(unit_of("/system.slice/backend.service").as_deref(), Some("backend.service"));
        assert_eq!(unit_of("/user.slice/user-1000.slice/session-3.scope").as_deref(), Some("session-3.scope"));
        assert_eq!(unit_of("/user.slice/user-1000.slice/user@1000.service/app.slice/foo.service").as_deref(), Some("foo.service"));
        assert_eq!(unit_of("/"), None);
        assert_eq!(unit_of("/user.slice"), None);
    }

    #[test]
    fn ancestry_runs_root_first_and_stops_where_the_data_does() {
        let mut m = HashMap::new();
        for (a, b, c) in [(1, 0, 1), (824, 1, 10), (1250, 824, 20), (1280, 1250, 30)] {
            m.insert(a, p(a, b, c));
        }
        let (chain, end) = build_ancestry(1280, &m, &HashMap::new());
        assert_eq!(chain.iter().map(|x| x.pid).collect::<Vec<_>>(), [1, 824, 1250, 1280]);
        assert_eq!(end, AncestryEnd::Root);
        // A parent that was gone or unreadable.
        m.remove(&824);
        assert_eq!(build_ancestry(1280, &m, &HashMap::from([(824, false)])).1, AncestryEnd::ParentGone { pid: 824 });
        assert_eq!(build_ancestry(1280, &m, &HashMap::from([(824, true)])).1, AncestryEnd::ParentDenied { pid: 824 });
        assert_eq!(build_ancestry(1280, &m, &HashMap::new()).1, AncestryEnd::Unread { pid: 824 });
        assert_eq!(build_ancestry(1280, &m, &HashMap::new()).0.iter().map(|x| x.pid).collect::<Vec<_>>(), [1250, 1280]);
    }

    #[test]
    fn a_reused_pid_is_not_taken_for_the_parent() {
        let mut m = HashMap::new();
        m.insert(500, p(500, 700, 100)); // its recorded parent 700…
        m.insert(700, p(700, 1, 5000)); // …started long after it did.
        let (chain, end) = build_ancestry(500, &m, &HashMap::new());
        assert_eq!(chain.len(), 1);
        assert_eq!(end, AncestryEnd::ParentReused { pid: 700 });
    }

    #[test]
    fn cycles_and_long_chains_end() {
        let mut m = HashMap::new();
        m.insert(2, p(2, 3, 10));
        m.insert(3, p(3, 2, 10));
        assert_eq!(build_ancestry(2, &m, &HashMap::new()).1, AncestryEnd::Cycle { pid: 2 });
        let mut long = HashMap::new();
        for i in 2..200u32 {
            long.insert(i, p(i, i - 1, u64::from(i)));
        }
        long.insert(1, p(1, 0, 0));
        // Parents start earlier here, so only the depth stops it.
        let (chain, end) = build_ancestry(199, &long, &HashMap::new());
        assert_eq!((chain.len(), end), (MAX_DEPTH, AncestryEnd::DepthLimit));
    }

    #[test]
    fn socket_lines_v4_v6_and_shared_owners() {
        let a = parse_socket(Proto::Tcp, r#"LISTEN 0 511 0.0.0.0:8080 0.0.0.0:* users:(("node",pid=4231,fd=19),("node",pid=4232,fd=19)) uid:1000 ino:123 sk:1 cgroup:/x <->"#).unwrap();
        assert_eq!((a.address.as_str(), a.port, a.ipv6, a.local_only, a.uid), ("0.0.0.0", 8080, false, false, Some(1000)));
        assert_eq!(a.owners.iter().map(|o| o.pid).collect::<Vec<_>>(), [4231, 4232]);
        assert_eq!(a.owners[0].name, "node");
        let b = parse_socket(Proto::Tcp, "LISTEN 0 4096 [::1]:8080 [::]:* uid:0 ino:9").unwrap();
        assert_eq!((b.address.as_str(), b.ipv6, b.local_only, b.owners.len(), b.uid), ("::1", true, true, 0, Some(0)));
        let c = parse_socket(Proto::Tcp, r#"LISTEN 0 1 *:8080 *:* users:(("we,pid=1,ird",pid=77,fd=3))"#).unwrap();
        assert_eq!((c.owners[0].pid, c.owners[0].name.as_str()), (77, "we,pid=1,ird"));
        assert!(parse_socket(Proto::Tcp, "State Recv-Q").is_none());
    }

    fn proc_block(pid: u32, name: &str, ppid: u32, start: u64, extra: &str) -> String {
        format!("@@P {pid}\nS {}\nC /usr/bin/{name}\u{1f}--flag\nU 1000\nO appuser\n{extra}", stat_line(pid, name, ppid, start))
    }

    fn pid_out(blocks: &str) -> String {
        format!("@@OS\nLinux\n@@CLK\n100\n@@BTIME\n1700000000\n@@SYSTEMD\nok\n{blocks}@@END\n")
    }

    #[test]
    fn a_pid_is_traced_to_the_unit_its_cgroup_names() {
        let node = proc_block(4231, "node", 1, 5000, "G 0::/system.slice/backend.service\nN backend.service\n");
        let init = "@@P 1\nS 1 (systemd) S 0 1 1 0 -1 4194560 1 0 0 0 1 1 0 0 20 0 1 0 1 1 1\nC /sbin/init\nU 0\nO root\nG 0::/init.scope\nN init.scope\n";
        let unit = "@@UNIT backend.service\nV LoadState=loaded\nV ActiveState=active\nV SubState=running\nV MainPID=4231\n";
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 4231 }, &pid_out(&format!("{node}{unit}{init}"))).unwrap();
        assert_eq!(r.status, Status::Found);
        let s = &r.subjects[0];
        assert_eq!((s.process.name.as_str(), s.process.owner.as_deref(), s.process.command.as_deref()), ("node", Some("appuser"), Some("/usr/bin/node --flag")));
        assert_eq!(s.ancestry.iter().map(|x| x.pid).collect::<Vec<_>>(), [1, 4231]);
        assert_eq!(s.ancestry_end, AncestryEnd::Root);
        assert_eq!(s.process.started_ms, Some(1_700_000_000_000 + 50_000));
        let svc = s.service.as_ref().unwrap();
        assert_eq!((svc.unit_name.as_str(), svc.unit_type, svc.confidence, svc.state.as_deref()), ("backend.service", UnitType::Service, Confidence::Confirmed, Some("active (running)")));
        assert!(svc.evidence.iter().any(|e| e.contains("main process")), "{:?}", svc.evidence);
    }

    #[test]
    fn a_process_with_no_unit_in_its_cgroup_is_not_given_one() {
        // Its parent is PID 1, which says nothing about a service.
        let a = proc_block(900, "python", 1, 2000, "G 0::/user.slice/user-1000.slice/user@1000.service/app.slice/x\n");
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 900 }, &pid_out(&a)).unwrap();
        assert!(r.subjects[0].service.is_none());
        assert_eq!(r.subjects[0].ancestry_end, AncestryEnd::Unread { pid: 1 });
    }

    #[test]
    fn a_unit_systemd_does_not_know_is_only_inferred() {
        let a = proc_block(7, "x", 0, 1, "G 0::/system.slice/ghost.service\nN ghost.service\n");
        let u = "@@UNIT ghost.service\nV LoadState=not-found\nV ActiveState=inactive\n";
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 7 }, &pid_out(&format!("{a}{u}"))).unwrap();
        let svc = r.subjects[0].service.as_ref().unwrap();
        assert_eq!((svc.confidence, svc.state.clone()), (Confidence::Inferred, None));
    }

    #[test]
    fn a_missing_or_unreadable_process_is_said_so() {
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 9 }, &pid_out("@@P 9\nX gone\n")).unwrap();
        assert_eq!((r.status, r.subjects.len(), r.warnings.len()), (Status::ProcessNotFound, 0, 0));
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 9 }, &pid_out("@@P 9\nX denied\n")).unwrap();
        assert!(r.warnings.iter().any(|w| w.code == "process_denied"));
    }

    #[test]
    fn a_port_gives_each_owner_its_own_chain() {
        let ss = "@@SS\nT ss\nL LISTEN 0 511 0.0.0.0:8080 0.0.0.0:* users:((\"node\",pid=20,fd=3),(\"node\",pid=30,fd=3)) uid:1000\nL LISTEN 0 511 [::]:8080 [::]:* uid:1000\nL LISTEN 0 5 0.0.0.0:9090 0.0.0.0:*\n";
        let blocks = format!("{}{}{}", proc_block(20, "node", 10, 300, ""), proc_block(30, "node", 11, 400, ""), proc_block(10, "sh", 0, 100, "") + &proc_block(11, "sh", 0, 200, ""));
        let out = pid_out(&format!("{ss}{blocks}"));
        let r = parse(Uuid::nil(), 0, Target::Port { port: 8080, protocol: Proto::Tcp }, &out).unwrap();
        assert_eq!(r.sockets.len(), 2, "9090 is another port");
        assert!(r.sockets.iter().any(|s| s.ipv6) && r.sockets.iter().any(|s| !s.ipv6));
        assert_eq!(r.subjects.len(), 2);
        assert_eq!(r.subjects[0].ancestry.iter().map(|x| x.pid).collect::<Vec<_>>(), [10, 20]);
        assert_eq!(r.subjects[1].ancestry.iter().map(|x| x.pid).collect::<Vec<_>>(), [11, 30]);
        assert!(r.warnings.iter().any(|w| w.code == "owners_hidden"), "the v6 socket names no process");
    }

    #[test]
    fn a_port_with_no_socket_or_no_ss_is_reported() {
        let r = parse(Uuid::nil(), 0, Target::Port { port: 1, protocol: Proto::Tcp }, &pid_out("@@SS\nT ss\n")).unwrap();
        assert_eq!(r.status, Status::PortNotFound);
        let r = parse(Uuid::nil(), 0, Target::Port { port: 1, protocol: Proto::Tcp }, &pid_out("@@SS\nT none\n")).unwrap();
        assert!(r.warnings.iter().any(|w| w.code == "ss_missing"));
    }

    #[test]
    fn a_remote_peer_is_not_a_listener() {
        // Only the local column counts: a socket whose peer is :8080 is not listening on 8080.
        let ss = "@@SS\nT ss\nL ESTAB 0 0 10.0.0.1:50000 10.0.0.2:8080 users:((\"curl\",pid=5,fd=3))\n";
        let r = parse(Uuid::nil(), 0, Target::Port { port: 8080, protocol: Proto::Tcp }, &pid_out(ss)).unwrap();
        assert_eq!(r.status, Status::PortNotFound);
    }

    #[test]
    fn a_forged_marker_cannot_end_the_frame() {
        // The script tags every data line, so a process named "@@END" arrives as "S 5 (@@END) …".
        let a = proc_block(5, "@@END", 0, 1, "");
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 5 }, &pid_out(&a)).unwrap();
        assert_eq!(r.subjects[0].process.name, "@@END");
        assert_eq!(parse(Uuid::nil(), 0, Target::Pid { pid: 5 }, "@@OS\nLinux\n"), Err(ParseError::Incomplete));
    }

    #[test]
    fn other_systems_and_bad_answers() {
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 1 }, "@@OS\nDarwin\n@@END\n").unwrap();
        assert_eq!((r.status, r.os.as_deref()), (Status::UnsupportedOs, Some("Darwin")));
        assert_eq!(parse(Uuid::nil(), 0, Target::Pid { pid: 1 }, "  \n"), Err(ParseError::Empty));
        assert_eq!(parse(Uuid::nil(), 0, Target::Pid { pid: 1 }, &"x".repeat(MAX_OUTPUT_BYTES + 1)), Err(ParseError::TooLarge));
    }

    /// The real script on this machine, against this test's own process: the shell, `/proc`, `ss` and systemd as they are.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_script_reads_this_machines_own_process() {
        let pid = std::process::id();
        let out = std::process::Command::new("sh").arg("-c").arg(script(Target::Pid { pid })).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let r = parse(Uuid::nil(), 0, Target::Pid { pid }, &text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(r.status, Status::Found, "{text}");
        let s = &r.subjects[0];
        assert_eq!(s.process.pid, pid);
        assert_eq!(s.ancestry.last().map(|p| p.pid), Some(pid));
        assert!(s.process.command.is_some() && s.process.start_ticks.is_some() && s.process.started_ms.is_some());
        assert!(s.ancestry.len() >= 2, "{:?}", s.ancestry_end);
        // A pid that does not exist is reported as gone.
        let gone = std::process::Command::new("sh").arg("-c").arg(script(Target::Pid { pid: 4_194_000 })).output().unwrap();
        let r = parse(Uuid::nil(), 0, Target::Pid { pid: 4_194_000 }, &String::from_utf8_lossy(&gone.stdout)).unwrap();
        assert_eq!(r.status, Status::ProcessNotFound);
    }

    /// A real listener on a port is found, with this process as its owner.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_script_finds_a_real_listener() {
        if std::process::Command::new("sh").arg("-c").arg("command -v ss").output().map(|o| !o.status.success()).unwrap_or(true) {
            return;
        }
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = Target::Port { port: port.into(), protocol: Proto::Tcp };
        let out = std::process::Command::new("sh").arg("-c").arg(script(t)).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let r = parse(Uuid::nil(), 0, t, &text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(r.status, Status::Found, "{text}");
        assert!(r.sockets.iter().any(|s| s.port == port && s.local_only));
        assert!(r.subjects.iter().any(|s| s.process.pid == std::process::id()), "{text}");
        drop(l);
    }
}
