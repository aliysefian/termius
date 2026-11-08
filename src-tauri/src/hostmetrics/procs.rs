//! The process list, and signalling a process.
//!
//! The list is read when a view asks for it (it is far bigger than a sample), from `/proc/<pid>/stat` for everything but
//! the command line, which comes from `ps` when the host has it. CPU use is the change in a process's own CPU time over
//! the host's clock, so the first list has none and says why, unless it was asked to take two readings a second apart.
//! A list is bounded: the busiest 500, however many processes the host has.
//!
//! Signalling a process checks, on the host and in the same command, that the pid is still the process that was on
//! screen (same name and same start time) before sending anything.

use std::collections::HashMap;

use uuid::Uuid;

use super::derive::{baseline, delta};
use super::model::{Baseline, ProcessInfo, ProcessList};
use super::parse::{f, sections, u, ParseError, MAX_OUTPUT_BYTES};

/// The most processes returned. A host with more has the rest counted in `total`.
pub const MAX_ITEMS: usize = 500;
/// A command line is cut to this many characters.
const MAX_COMMAND: usize = 200;

pub const SCRIPT: &str = r#"LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"
if [ "$os" = Linux ]; then
  echo "@@CLK"; getconf CLK_TCK 2>/dev/null
  echo "@@PAGE"; getconf PAGESIZE 2>/dev/null
  psnap() { echo "@@UP$1"; cat /proc/uptime 2>/dev/null; echo "@@STAT$1"; cat /proc/[0-9]*/stat 2>/dev/null; }
  psnap A
  if [ "$TWICE" = 1 ]; then sleep 1; psnap B; fi
  echo "@@MEMTOTAL"; grep '^MemTotal:' /proc/meminfo 2>/dev/null
  echo "@@PASSWD"; cat /etc/passwd 2>/dev/null
  echo "@@UIDS"; cat /proc/[0-9]*/status 2>/dev/null | awk '/^Pid:/ { p = $2 } /^Uid:/ { print p, $2 }'
  echo "@@ARGS"
  if command -v ps >/dev/null 2>&1; then
    { ps -eww -o pid=,args= 2>/dev/null || ps -o pid=,args= 2>/dev/null; } | cut -c1-260
  fi
fi
echo "@@END"
"#;

