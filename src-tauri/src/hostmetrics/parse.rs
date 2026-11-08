//! Reads what `script.rs` printed into raw counters. No arithmetic between readings happens here (that is `derive.rs`);
//! this only turns text into numbers and says what the host has. Every parser accepts junk lines and skips them, and
//! every section may be missing: a host without `/proc/diskstats` simply has no disks in its reading.

use std::collections::HashMap;

use super::model::{Capabilities, CpuCache, Filesystem, Gpu, GpuProcess, GpuVendor, IfaceKind, StaticInfo, TempKind, TempSensor};

/// More than this in one section is not a host this engine should be reading; the rest is ignored.
const MAX_SECTION_LINES: usize = 5000;
/// A reading larger than this is refused outright.
pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    TooLarge,
    /// The run was cut off: its last line, `@@END`, never arrived.
    Incomplete,
    Empty,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::TooLarge => write!(f, "the host's answer was too large to read"),
            ParseError::Incomplete => write!(f, "the host's answer was cut short"),
            ParseError::Empty => write!(f, "the host printed nothing"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    /// Ticks spent on work: everything but idle and waiting for I/O. (Guest time is already inside user and nice.)
    pub fn busy(&self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq + self.steal
    }
    pub fn total(&self) -> u64 {
        self.busy() + self.idle + self.iowait
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_drops: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_drops: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfMeta {
    pub state: String,
    pub mtu: Option<u32>,
    pub speed_mbps: Option<u32>,
    pub arp_type: Option<u32>,
    pub ifindex: Option<u64>,
    pub has_device: bool,
    pub devtype: String,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskCounters {
    pub rd_ios: u64,
    pub rd_sectors: u64,
    pub rd_ticks: u64,
    pub wr_ios: u64,
    pub wr_sectors: u64,
    pub wr_ticks: u64,
    /// Milliseconds the device had I/O in flight.
    pub io_ticks: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockMeta {
    pub size_sectors: Option<u64>,
    pub rotational: Option<bool>,
    pub virtual_device: bool,
    pub model: Option<String>,
}

/// One reading of a host, as numbers straight from its counters.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub os: String,
    pub linux: bool,
    pub uptime: Option<f64>,
    pub cpu_total: Option<CpuTimes>,
    pub cpu_cores: Vec<(u32, CpuTimes)>,
    pub mem_kb: HashMap<String, u64>,
    pub load: Option<(f64, f64, f64)>,
    pub running: Option<u32>,
    pub threads: Option<u32>,
    pub processes: Option<u32>,
    pub freq_mhz: Option<f64>,
    pub net: Vec<(String, NetCounters)>,
    pub ifaces: HashMap<String, IfMeta>,
    /// `(master, member)`: a bond or bridge and an interface under it.
    pub lower: Vec<(String, String)>,
    pub default_iface: Option<String>,
    pub disk: HashMap<String, DiskCounters>,
    pub block: HashMap<String, BlockMeta>,
    pub filesystems: Vec<Filesystem>,
    pub temps: Vec<TempSensor>,
    pub amd: Vec<Gpu>,
    /// NVIDIA GPUs, when `nvidia-smi` was asked this time (`None` when it was not, which is not the same as none).
    pub nvidia: Option<Vec<Gpu>>,
    /// Things the host could not do that a person would want to know ("no timeout command: GPU skipped").
    pub notes: Vec<String>,
    pub capabilities: Capabilities,
    pub info: Option<StaticInfo>,
}

impl Raw {
    /// A reading holding only the host's uptime: what the rate rules need to relate two readings of something else.
    pub(super) fn with_uptime(uptime: f64) -> Raw {
        Raw {
            os: "Linux".into(),
            linux: true,
            uptime: Some(uptime),
            cpu_total: None,
            cpu_cores: Vec::new(),
            mem_kb: HashMap::new(),
            load: None,
            running: None,
            threads: None,
            processes: None,
            freq_mhz: None,
            net: Vec::new(),
            ifaces: HashMap::new(),
            lower: Vec::new(),
            default_iface: None,
            disk: HashMap::new(),
            block: HashMap::new(),
            filesystems: Vec::new(),
            temps: Vec::new(),
            amd: Vec::new(),
            nvidia: None,
            notes: Vec::new(),
            capabilities: Capabilities::default(),
            info: None,
        }
    }
}

pub(super) fn sections(out: &str) -> HashMap<&str, Vec<&str>> {
    let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut cur: Option<&str> = None;
    for line in out.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(name) = line.strip_prefix("@@") {
            let name = name.trim();
            cur = Some(name);
            map.entry(name).or_default();
        } else if let Some(c) = cur {
            if let Some(v) = map.get_mut(c) {
                if v.len() < MAX_SECTION_LINES {
                    v.push(line);
                }
            }
        }
    }
    map
}

pub(super) fn u(s: &str) -> Option<u64> {
    s.trim().parse().ok()
}

pub(super) fn f(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Interface and device names are plain text from the host that end up in the window and as keys: only the usual
/// characters are accepted.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':'))
}

fn cpu_times(fields: &[&str]) -> Option<CpuTimes> {
    let n: Vec<u64> = fields.iter().take(8).map(|v| v.parse().ok()).collect::<Option<Vec<u64>>>()?;
    if n.len() < 4 {
        return None;
    }
    let g = |i: usize| n.get(i).copied().unwrap_or(0);
    Some(CpuTimes { user: g(0), nice: g(1), system: g(2), idle: g(3), iowait: g(4), irq: g(5), softirq: g(6), steal: g(7) })
}

fn parse_stat(lines: &[&str]) -> (Option<CpuTimes>, Vec<(u32, CpuTimes)>) {
    let mut total = None;
    let mut cores = Vec::new();
    for l in lines {
        let mut it = l.split_whitespace();
        let Some(tag) = it.next() else { continue };
        let rest: Vec<&str> = it.collect();
        if tag == "cpu" {
            total = cpu_times(&rest);
        } else if let Some(idx) = tag.strip_prefix("cpu").and_then(|i| i.parse::<u32>().ok()) {
            if let Some(t) = cpu_times(&rest) {
                if cores.len() < 4096 {
                    cores.push((idx, t));
                }
            }
        }
    }
    (total, cores)
}

fn parse_meminfo(lines: &[&str]) -> HashMap<String, u64> {
    lines
        .iter()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            // "MemTotal:  16384 kB"; HugePages_* have no unit and are counts, which are never read as memory here.
            Some((k.trim().to_string(), v.split_whitespace().next()?.parse().ok()?))
        })
        .collect()
}

