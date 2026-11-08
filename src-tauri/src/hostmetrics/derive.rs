//! Turns two readings into a [`Sample`]: the arithmetic of rates.
//!
//! The rules, which the tests pin down:
//! * time between readings is the **host's own uptime**, not the app's clock, so a slow connection does not skew a rate;
//! * a figure that cannot be measured is `None`, never 0: the first reading, a host that rebooted, a gap too long to
//!   mean anything, a counter that went backwards, an interface that is new or was recreated;
//! * a 32-bit counter that wrapped (it was near the top, and is now small) is carried over, since that is a real change;
//! * the same traffic is never added twice (see [`counted`] and the disk rules).

use std::collections::HashMap;

use uuid::Uuid;

use super::model::*;
use super::parse::{classify, CpuTimes, DiskCounters, NetCounters, Raw};

/// Closer than this, two readings are one moment: no rate is measured over them.
pub const MIN_ELAPSED_SECS: f64 = 0.2;
/// Further apart than this, a "rate" would be an average over something that was not being watched.
pub const MAX_GAP_SECS: f64 = 900.0;
/// A 32-bit counter wraps from just under 2^32 to just over 0. A drop is read as a wrap only if the old value was within
/// this distance of the top and the new one is within it of the bottom.
const WRAP_WINDOW: u64 = 1 << 28;
const SECTOR: u64 = 512;

/// How two readings relate.
pub fn baseline(prev: Option<&Raw>, cur: &Raw) -> Baseline {
    let (Some(p), Some(c)) = (prev.and_then(|p| p.uptime), cur.uptime) else { return Baseline::First };
    if c < p {
        return Baseline::Rebooted;
    }
    let dt = c - p;
    if dt > MAX_GAP_SECS {
        Baseline::Stale { gap_secs: dt }
    } else if dt < MIN_ELAPSED_SECS {
        Baseline::TooClose
    } else {
        Baseline::Measured { elapsed_secs: dt }
    }
}

/// The change in a counter, or `None` if it went backwards in a way that is a reset rather than a wrap.
pub fn delta(prev: u64, cur: u64) -> Option<u64> {
    if cur >= prev {
        Some(cur - prev)
    } else if prev <= u64::from(u32::MAX) && prev > u64::from(u32::MAX) - WRAP_WINDOW && cur < WRAP_WINDOW {
        Some(cur + (1u64 << 32) - prev)
    } else {
        None
    }
}

fn rate(prev: u64, cur: u64, dt: f64) -> Option<f64> {
    delta(prev, cur).map(|d| d as f64 / dt)
}

fn pct(part: f64, whole: f64) -> Option<f64> {
    (whole > 0.0).then(|| (part * 100.0 / whole).clamp(0.0, 100.0))
}

// -- CPU --------------------------------------------------------------------------------------------------------

struct CpuDelta {
    busy: u64,
    iowait: u64,
    steal: u64,
    total: u64,
}

fn cpu_delta(p: &CpuTimes, c: &CpuTimes) -> Option<CpuDelta> {
    let d = |a: u64, b: u64| b.checked_sub(a);
    let user = d(p.user, c.user)?;
    let nice = d(p.nice, c.nice)?;
    let system = d(p.system, c.system)?;
    let idle = d(p.idle, c.idle)?;
    let irq = d(p.irq, c.irq)?;
    let softirq = d(p.softirq, c.softirq)?;
    let steal = d(p.steal, c.steal)?;
    // The kernel documents that iowait can go backwards (a CPU going offline, or a task waking on another CPU), so a
    // drop in it alone is not a reset: that term counts as nothing.
    let iowait = c.iowait.saturating_sub(p.iowait);
    let busy = user + nice + system + irq + softirq + steal;
    Some(CpuDelta { busy, iowait, steal, total: busy + idle + iowait })
}