/// The script, asking for two readings a second apart when there is no earlier one to compare with.
pub fn script(two_readings: bool) -> String {
    if two_readings {
        format!("TWICE=1; export TWICE\n{SCRIPT}")
    } else {
        SCRIPT.to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub name: String,
    pub state: String,
    pub ppid: u32,
    /// User plus system CPU time, in clock ticks.
    pub ticks: u64,
    pub threads: u32,
    pub start: u64,
    pub rss_pages: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snap {
    pub uptime: f64,
    pub procs: HashMap<u32, Stat>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcRaw {
    pub os: String,
    pub clk: f64,
    pub page: u64,
    pub mem_total_kb: Option<u64>,
    /// The earlier reading of a two-reading run.
    pub earlier: Option<Snap>,
    pub now: Snap,
    pub users: HashMap<u32, String>,
    pub uids: HashMap<u32, u32>,
    pub args: HashMap<u32, String>,
}

/// One line of `/proc/<pid>/stat`. The name is in parentheses and may hold spaces and parentheses itself, so it is cut at
/// the first `(` and the last `)`.
fn stat_line(l: &str) -> Option<(u32, Stat)> {
    let open = l.find('(')?;
    let close = l.rfind(')')?;
    if close <= open {
        return None;
    }
    let pid = l[..open].trim().parse::<u32>().ok()?;
    let rest: Vec<&str> = l[close + 1..].split_whitespace().collect();
    // state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime stime cutime cstime priority nice
    // num_threads itrealvalue starttime vsize rss
    if rest.len() < 22 {
        return None;
    }
    let n = |i: usize| rest[i].parse::<u64>().ok();
    Some((
        pid,
        Stat {
            name: l[open + 1..close].chars().take(40).collect(),
            state: rest[0].chars().take(1).collect(),
            ppid: u32::try_from(n(1)?).ok()?,
            ticks: n(11)?.saturating_add(n(12)?),
            threads: u32::try_from(n(17)?).ok()?,
            start: n(19)?,
            rss_pages: n(21)?,
        },
    ))
}

fn snap(uptime: &[&str], stat: &[&str]) -> Option<Snap> {
    let uptime = uptime.first().and_then(|l| f(l.split_whitespace().next()?))?;
    // A hundred thousand processes is not a host this list is for.
    let procs: HashMap<u32, Stat> = stat.iter().filter_map(|l| stat_line(l)).take(100_000).collect();
    Some(Snap { uptime, procs })
}

pub fn parse(out: &str) -> Result<ProcRaw, ParseError> {
    if out.len() > MAX_OUTPUT_BYTES * 4 {
        return Err(ParseError::TooLarge);
    }
    if out.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let s = sections(out);
    if !s.contains_key("END") {
        return Err(ParseError::Incomplete);
    }
    let get = |k: &str| s.get(k).map(Vec::as_slice).unwrap_or(&[]);
    let os = get("OS").iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").to_string();
    let first = |k: &str| get(k).iter().map(|l| l.trim()).find(|l| !l.is_empty()).map(str::to_string);
    let a = snap(get("UPA"), get("STATA"));
    let b = snap(get("UPB"), get("STATB"));
    let (earlier, now) = match (a, b) {
        (Some(a), Some(b)) => (Some(a), b),
        (Some(a), None) => (None, a),
        _ => (None, Snap { uptime: 0.0, procs: HashMap::new() }),
    };
    let users = get("PASSWD")
        .iter()
        .filter_map(|l| {
            let p: Vec<&str> = l.split(':').collect();
            Some((u32::try_from(u(p.get(2)?)?).ok()?, p.first()?.to_string()))
        })
        .collect();
    let uids = get("UIDS")
        .iter()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect();
    let args = get("ARGS")
        .iter()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, rest) = l.split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, rest.trim().chars().take(MAX_COMMAND).collect()))
        })
        .collect();
    Ok(ProcRaw {
        os,
        clk: first("CLK").and_then(|v| v.parse().ok()).filter(|c: &f64| *c > 0.0).unwrap_or(100.0),
        page: first("PAGE").and_then(|v| v.parse().ok()).filter(|p| *p > 0).unwrap_or(4096),
        mem_total_kb: get("MEMTOTAL").iter().find_map(|l| l.split_whitespace().nth(1)?.parse().ok()),
        earlier,
        now,
        users,
        uids,
        args,
    })
}

/// The list from a run (and the reading before it, when this run did not take two itself).
pub fn derive(host_id: Uuid, taken_at_ms: u64, prev: Option<&Snap>, raw: &ProcRaw) -> ProcessList {
    let before = raw.earlier.as_ref().or(prev);
    let base = match before {
        Some(b) => baseline(Some(&fake(b)), &fake(&raw.now)),
        None => Baseline::First,
    };
    let dt = match base {
        Baseline::Measured { elapsed_secs } => Some(elapsed_secs),
        _ => None,
    };
    let mem_total = raw.mem_total_kb.map(|k| k * 1024);
    let mut items: Vec<ProcessInfo> = raw
        .now
        .procs
        .iter()
        .map(|(pid, p)| {
            // The same pid with the same start time is the same process; anything else is a new one with no history.
            let cpu = dt.zip(before.and_then(|b| b.procs.get(pid)).filter(|o| o.start == p.start && o.name == p.name)).and_then(|(dt, o)| delta(o.ticks, p.ticks).map(|d| d as f64 * 100.0 / (raw.clk * dt)));
            let uid = raw.uids.get(pid);
            let rss = p.rss_pages.saturating_mul(raw.page);
            ProcessInfo {
                pid: *pid,
                ppid: p.ppid,
                user: uid.map(|u| raw.users.get(u).cloned().unwrap_or_else(|| u.to_string())).unwrap_or_default(),
                state: p.state.clone(),
                name: p.name.clone(),
                command: raw.args.get(pid).cloned().unwrap_or_default(),
                cpu_pct: cpu.map(|c| (c * 10.0).round() / 10.0),
                rss,
                mem_pct: mem_total.filter(|t| *t > 0).map(|t| (rss as f64 * 1000.0 / t as f64).round() / 10.0),
                threads: p.threads,
                start_ticks: p.start,
                age_secs: Some(raw.now.uptime - p.start as f64 / raw.clk).filter(|a| *a >= 0.0),
            }
        })
        .collect();
    let total = items.len() as u32;
    // Busiest first (a process with no figure after those with one), then by memory, then by pid so the order is stable.
    items.sort_by(|a, b| b.cpu_pct.unwrap_or(-1.0).total_cmp(&a.cpu_pct.unwrap_or(-1.0)).then(b.rss.cmp(&a.rss)).then(a.pid.cmp(&b.pid)));
    let truncated = items.len() > MAX_ITEMS;
    items.truncate(MAX_ITEMS);
    ProcessList { host_id, taken_at_ms, total, truncated, baseline: base, commands: !raw.args.is_empty(), items }
}