/// The three load averages, and the running and total scheduling entities of `/proc/loadavg`'s fourth field.
struct LoadAvg {
    load: Option<(f64, f64, f64)>,
    running: Option<u32>,
    threads: Option<u32>,
}

fn parse_loadavg(lines: &[&str]) -> LoadAvg {
    let none = LoadAvg { load: None, running: None, threads: None };
    let Some(l) = lines.iter().find(|l| !l.trim().is_empty()) else { return none };
    let p: Vec<&str> = l.split_whitespace().collect();
    let load = match (p.first().and_then(|v| f(v)), p.get(1).and_then(|v| f(v)), p.get(2).and_then(|v| f(v))) {
        (Some(a), Some(b), Some(c)) => Some((a, b, c)),
        _ => None,
    };
    let (running, threads) = match p.get(3).and_then(|v| v.split_once('/')) {
        Some((r, t)) => (r.parse().ok(), t.parse().ok()),
        None => (None, None),
    };
    LoadAvg { load, running, threads }
}

fn parse_netdev(lines: &[&str]) -> Vec<(String, NetCounters)> {
    let mut out = Vec::new();
    for l in lines {
        // "  eth0: 1234 ..." and, when the name is long, "enp0s31f6:1234 ...": split at the colon, not at a space.
        let Some((name, rest)) = l.split_once(':') else { continue };
        let name = name.trim();
        if !name_ok(name) || out.len() >= 512 {
            continue;
        }
        let n: Vec<u64> = rest.split_whitespace().map(|v| v.parse().unwrap_or(u64::MAX)).collect();
        if n.len() < 12 || [n[0], n[1], n[8], n[9]].contains(&u64::MAX) {
            continue;
        }
        let z = |v: u64| if v == u64::MAX { 0 } else { v };
        out.push((
            name.to_string(),
            NetCounters { rx_bytes: n[0], rx_packets: n[1], rx_errors: z(n[2]), rx_drops: z(n[3]), tx_bytes: n[8], tx_packets: n[9], tx_errors: z(n[10]), tx_drops: z(n[11]) },
        ));
    }
    out
}