fn cpu(prev: Option<&Raw>, cur: &Raw, measured: bool) -> Option<CpuUsage> {
    let total_now = cur.cpu_total.as_ref()?;
    let prev = prev.filter(|_| measured);
    let all = prev.and_then(|p| p.cpu_total.as_ref()).and_then(|p| cpu_delta(p, total_now)).filter(|d| d.total > 0);
    let prev_cores: HashMap<u32, &CpuTimes> = prev.map(|p| p.cpu_cores.iter().map(|(i, t)| (*i, t)).collect()).unwrap_or_default();
    let cores = cur
        .cpu_cores
        .iter()
        .map(|(i, t)| CoreUsage { index: *i, pct: prev_cores.get(i).and_then(|p| cpu_delta(p, t)).filter(|d| d.total > 0).and_then(|d| pct(d.busy as f64, d.total as f64)) })
        .collect();
    Some(CpuUsage {
        total_pct: all.as_ref().and_then(|d| pct(d.busy as f64, d.total as f64)),
        iowait_pct: all.as_ref().and_then(|d| pct(d.iowait as f64, d.total as f64)),
        steal_pct: all.as_ref().and_then(|d| pct(d.steal as f64, d.total as f64)),
        cores,
        logical: cur.cpu_cores.len() as u32,
        freq_mhz: cur.freq_mhz,
    })
}

// -- memory -----------------------------------------------------------------------------------------------------

fn memory(raw: &Raw) -> Option<Memory> {
    let kb = |k: &str| raw.mem_kb.get(k).copied();
    let b = |k: &str| kb(k).map(|v| v.saturating_mul(1024));
    let total = b("MemTotal").filter(|t| *t > 0)?;
    let free = b("MemFree").unwrap_or(0).min(total);
    let buffers = b("Buffers").unwrap_or(0);
    let shared = b("Shmem").unwrap_or(0);
    let sreclaimable = b("SReclaimable").unwrap_or(0);
    // Shmem is counted in "Cached" by the kernel but cannot be dropped like cache, so it is taken out: a tmpfs full of
    // data is memory in use, and counting it as cache would show a loaded host as idle.
    let cached = b("Cached").unwrap_or(0).saturating_add(sreclaimable).saturating_sub(shared);
    let exact = kb("MemAvailable").is_some();
    // Kernels before 3.14 have no MemAvailable: free + buffers + cache is the usual stand-in.
    let available = b("MemAvailable").unwrap_or_else(|| free.saturating_add(buffers).saturating_add(cached)).min(total);
    let used = total - available;
    let swap_total = b("SwapTotal").unwrap_or(0);
    let swap_used = swap_total.saturating_sub(b("SwapFree").unwrap_or(0));
    Some(Memory {
        total,
        used,
        available,
        free,
        buffers,
        cached,
        shared,
        swap_total,
        swap_used,
        committed: b("Committed_AS"),
        slab: b("Slab"),
        used_pct: used as f64 * 100.0 / total as f64,
        available_exact: exact,
    })
}

// -- disks ------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct DiskRates {
    read_bps: Option<f64>,
    write_bps: Option<f64>,
    read_iops: Option<f64>,
    write_iops: Option<f64>,
    busy_pct: Option<f64>,
    await_ms: Option<f64>,
}

fn disk_rates(p: &DiskCounters, c: &DiskCounters, dt: f64) -> DiskRates {
    let rd_ios = delta(p.rd_ios, c.rd_ios);
    let wr_ios = delta(p.wr_ios, c.wr_ios);
    let rd_ticks = delta(p.rd_ticks, c.rd_ticks);
    let wr_ticks = delta(p.wr_ticks, c.wr_ticks);
    let io_ticks = delta(p.io_ticks, c.io_ticks);
    let await_ms = match (rd_ios, wr_ios, rd_ticks, wr_ticks) {
        (Some(a), Some(b), Some(x), Some(y)) if a + b > 0 => Some((x + y) as f64 / (a + b) as f64),
        _ => None,
    };
    DiskRates {
        read_bps: rate(p.rd_sectors, c.rd_sectors, dt).map(|s| s * SECTOR as f64),
        write_bps: rate(p.wr_sectors, c.wr_sectors, dt).map(|s| s * SECTOR as f64),
        read_iops: rd_ios.map(|v| v as f64 / dt),
        write_iops: wr_ios.map(|v| v as f64 / dt),
        busy_pct: io_ticks.and_then(|t| pct(t as f64, dt * 1000.0)),
        await_ms,
    }
}