/// `baseline` takes readings; a process snapshot only has the host's uptime to offer it.
fn fake(s: &Snap) -> super::parse::Raw {
    super::parse::Raw::with_uptime(s.uptime)
}

// -- signals ----------------------------------------------------------------------------------------------------------

/// The signals offered. A name, never a number: the numbers differ between systems.
pub const SIGNALS: &[&str] = &["TERM", "KILL", "HUP", "INT", "STOP", "CONT", "USR1", "USR2"];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalOutcome {
    /// The signal was delivered (what the program does with it is its own business).
    Sent,
    /// There was no such process any more.
    Gone,
    /// The pid is now a different process (a reused number). Nothing was sent.
    Changed,
    /// The host refused, in its own words.
    Refused { reason: String },
    /// The host did not answer: the signal may or may not have been delivered, and it is not sent again.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalError {
    BadPid,
    BadSignal,
    BadName,
}

impl std::fmt::Display for SignalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SignalError::BadPid => "that is not a process that can be signalled (pid 1 and the kernel's own are protected)",
            SignalError::BadSignal => "that signal is not offered",
            SignalError::BadName => "the process name has characters that cannot be checked safely",
        })
    }
}

/// The command that signals one process, after checking on the host that it is still the one meant. Everything that goes
/// into it is validated here and quoted: the pid is a number, the signal is from the list, the name is single-quoted.
pub fn signal_script(pid: u32, start_ticks: u64, name: &str, signal: &str) -> Result<String, SignalError> {
    if pid < 2 {
        return Err(SignalError::BadPid);
    }
    if !SIGNALS.contains(&signal) {
        return Err(SignalError::BadSignal);
    }
    // A kernel name is at most 15 bytes; whatever a process calls itself, only plain characters are accepted for the check.
    if name.is_empty() || name.len() > 40 || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '+' | '@' | '/' | ' ')) {
        return Err(SignalError::BadName);
    }
    Ok(format!(
        r#"LC_ALL=C; export LC_ALL
pid={pid}
line=$(cat /proc/$pid/stat 2>/dev/null) || {{ echo RESULT=gone; exit 0; }}
[ -n "$line" ] || {{ echo RESULT=gone; exit 0; }}
head=${{line%) *}}
comm=${{head#*(}}
rest=${{line##*) }}
start=$(echo "$rest" | awk '{{print $20}}')
if [ "$start" != {start_ticks} ] || [ "$comm" != '{name}' ]; then echo RESULT=changed; exit 0; fi
if err=$(kill -{signal} "$pid" 2>&1); then echo RESULT=ok; else echo "RESULT=refused:$err"; fi
echo "@@END"
"#
    ))
}

pub fn parse_signal(out: &str) -> SignalOutcome {
    for l in out.lines() {
        match l.trim() {
            "RESULT=ok" => return SignalOutcome::Sent,
            "RESULT=gone" => return SignalOutcome::Gone,
            "RESULT=changed" => return SignalOutcome::Changed,
            r => {
                if let Some(why) = r.strip_prefix("RESULT=refused:") {
                    // `kill`'s words, without the shell's prefix noise, and bounded.
                    return SignalOutcome::Refused { reason: why.trim().chars().take(200).collect() };
                }
            }
        }
    }
    SignalOutcome::Refused { reason: "the host's answer was not understood".into() }
}