fn parse_netif(lines: &[&str]) -> (HashMap<String, IfMeta>, Vec<(String, String)>) {
    let mut ifaces = HashMap::new();
    let mut lower = Vec::new();
    for l in lines {
        let p: Vec<&str> = l.split('|').collect();
        match p.first().copied() {
            Some("IF") if p.len() >= 10 && name_ok(p[1]) && ifaces.len() < 512 => {
                ifaces.insert(
                    p[1].to_string(),
                    IfMeta {
                        state: if p[2].trim().is_empty() { "unknown".into() } else { p[2].trim().to_string() },
                        mtu: p[3].trim().parse().ok(),
                        // The driver says -1 (or nothing) when it has no link speed, as virtual devices do.
                        speed_mbps: p[4].trim().parse::<i64>().ok().filter(|v| *v > 0 && *v < 10_000_000).map(|v| v as u32),
                        arp_type: p[5].trim().parse().ok(),
                        ifindex: p[6].trim().parse().ok(),
                        has_device: p[7].trim() == "1",
                        devtype: p[8].trim().to_string(),
                        flags: p[9].split_whitespace().map(str::to_string).collect(),
                    },
                );
            }
            Some("LOWER") if p.len() >= 3 && name_ok(p[1]) && name_ok(p[2]) && lower.len() < 2048 => lower.push((p[1].to_string(), p[2].to_string())),
            _ => {}
        }
    }
    (ifaces, lower)
}

/// What kind of interface this is, from what the kernel says about it and, only where it says nothing, from the name.
pub fn classify(name: &str, m: Option<&IfMeta>) -> IfaceKind {
    let flag = |f: &str| m.is_some_and(|m| m.flags.iter().any(|x| x == f));
    let devtype = m.map_or("", |m| m.devtype.as_str());
    let has = |prefixes: &[&str]| prefixes.iter().any(|p| name.starts_with(p));
    if name == "lo" || m.and_then(|m| m.arp_type) == Some(772) {
        IfaceKind::Loopback
    } else if flag("bond") || devtype == "bond" {
        IfaceKind::Bond
    } else if flag("bridge") || devtype == "bridge" {
        IfaceKind::Bridge
    } else if devtype == "vlan" {
        IfaceKind::Vlan
    } else if matches!(devtype, "wireguard" | "gre" | "gretap" | "ipip" | "sit" | "vxlan" | "geneve" | "ppp") || flag("tun") || (has(&["wg", "tun", "tap", "tailscale", "ppp", "zt", "vpn"]) && !m.is_some_and(|m| m.has_device)) {
        IfaceKind::Tunnel
    } else if devtype == "veth" || (has(&["docker", "veth", "virbr", "cni", "flannel", "cali", "lxc", "vnet", "podman", "kube"]) && !m.is_some_and(|m| m.has_device)) {
        IfaceKind::Container
    } else if flag("wireless") || devtype == "wlan" {
        IfaceKind::Wireless
    } else if m.is_some_and(|m| m.has_device) {
        IfaceKind::Physical
    } else {
        IfaceKind::Other
    }
}