/// One entry per whole block device. Partitions are not in `/sys/block`, so they never appear; device-mapper and RAID
/// layers are, but are marked virtual and left out of any total, since their traffic is also the disks' underneath.
fn disks(prev: Option<&Raw>, cur: &Raw, dt: Option<f64>) -> Vec<DiskIo> {
    let mut names: Vec<&String> = cur.block.keys().filter(|n| cur.disk.contains_key(*n)).collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let (meta, c) = (&cur.block[name], &cur.disk[name]);
            let r = dt.zip(prev.and_then(|p| p.disk.get(name))).map(|(dt, p)| disk_rates(p, c, dt)).unwrap_or_default();
            DiskIo {
                name: name.clone(),
                model: meta.model.clone(),
                size: meta.size_sectors.map(|s| s.saturating_mul(SECTOR)),
                rotational: meta.rotational,
                virtual_device: meta.virtual_device,
                read_bps: r.read_bps,
                write_bps: r.write_bps,
                read_iops: r.read_iops,
                write_iops: r.write_iops,
                busy_pct: r.busy_pct,
                await_ms: r.await_ms,
                read_total: c.rd_sectors.saturating_mul(SECTOR),
                write_total: c.wr_sectors.saturating_mul(SECTOR),
            }
        })
        .collect()
}

// -- network ----------------------------------------------------------------------------------------------------

/// Whether an interface's traffic goes into the host total. Bridges, tunnels, containers and VLANs carry traffic that
/// also crosses a real device, and the members of a bond are already in the bond: counting any of those adds the same
/// bytes twice.
fn counted(kind: IfaceKind, master_kind: Option<IfaceKind>) -> bool {
    match kind {
        IfaceKind::Bond => true,
        IfaceKind::Physical | IfaceKind::Wireless => master_kind != Some(IfaceKind::Bond),
        _ => false,
    }
}

fn network(prev: Option<&Raw>, cur: &Raw, dt: Option<f64>) -> Option<Network> {
    if cur.net.is_empty() {
        return None;
    }
    let kinds: HashMap<&str, IfaceKind> = cur.net.iter().map(|(n, _)| (n.as_str(), classify(n, cur.ifaces.get(n)))).collect();
    // A member's master, when that master is a bond or a bridge.
    let master_of = |name: &str| -> Option<&str> {
        cur.lower
            .iter()
            .find(|(m, member)| member == name && matches!(kinds.get(m.as_str()), Some(IfaceKind::Bond | IfaceKind::Bridge)))
            .map(|(m, _)| m.as_str())
    };
    let prev_counters: HashMap<&str, &NetCounters> = prev.map(|p| p.net.iter().map(|(n, c)| (n.as_str(), c)).collect()).unwrap_or_default();
    let mut interfaces = Vec::new();
    for (name, c) in &cur.net {
        let kind = kinds[name.as_str()];
        if kind == IfaceKind::Loopback {
            continue;
        }
        let meta = cur.ifaces.get(name);
        let master = master_of(name);
        let master_kind = master.and_then(|m| kinds.get(m).copied());
        // An interface deleted and made again under the same name starts its counters at zero but is not the same
        // interface: its index differs. Its first reading has no rate.
        let same = |p: &Raw| match (p.ifaces.get(name).and_then(|m| m.ifindex), meta.and_then(|m| m.ifindex)) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        let before = dt.and_then(|dt| prev.filter(|p| same(p)).and_then(|_| prev_counters.get(name.as_str()).copied()).map(|p| (p, dt)));
        let r = |f: fn(&NetCounters) -> u64| before.and_then(|(p, dt)| rate(f(p), f(c), dt));
        interfaces.push(NetIface {
            name: name.clone(),
            kind,
            state: meta.map_or_else(|| "unknown".to_string(), |m| m.state.clone()),
            master: master.map(str::to_string),
            mtu: meta.and_then(|m| m.mtu),
            link_mbps: meta.and_then(|m| m.speed_mbps),
            rx_bps: r(|x| x.rx_bytes),
            tx_bps: r(|x| x.tx_bytes),
            rx_pps: r(|x| x.rx_packets),
            tx_pps: r(|x| x.tx_packets),
            rx_bytes: c.rx_bytes,
            tx_bytes: c.tx_bytes,
            rx_errors: c.rx_errors,
            tx_errors: c.tx_errors,
            rx_drops: c.rx_drops,
            tx_drops: c.tx_drops,
            counted: counted(kind, master_kind),
        });
    }
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    // The total exists only if every counted interface has a rate: a total missing an interface would be too low
    // without saying so.
    let counted_ifaces: Vec<&NetIface> = interfaces.iter().filter(|i| i.counted).collect();
    let total = (!counted_ifaces.is_empty() && counted_ifaces.iter().all(|i| i.rx_bps.is_some() && i.tx_bps.is_some())).then(|| NetTotal {
        rx_bps: counted_ifaces.iter().filter_map(|i| i.rx_bps).sum(),
        tx_bps: counted_ifaces.iter().filter_map(|i| i.tx_bps).sum(),
        from: counted_ifaces.iter().map(|i| i.name.clone()).collect(),
    });
    let default_iface = cur.default_iface.clone().filter(|d| interfaces.iter().any(|i| &i.name == d));
    Some(Network { interfaces, default_iface, total })
}

