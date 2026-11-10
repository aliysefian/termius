//! CPU and memory attributed to tmux sessions and panes, from the host's own `/proc` (the same reading the process list
//! uses, see `hostmetrics::procs`).
//!
//! A pane's process is the root of a tree: the shell tmux started and everything it started. Every process on the host is
//! given to the *nearest* pane root above it (itself included), so a process is counted once, even when one tmux runs
//! inside another's pane. What cannot be attributed is left out, never guessed:
//!
//! * a pane whose pid is not visible (another PID namespace, `hidepid`, a process that just ended) has no figures, and the
//!   session says how many of its panes were not seen;
//! * a daemon that detached from its parent (`setsid`, double fork) is no longer below the pane and is not counted.
//!
//! The figures are estimates built from process trees. They are not cgroup accounting: resident memory is summed per
//! process, so pages shared between processes are counted in each.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hostmetrics::derive::delta;
use crate::hostmetrics::procs::{ProcRaw, Snap};

use super::model::Session;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub processes: u32,
    /// Resident memory in bytes.
    pub rss: u64,
    /// Share of one core, so a busy multi-process session passes 100. `None` until two readings exist.
    pub cpu_pct: Option<f64>,
    /// Program names by process count, most first (at most 5).
    pub top: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionUsage {
    pub session_id: String,
    /// `None` when none of the session's panes could be seen.
    pub usage: Option<Usage>,
    pub panes_seen: u32,
    pub panes_total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    pub host_id: Uuid,
    pub taken_at_ms: u64,
    pub sessions: Vec<SessionUsage>,
    /// By pane id.
    pub panes: HashMap<String, Usage>,
    /// Why CPU is missing, when it is.
    pub cpu_note: Option<String>,
    /// Always true: these are estimates from process trees.
    pub estimate: bool,
}

/// Attribute the host's processes to the panes of `sessions`. `prev` is the reading to compare with when `raw` has no
/// earlier one of its own.
pub fn derive(host_id: Uuid, taken_at_ms: u64, sessions: &[Session], raw: &ProcRaw, prev: Option<&Snap>) -> Resources {
    let roots: HashMap<u32, &str> = sessions.iter().flat_map(|s| &s.windows).flat_map(|w| &w.panes).filter_map(|p| p.pid.map(|pid| (pid, p.id.as_str()))).collect();

    let before = raw.earlier.as_ref().or(prev);
    let dt = before.map(|b| raw.now.uptime - b.uptime).filter(|d| *d > 0.05);

    // Each process goes to the nearest pane root at or above it.
    let mut owner: HashMap<u32, &str> = HashMap::new();
    for &pid in raw.now.procs.keys() {
        let mut cur = pid;
        let mut seen = HashSet::new();
        for _ in 0..128 {
            if let Some(p) = roots.get(&cur) {
                owner.insert(pid, p);
                break;
            }
            if !seen.insert(cur) {
                break;
            }
            match raw.now.procs.get(&cur) {
                Some(s) if s.ppid != 0 && s.ppid != cur => cur = s.ppid,
                _ => break,
            }
        }
    }

    let mut acc: HashMap<&str, (Usage, HashMap<String, u32>, f64)> = HashMap::new();
    for (pid, pane) in &owner {
        let st = &raw.now.procs[pid];
        let e = acc.entry(pane).or_default();
        e.0.processes += 1;
        e.0.rss = e.0.rss.saturating_add(st.rss_pages.saturating_mul(raw.page));
        *e.1.entry(st.name.clone()).or_insert(0) += 1;
        if let Some(dt) = dt {
            // The same pid with the same start time and name is the same process; anything else has no history here.
            if let Some(old) = before.and_then(|b| b.procs.get(pid)).filter(|o| o.start == st.start && o.name == st.name) {
                if let Some(d) = delta(old.ticks, st.ticks) {
                    e.2 += d as f64 * 100.0 / (raw.clk * dt);
                }
            }
        }
    }

    let mut panes: HashMap<String, Usage> = HashMap::new();
    for (pane, (mut u, names, cpu)) in acc {
        u.cpu_pct = dt.map(|_| cpu);
        u.top = top_names(names);
        panes.insert(pane.to_string(), u);
    }

    let out = sessions
        .iter()
        .map(|s| {
            let ids: Vec<&str> = s.windows.iter().flat_map(|w| &w.panes).map(|p| p.id.as_str()).collect();
            let seen: Vec<&Usage> = ids.iter().filter_map(|id| panes.get(*id)).collect();
            let usage = (!seen.is_empty()).then(|| {
                let mut names: HashMap<String, u32> = HashMap::new();
                for u in &seen {
                    for (n, c) in &u.top {
                        *names.entry(n.clone()).or_insert(0) += c;
                    }
                }
                Usage {
                    processes: seen.iter().map(|u| u.processes).sum(),
                    rss: seen.iter().map(|u| u.rss).sum(),
                    cpu_pct: if dt.is_some() { Some(seen.iter().filter_map(|u| u.cpu_pct).sum()) } else { None },
                    top: top_names(names),
                }
            });
            SessionUsage { session_id: s.id.clone(), usage, panes_seen: seen.len() as u32, panes_total: ids.len() as u32 }
        })
        .collect();

    Resources {
        host_id,
        taken_at_ms,
        sessions: out,
        panes,
        cpu_note: if dt.is_none() { Some("CPU needs two readings; it appears after the next refresh.".into()) } else { None },
        estimate: true,
    }
}

fn top_names(names: HashMap<String, u32>) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = names.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(5);
    v
}