fn parse_diskstats(lines: &[&str]) -> HashMap<String, DiskCounters> {
    let mut out = HashMap::new();
    for l in lines {
        let p: Vec<&str> = l.split_whitespace().collect();
        // major minor name rd_ios rd_merges rd_sectors rd_ticks wr_ios wr_merges wr_sectors wr_ticks in_flight io_ticks ...
        if p.len() < 13 || !name_ok(p[2]) || out.len() >= 1024 {
            continue;
        }
        let g = |i: usize| p.get(i).and_then(|v| v.parse::<u64>().ok());
        if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(g2), Some(h)) = (g(3), g(5), g(6), g(7), g(9), g(10), g(12)) {
            out.insert(p[2].to_string(), DiskCounters { rd_ios: a, rd_sectors: b, rd_ticks: c, wr_ios: d, wr_sectors: e, wr_ticks: g2, io_ticks: h });
        }
    }
    out
}

fn parse_block(lines: &[&str]) -> HashMap<String, BlockMeta> {
    let mut out = HashMap::new();
    for l in lines {
        let p: Vec<&str> = l.split('|').collect();
        if p.len() < 5 || !name_ok(p[0]) || out.len() >= 1024 {
            continue;
        }
        let model = p[4].trim();
        out.insert(
            p[0].to_string(),
            BlockMeta {
                size_sectors: u(p[1]),
                rotational: match p[2].trim() {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                },
                virtual_device: p[3].trim() == "1",
                model: (!model.is_empty()).then(|| model.chars().take(80).collect()),
            },
        );
    }
    out
}

/// Filesystems that are the kernel or memory, not storage.
const PSEUDO_FS: &[&str] = &[
    "tmpfs", "devtmpfs", "squashfs", "overlay", "efivarfs", "proc", "sysfs", "cgroup", "cgroup2", "ramfs", "devpts", "securityfs", "debugfs", "tracefs", "fusectl", "configfs",
    "pstore", "bpf", "autofs", "mqueue", "hugetlbfs", "binfmt_misc", "nsfs", "rpc_pipefs", "selinuxfs", "fuse.lxcfs", "fuse.gvfsd-fuse", "fuse.portal", "iso9660", "erofs",
];
const PSEUDO_DEV: &[&str] = &["tmpfs", "devtmpfs", "udev", "overlay", "none", "shm", "cgroup", "cgroup2", "proc", "sysfs", "efivarfs", "squashfs"];

fn parse_df(lines: &[&str]) -> Vec<Filesystem> {
    let mut rows = lines.iter().filter(|l| !l.trim().is_empty());
    let Some(header) = rows.next() else { return Vec::new() };
    // With `-T` there is a Type column after the device.
    let typed = header.contains("Type");
    let mut out: Vec<Filesystem> = Vec::new();
    for l in rows {
        let p: Vec<&str> = l.split_whitespace().collect();
        let (dev, fstype, rest) = if typed {
            if p.len() < 7 {
                continue;
            }
            (p[0], Some(p[1]), &p[2..])
        } else {
            if p.len() < 6 {
                continue;
            }
            (p[0], None, &p[1..])
        };
        // rest: 1024-blocks used available capacity mount...
        let (Some(total), Some(used), Some(avail)) = (u(rest[0]), u(rest[1]), u(rest[2])) else { continue };
        let mount = rest[4..].join(" ");
        if mount.is_empty() || total == 0 {
            continue;
        }
        if fstype.is_some_and(|t| PSEUDO_FS.contains(&t)) || PSEUDO_DEV.contains(&dev) || dev.starts_with("/dev/loop") {
            continue;
        }
        // The same device mounted twice (a bind mount, btrfs subvolumes) is one filesystem: keep the first.
        if out.iter().any(|x| x.device == dev && !dev.starts_with("//")) {
            continue;
        }
        let denom = used.saturating_add(avail);
        let used_pct = if denom == 0 { 0.0 } else { used as f64 * 100.0 / denom as f64 };
        if out.len() < 256 {
            out.push(Filesystem { device: dev.to_string(), mount, fstype: fstype.map(str::to_string), total: total.saturating_mul(1024), used: used.saturating_mul(1024), available: avail.saturating_mul(1024), used_pct });
        }
    }
    out
}