// -- the sample -------------------------------------------------------------------------------------------------

/// Build a sample from this reading and the one before it. The second value says whether this reading should become
/// the new baseline (it should, except when it was too close to the last one to measure anything).
pub fn derive(host_id: Uuid, taken_at_ms: u64, prev: Option<&Raw>, cur: &Raw, info: &StaticInfo) -> (Sample, bool) {
    derive_with(host_id, taken_at_ms, prev, cur, info, &[])
}

/// [`derive`] with NVIDIA GPUs carried over from an earlier reading, for readings that did not ask `nvidia-smi` again.
pub fn derive_with(host_id: Uuid, taken_at_ms: u64, prev: Option<&Raw>, cur: &Raw, info: &StaticInfo, carried_nvidia: &[Gpu]) -> (Sample, bool) {
    if !cur.linux {
        let sample = Sample {
            host_id,
            taken_at_ms,
            platform: Platform::Unsupported { os: cur.os.clone() },
            baseline: Baseline::First,
            uptime_secs: None,
            cpu: None,
            memory: None,
            load: None,
            processes: None,
            filesystems: Vec::new(),
            disks: Vec::new(),
            network: None,
            temperatures: Vec::new(),
            gpus: Vec::new(),
            notes: Vec::new(),
            capabilities: Capabilities::default(),
            info: info.clone(),
        };
        return (sample, false);
    }
    let base = baseline(prev, cur);
    let dt = match base {
        Baseline::Measured { elapsed_secs } => Some(elapsed_secs),
        _ => None,
    };
    let prev_ok = prev.filter(|_| base.has_rates());
    let sample = Sample {
        host_id,
        taken_at_ms,
        platform: Platform::Linux,
        baseline: base,
        uptime_secs: cur.uptime,
        cpu: cpu(prev_ok, cur, base.has_rates()),
        memory: memory(cur),
        load: cur.load.map(|(m1, m5, m15)| Load { m1, m5, m15 }),
        processes: (cur.processes.is_some() || cur.threads.is_some()).then_some(ProcessCounts { processes: cur.processes, threads: cur.threads, running: cur.running }),
        filesystems: cur.filesystems.clone(),
        disks: disks(prev_ok, cur, dt),
        network: network(prev_ok, cur, dt),
        temperatures: cur.temps.clone(),
        gpus: {
            let mut g = cur.amd.clone();
            g.extend(cur.nvidia.clone().unwrap_or_else(|| carried_nvidia.to_vec()));
            g
        },
        notes: cur.notes.clone(),
        capabilities: cur.capabilities.clone(),
        info: info.clone(),
    };
    (sample, !matches!(base, Baseline::TooClose))
}