fn kv(lines: &[&str]) -> Vec<(String, String)> {
    lines.iter().filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect()
}

fn first_num(s: &str) -> Option<f64> {
    f(s.split_whitespace().next()?)
}

fn parse_static(stat: &[&str], cpuinfo: &[&str], lscpu: &[&str]) -> StaticInfo {
    let mut info = StaticInfo::default();
    let mut sysfs_caches = Vec::new();
    for (k, v) in kv(stat) {
        let nonempty = (!v.is_empty()).then(|| v.clone());
        match k.as_str() {
            "hostname" => info.hostname = nonempty.filter(|h| h.len() <= 253 && h.chars().all(|c| c.is_ascii_graphic())),
            "kernel" => info.kernel = nonempty.filter(|h| h.len() <= 100),
            "arch" => info.arch = nonempty.filter(|h| h.len() <= 32),
            "os" => info.os_name = nonempty.map(|o| o.trim_matches('"').trim_matches('\'').chars().take(100).collect()),
            "max_khz" => info.max_mhz = u(&v).map(|k| k as f64 / 1000.0),
            "base_khz" => info.base_mhz = u(&v).map(|k| k as f64 / 1000.0),
            "cache" => {
                if let Some((label, size)) = v.split_once('|') {
                    if !size.trim().is_empty() && !label.contains("Instruction") {
                        sysfs_caches.push(CpuCache { level: label.trim().to_string(), size: size.trim().to_string() });
                    }
                }
            }
            _ => {}
        }
    }
    // /proc/cpuinfo's summary first, then lscpu over it where it says more.
    let mut logical = None;
    let mut cores_per_socket = None;
    let mut sockets = None;
    for (k, v) in kv(cpuinfo) {
        match k.as_str() {
            "model" if !v.is_empty() => info.cpu_model = Some(v.chars().take(120).collect()),
            "logical" => logical = u(&v).filter(|n| *n > 0).map(|n| n as u32),
            "cores_per_socket" => cores_per_socket = u(&v).filter(|n| *n > 0).map(|n| n as u32),
            "sockets" => sockets = u(&v).filter(|n| *n > 0).map(|n| n as u32),
            "virt" if !v.is_empty() => info.virtualization = Some(v),
            _ => {}
        }
    }
    let mut threads_per_core = None;
    let mut lscpu_caches = Vec::new();
    for l in lscpu {
        let Some((k, v)) = l.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Model name" if !v.is_empty() => info.cpu_model = Some(v.chars().take(120).collect()),
            "Socket(s)" => sockets = u(v).filter(|n| *n > 0).map(|n| n as u32).or(sockets),
            "Core(s) per socket" => cores_per_socket = u(v).filter(|n| *n > 0).map(|n| n as u32).or(cores_per_socket),
            "Thread(s) per core" => threads_per_core = u(v).filter(|n| *n > 0).map(|n| n as u32),
            "CPU(s)" => logical = u(v).filter(|n| *n > 0).map(|n| n as u32).or(logical),
            "CPU max MHz" => info.max_mhz = first_num(v).or(info.max_mhz),
            "Virtualization" if !v.is_empty() => info.virtualization = Some(v.chars().take(40).collect()),
            "Hypervisor vendor" if !v.is_empty() => info.hypervisor = Some(v.chars().take(40).collect()),
            "L1d cache" | "L1i cache" | "L2 cache" | "L3 cache" => lscpu_caches.push(CpuCache { level: k.trim_end_matches(" cache").to_string(), size: v.split(" (").next().unwrap_or(v).to_string() }),
            _ => {}
        }
    }
    info.sockets = sockets;
    info.logical_cpus = logical;
    // Physical cores: sockets times cores per socket. Without a socket count on a single-socket machine (ARM boards say
    // nothing), threads-per-core can still give it; otherwise it stays unknown rather than being guessed.
    info.physical_cores = match (sockets, cores_per_socket) {
        (Some(s), Some(c)) => Some(s * c),
        (None, Some(c)) => Some(c),
        _ => match (logical, threads_per_core) {
            (Some(l), Some(t)) if t > 0 && l % t == 0 => Some(l / t),
            _ => None,
        },
    };
    info.caches = if lscpu_caches.is_empty() { sysfs_caches } else { lscpu_caches };
    info
}

// -- temperatures --------------------------------------------------------------------------------------------------

/// What a sensor measures, from the driver that owns it. A label is used only to tell a CPU's package from its cores;
/// it never turns another driver's sensor into a CPU's.
pub fn temp_kind(chip: &str, label: &str) -> TempKind {
    let chip = chip.to_ascii_lowercase();
    let label = label.to_ascii_lowercase();
    let starts = |ps: &[&str]| ps.iter().any(|p| chip.starts_with(p));
    match chip.as_str() {
        "coretemp" if label.starts_with("package") => TempKind::CpuPackage,
        "coretemp" => TempKind::CpuCore,
        "k10temp" | "zenpower" if label.starts_with("tctl") || label.starts_with("tdie") => TempKind::CpuPackage,
        "k10temp" | "zenpower" => TempKind::CpuCore,
        "cpu_thermal" | "cpu-thermal" | "x86_pkg_temp" => TempKind::CpuPackage,
        "nvme" | "drivetemp" => TempKind::Storage,
        "amdgpu" | "nouveau" | "radeon" | "nvidia" => TempKind::Gpu,
        "acpitz" | "thinkpad" | "dell_smm" | "applesmc" => TempKind::System,
        _ if starts(&["pch_", "it8", "nct", "w83", "asus", "f71", "lm7", "adt7"]) => TempKind::System,
        _ if starts(&["iwlwifi", "ath", "mt76", "mt79", "r8169", "rtw"]) => TempKind::Network,
        _ if starts(&["bat", "battery", "ucsi"]) => TempKind::Battery,
        _ => TempKind::Other,
    }
}

/// Milli-degrees to degrees, refusing what is not a temperature: unconnected sensors report 0 or absurd values.
fn celsius(milli: &str) -> Option<f64> {
    let v: f64 = milli.trim().parse::<i64>().ok()? as f64 / 1000.0;
    (v > -50.0 && v < 150.0 && v != 0.0).then_some(v)
}

fn parse_temps(lines: &[&str]) -> Vec<TempSensor> {
    let mut out: Vec<TempSensor> = Vec::new();
    let mut zones: Vec<(String, Option<f64>)> = Vec::new();
    for l in lines {
        let p: Vec<&str> = l.split('|').collect();
        match p.first().copied() {
            // H|chip|tempN|label|value|max|crit
            Some("H") if p.len() >= 7 && out.len() < 256 => {
                let chip = p[1].trim();
                if chip.is_empty() || chip.len() > 40 {
                    continue;
                }
                let label = if p[3].trim().is_empty() { p[2].trim() } else { p[3].trim() };
                out.push(TempSensor {
                    source: "hwmon".into(),
                    chip: chip.to_string(),
                    label: label.chars().take(40).collect(),
                    kind: temp_kind(chip, p[3]),
                    celsius: celsius(p[4]),
                    high: celsius(p[5]),
                    critical: celsius(p[6]),
                });
            }
            Some("Z") if p.len() >= 3 && zones.len() < 64 => zones.push((p[1].trim().to_string(), celsius(p[2]))),
            _ => {}
        }
    }
    // A thermal zone is a second view of something the kernel often also lists as a hwmon chip (`acpitz`, the CPU
    // package): it is added only when no hwmon sensor of the same chip, or of the same kind for the CPU, is there.
    for (ty, c) in zones {
        let kind = temp_kind(&ty, "");
        let dup = out.iter().any(|t| t.chip.eq_ignore_ascii_case(&ty)) || (kind == TempKind::CpuPackage && out.iter().any(|t| t.kind == TempKind::CpuPackage));
        if !dup && !ty.is_empty() && ty.len() <= 40 {
            out.push(TempSensor { source: "thermal".into(), chip: ty.clone(), label: ty, kind, celsius: c, high: None, critical: None });
        }
    }
    out
}

// -- GPUs ------------------------------------------------------------------------------------------------------------

fn parse_amd(lines: &[&str]) -> Vec<Gpu> {
    let mut out = Vec::new();
    for l in lines {
        let p: Vec<&str> = l.split('|').collect();
        // A|card|product|pci|driver|busy|vram_used|vram_total|temp|power_uW|cap_uW|pwm|pwm_max
        if p.first() != Some(&"A") || p.len() < 13 || out.len() >= 16 {
            continue;
        }
        let name = if p[2].trim().is_empty() { format!("AMD GPU {}", p[3].trim()).trim().to_string() } else { p[2].trim().to_string() };
        let pwm_max = u(p[12]).filter(|m| *m > 0).unwrap_or(255);
        out.push(Gpu {
            id: format!("amd{}", out.len()),
            vendor: GpuVendor::Amd,
            name: name.chars().take(80).collect(),
            driver: (!p[4].trim().is_empty()).then(|| p[4].trim().chars().take(30).collect()),
            util_pct: u(p[5]).map(|v| v.min(100) as f64),
            mem_util_pct: None,
            vram_used: u(p[6]),
            vram_total: u(p[7]).filter(|t| *t > 0),
            celsius: celsius(p[8]),
            power_watts: u(p[9]).map(|uw| uw as f64 / 1e6),
            power_limit_watts: u(p[10]).filter(|c| *c > 0).map(|uw| uw as f64 / 1e6),
            fan_pct: u(p[11]).map(|v| (v as f64 * 100.0 / pwm_max as f64).min(100.0)),
            processes: Vec::new(),
        });
    }
    out
}

/// `nvidia-smi` prints "[N/A]" or "[Not Supported]" for what a card does not report; that is "unavailable".
fn smi(v: &str) -> Option<f64> {
    let v = v.trim();
    if v.starts_with('[') {
        None
    } else {
        f(v)
    }
}

fn parse_nvidia(gpu_lines: &[&str], app_lines: &[&str]) -> (Vec<Gpu>, Vec<String>) {
    let mut notes = Vec::new();
    let mut gpus: Vec<(Gpu, String)> = Vec::new();
    for l in gpu_lines {
        if let Some(rc) = l.strip_prefix("rc=") {
            match rc.trim() {
                "0" => {}
                "124" => notes.push("nvidia-smi did not answer within 5 seconds".to_string()),
                rc => notes.push(format!("nvidia-smi failed (status {rc})")),
            }
            continue;
        }
        let p: Vec<&str> = l.split(", ").collect();
        // index, name, util.gpu, util.mem, mem.used, mem.total, temp, power.draw, power.limit, fan, driver, uuid
        if p.len() < 12 || gpus.len() >= 16 {
            continue;
        }
        let mib = |v: &str| smi(v).map(|m| (m * 1024.0 * 1024.0) as u64);
        gpus.push((
            Gpu {
                id: format!("nvidia{}", p[0].trim()),
                vendor: GpuVendor::Nvidia,
                name: p[1].trim().chars().take(80).collect(),
                driver: (!p[10].trim().is_empty()).then(|| p[10].trim().chars().take(30).collect()),
                util_pct: smi(p[2]),
                mem_util_pct: smi(p[3]),
                vram_used: mib(p[4]),
                vram_total: mib(p[5]).filter(|t| *t > 0),
                celsius: smi(p[6]).filter(|t| *t > -50.0 && *t < 150.0),
                power_watts: smi(p[7]),
                power_limit_watts: smi(p[8]),
                fan_pct: smi(p[9]),
                processes: Vec::new(),
            },
            p[11].trim().to_string(),
        ));
    }
    for l in app_lines {
        let p: Vec<&str> = l.split(", ").collect();
        if p.len() < 4 {
            continue;
        }
        let Some(pid) = u(p[0]).and_then(|v| u32::try_from(v).ok()) else { continue };
        let name = p[1].trim().rsplit('/').next().unwrap_or("").chars().take(60).collect::<String>();
        if let Some((g, _)) = gpus.iter_mut().find(|(_, uuid)| uuid == p[3].trim()) {
            if g.processes.len() < 64 {
                g.processes.push(GpuProcess { pid, name, vram: smi(p[2]).map(|m| (m * 1024.0 * 1024.0) as u64) });
            }
        }
    }
    (gpus.into_iter().map(|(g, _)| g).collect(), notes)
}

pub fn parse(stdout: &str) -> Result<Raw, ParseError> {
    if stdout.len() > MAX_OUTPUT_BYTES {
        return Err(ParseError::TooLarge);
    }
    if stdout.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let s = sections(stdout);
    if !s.contains_key("END") {
        return Err(ParseError::Incomplete);
    }
    let get = |k: &str| s.get(k).map(Vec::as_slice).unwrap_or(&[]);
    let os = get("OS").iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").chars().take(40).collect::<String>();
    let linux = os == "Linux";

    let uptime = get("UPTIME").first().and_then(|l| f(l.split_whitespace().next()?)).filter(|v| *v >= 0.0);
    let (cpu_total, cpu_cores) = parse_stat(get("STAT"));
    let mem_kb = parse_meminfo(get("MEMINFO"));
    let LoadAvg { load, running, threads } = parse_loadavg(get("LOADAVG"));
    let processes = get("PROCS").first().and_then(|l| l.trim().parse::<u32>().ok()).filter(|n| *n > 0);
    let freqs: Vec<f64> = get("CPUFREQ").iter().filter_map(|l| u(l)).map(|k| k as f64 / 1000.0).filter(|m| *m > 0.0).collect();
    let freq_mhz = (!freqs.is_empty()).then(|| freqs.iter().sum::<f64>() / freqs.len() as f64);
    let net = parse_netdev(get("NETDEV"));
    let (ifaces, lower) = parse_netif(get("NETIF"));
    let default_iface = get("ROUTE").iter().map(|l| l.trim()).find(|l| name_ok(l)).map(str::to_string);
    let disk = parse_diskstats(get("DISKSTATS"));
    let block = parse_block(get("BLOCK"));
    let filesystems = parse_df(get("DF"));
    let tools: Vec<String> = get("TOOLS").iter().map(|l| l.trim()).filter(|l| name_ok(l)).map(str::to_string).collect();
    let temps = parse_temps(get("TEMPS"));
    let amd = parse_amd(get("AMDGPU"));
    let mut notes = Vec::new();
    let nvidia = if s.contains_key("NVIDIA") {
        let (g, n) = parse_nvidia(get("NVIDIA"), get("NVAPPS"));
        notes.extend(n);
        Some(g)
    } else {
        None
    };
    if s.contains_key("NVSKIP") {
        notes.push("This host has nvidia-smi but no `timeout` command, so it is not run (a stuck driver could hang every reading).".to_string());
    }
    let info = s.contains_key("STATIC").then(|| parse_static(get("STATIC"), get("CPUINFO"), get("LSCPU")));

    let capabilities = Capabilities {
        proc_stat: cpu_total.is_some(),
        meminfo: mem_kb.contains_key("MemTotal"),
        loadavg: load.is_some(),
        net_dev: !net.is_empty(),
        diskstats: !disk.is_empty(),
        sys_block: !block.is_empty(),
        sys_net: !ifaces.is_empty(),
        filesystems: !filesystems.is_empty(),
        cpufreq: freq_mhz.is_some(),
        sensors: !temps.is_empty(),
        amd_gpu: !amd.is_empty(),
        nvidia_smi: tools.iter().any(|t| t == "nvidia-smi"),
        tools,
    };
    Ok(Raw { os, linux, uptime, cpu_total, cpu_cores, mem_kb, load, running, threads, processes, freq_mhz, net, ifaces, lower, default_iface, disk, block, filesystems, temps, amd, nvidia, notes, capabilities, info })
}
