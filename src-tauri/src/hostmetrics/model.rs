//! The typed readings of a host. Everything that can be unknown is an `Option`: a figure that could not be measured
//! (the first sample has nothing to subtract from, a counter was reset, the host has no such interface) is `None` and
//! is shown as "unavailable", never as 0.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Why the rates in a sample are (or are not) there. Rates are changes between two readings of the host's counters, so
/// they need a baseline reading from the same boot, not too long ago.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Baseline {
    /// The first reading of this host (or since the app forgot it): there is nothing to compare with yet.
    First,
    /// The host's uptime went backwards: it rebooted, so every counter started again.
    Rebooted,
    /// The previous reading was too long ago for a rate to mean anything (the connection was down, or sampling paused).
    Stale { gap_secs: f64 },
    /// The previous reading was too close in time to measure a rate over.
    TooClose,
    /// Rates are the change since the previous reading, `elapsed_secs` of the host's own clock earlier.
    Measured { elapsed_secs: f64 },
}

impl Baseline {
    pub fn has_rates(&self) -> bool {
        matches!(self, Baseline::Measured { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Platform {
    Linux,
    /// A host this engine has no adapter for yet (macOS, the BSDs, Windows, RouterOS...). Nothing is guessed for it.
    Unsupported { os: String },
}

/// What the host offers, found from the same reading (no extra round trip). Later collectors use it to decide what to run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub proc_stat: bool,
    pub meminfo: bool,
    pub loadavg: bool,
    pub net_dev: bool,
    pub diskstats: bool,
    pub sys_block: bool,
    pub sys_net: bool,
    pub filesystems: bool,
    pub cpufreq: bool,
    /// Temperature sensors are exposed (`/sys/class/hwmon` or `/sys/class/thermal`). Virtual machines usually have none.
    pub sensors: bool,
    /// An AMD GPU with the `amdgpu` driver's counters was found.
    pub amd_gpu: bool,
    /// `nvidia-smi` is installed. It is only run under `timeout`, so a host without `timeout` is not asked.
    pub nvidia_smi: bool,
    /// Command-line tools found (from a fixed list): `ss`, `ip`, `lsblk`, `lscpu`, `systemctl`, `journalctl`,
    /// `sensors`, `nvidia-smi`, `rocm-smi`, `timeout`, `ufw`, `nft`, `iptables`.
    pub tools: Vec<String>,
}

impl Capabilities {
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreUsage {
    pub index: u32,
    pub pct: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuUsage {
    /// Percent of all logical CPUs that were busy since the previous reading (iowait counts as idle).
    pub total_pct: Option<f64>,
    pub iowait_pct: Option<f64>,
    /// Time taken by the hypervisor for other guests.
    pub steal_pct: Option<f64>,
    pub cores: Vec<CoreUsage>,
    pub logical: u32,
    /// Mean current frequency over the cores that report one.
    pub freq_mhz: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub total: u64,
    /// `total - available`: what programs hold that the kernel cannot hand back cheaply.
    pub used: u64,
    /// What a new program could get without swapping (the kernel's `MemAvailable`, or an estimate on old kernels).
    pub available: u64,
    pub free: u64,
    pub buffers: u64,
    /// Page cache and reclaimable slab, minus shared memory (which sits in "Cached" but cannot be dropped).
    pub cached: u64,
    pub shared: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub committed: Option<u64>,
    pub slab: Option<u64>,
    pub used_pct: f64,
    /// True when `available` is the kernel's own figure rather than an estimate.
    pub available_exact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub m1: f64,
    pub m5: f64,
    pub m15: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessCounts {
    pub processes: Option<u32>,
    /// Kernel scheduling entities (threads), from `/proc/loadavg`.
    pub threads: Option<u32>,
    pub running: Option<u32>,
}

/// Space on a mounted filesystem. This is how full it is, not how busy its disk is: see [`DiskIo`] for activity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filesystem {
    pub device: String,
    pub mount: String,
    pub fstype: Option<String>,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    /// `used / (used + available)`, the way `df` shows it (space reserved for root is not counted as free).
    pub used_pct: f64,
}

/// Activity of one whole block device (not its partitions, not a device-mapper or RAID layer on top of it, so the same
/// traffic is never counted twice).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskIo {
    pub name: String,
    pub model: Option<String>,
    pub size: Option<u64>,
    pub rotational: Option<bool>,
    /// A layer above real disks (device-mapper, RAID, zram...). Listed, but left out of any total.
    pub virtual_device: bool,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub read_iops: Option<f64>,
    pub write_iops: Option<f64>,
    /// Share of the interval the device had I/O in flight. Not a capacity figure; SSDs with parallel queues can be
    /// "100% busy" and still have room.
    pub busy_pct: Option<f64>,
    /// Mean time an I/O took, in milliseconds. `None` when nothing was read or written in the interval.
    pub await_ms: Option<f64>,
    pub read_total: u64,
    pub write_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfaceKind {
    Physical,
    Wireless,
    Bond,
    Bridge,
    Vlan,
    /// WireGuard, OpenVPN, Tailscale and other tunnels.
    Tunnel,
    /// Docker, veth pairs, libvirt and other container or VM plumbing.
    Container,
    Loopback,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetIface {
    pub name: String,
    pub kind: IfaceKind,
    /// `up`, `down`, or whatever the kernel said (`unknown` is common for virtual devices).
    pub state: String,
    /// The bond or bridge this interface belongs to.
    pub master: Option<String>,
    pub mtu: Option<u32>,
    pub link_mbps: Option<u32>,
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    pub rx_pps: Option<f64>,
    pub tx_pps: Option<f64>,
    /// Counters since the interface appeared (usually the boot).
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_drops: u64,
    pub tx_drops: u64,
    /// Whether this interface's traffic is part of [`Network::total`].
    pub counted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetTotal {
    pub rx_bps: f64,
    pub tx_bps: f64,
    /// The interfaces added together. Bridges, tunnels, containers and members of a bond are not, because their traffic
    /// also appears on the device underneath.
    pub from: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    pub interfaces: Vec<NetIface>,
    /// The interface the default route leaves by.
    pub default_iface: Option<String>,
    pub total: Option<NetTotal>,
}

/// Facts that do not change between readings. Read at the first sample and about once an hour after.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StaticInfo {
    pub hostname: Option<String>,
    pub os_name: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
    pub cpu_model: Option<String>,
    pub sockets: Option<u32>,
    pub physical_cores: Option<u32>,
    pub logical_cpus: Option<u32>,
    pub max_mhz: Option<f64>,
    pub base_mhz: Option<f64>,
    /// `vmx` or `svm` is in the CPU's flags (hardware virtualization is available to this system).
    pub virtualization: Option<String>,
    pub hypervisor: Option<String>,
    pub caches: Vec<CpuCache>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuCache {
    pub level: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub host_id: Uuid,
    /// Wall-clock time the reading arrived, milliseconds since 1970. For labels and charts; rates use the host's clock.
    pub taken_at_ms: u64,
    pub platform: Platform,
    pub baseline: Baseline,
    pub uptime_secs: Option<f64>,
    pub cpu: Option<CpuUsage>,
    pub memory: Option<Memory>,
    pub load: Option<Load>,
    pub processes: Option<ProcessCounts>,
    pub filesystems: Vec<Filesystem>,
    pub disks: Vec<DiskIo>,
    pub network: Option<Network>,
    /// Every sensor the host exposes, with what it measures. Empty when it exposes none.
    pub temperatures: Vec<TempSensor>,
    /// GPUs with their own counters (NVIDIA through `nvidia-smi`, AMD through the driver's files). Empty when none.
    pub gpus: Vec<Gpu>,
    /// Things this host could not do that are worth telling the person (a GPU tool that timed out, a missing command).
    pub notes: Vec<String>,
    pub capabilities: Capabilities,
    pub info: StaticInfo,
}

/// The small record kept per reading for charts: enough to draw history without keeping whole samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryPoint {
    pub t: u64,
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    /// Use of the fullest real filesystem.
    pub disk_used: Option<f64>,
    pub load1: Option<f64>,
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    /// The busiest real disk's busy share.
    pub disk_busy: Option<f64>,
    /// The CPU's temperature in degrees Celsius, from a sensor known to be the CPU's (see [`TempKind`]).
    pub temp_cpu: Option<f64>,
    /// The busiest GPU's utilisation.
    pub gpu_util: Option<f64>,
}

impl HistoryPoint {
    pub fn of(s: &Sample) -> Self {
        let disk_used = s.filesystems.iter().map(|f| f.used_pct).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        let disk_busy = s.disks.iter().filter(|d| !d.virtual_device).filter_map(|d| d.busy_pct).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        Self {
            t: s.taken_at_ms,
            cpu: s.cpu.as_ref().and_then(|c| c.total_pct),
            mem: s.memory.as_ref().map(|m| m.used_pct),
            disk_used,
            load1: s.load.map(|l| l.m1),
            rx_bps: s.network.as_ref().and_then(|n| n.total.as_ref()).map(|t| t.rx_bps),
            tx_bps: s.network.as_ref().and_then(|n| n.total.as_ref()).map(|t| t.tx_bps),
            disk_busy,
            temp_cpu: s.cpu_temperature(),
            gpu_util: s.gpus.iter().filter_map(|g| g.util_pct).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))),
        }
    }
}

impl Sample {
    /// The CPU's temperature: the package sensor if there is one, else the die, else the hottest core. Only sensors
    /// whose kind is CPU count, so a motherboard or disk sensor is never passed off as the processor's.
    pub fn cpu_temperature(&self) -> Option<f64> {
        let of = |k: TempKind| self.temperatures.iter().filter(|t| t.kind == k).filter_map(|t| t.celsius).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        of(TempKind::CpuPackage).or_else(|| of(TempKind::CpuCore))
    }
}

// -- temperatures and GPUs ---------------------------------------------------------------------------------------------

/// What a sensor measures. Decided from the driver (chip) that owns it, never from a guess about its label, so a CPU
/// figure is only ever from a CPU's own sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TempKind {
    CpuPackage,
    CpuCore,
    Gpu,
    /// NVMe and SATA drives.
    Storage,
    /// Motherboard, chipset and ACPI zones: the machine, not the processor.
    System,
    Network,
    Battery,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempSensor {
    /// Where it was read: `hwmon` or `thermal`.
    pub source: String,
    /// The driver or zone type that owns it (`coretemp`, `k10temp`, `nvme`, `acpitz`...).
    pub chip: String,
    /// The sensor's own label (`Package id 0`, `Tctl`, `Composite`...), or a generic one when it has none.
    pub label: String,
    pub kind: TempKind,
    pub celsius: Option<f64>,
    /// The temperature the hardware says is high or critical, when it says.
    pub high: Option<f64>,
    pub critical: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuProcess {
    pub pid: u32,
    pub name: String,
    /// Video memory held, in bytes.
    pub vram: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    /// A stable id for charts: `nvidia0`, `amd0`.
    pub id: String,
    pub vendor: GpuVendor,
    pub name: String,
    pub driver: Option<String>,
    pub util_pct: Option<f64>,
    /// Share of time the memory controller was busy (NVIDIA only).
    pub mem_util_pct: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub celsius: Option<f64>,
    pub power_watts: Option<f64>,
    pub power_limit_watts: Option<f64>,
    pub fan_pct: Option<f64>,
    /// Compute programs using the GPU (NVIDIA; empty where the driver does not say).
    pub processes: Vec<GpuProcess>,
}

impl Gpu {
    pub fn vram_pct(&self) -> Option<f64> {
        match (self.vram_used, self.vram_total) {
            (Some(u), Some(t)) if t > 0 => Some(u as f64 * 100.0 / t as f64),
            _ => None,
        }
    }
}

// -- on-demand views: processes, listening ports, logins -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    /// The kernel's one-letter state: R running, S sleeping, D waiting on I/O, Z zombie, T stopped...
    pub state: String,
    /// The program's short name.
    pub name: String,
    /// The command line, cut to a bounded length. Can hold what was typed, so it is shown but never logged.
    pub command: String,
    /// Share of one CPU core over the interval (so a busy multi-threaded program passes 100). `None` until there are two
    /// readings to compare.
    pub cpu_pct: Option<f64>,
    pub rss: u64,
    pub mem_pct: Option<f64>,
    pub threads: u32,
    /// When it started, ticks after the host booted. With the pid this identifies a process: a pid reused later has a
    /// different start.
    pub start_ticks: u64,
    /// Seconds since it started, when the host's clock allows saying.
    pub age_secs: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessList {
    pub host_id: Uuid,
    pub taken_at_ms: u64,
    /// How many processes the host has; `items` may hold fewer.
    pub total: u32,
    pub truncated: bool,
    /// Why `cpu_pct` is missing, when it is.
    pub baseline: Baseline,
    /// Whether command lines could be read (they need `ps`).
    pub commands: bool,
    pub items: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListeningPort {
    pub proto: Proto,
    pub address: String,
    pub port: u16,
    pub ipv6: bool,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub user: Option<String>,
    /// `LISTEN` for TCP, `UNCONN` for a bound UDP socket.
    pub state: String,
    /// True when the address is the host's own loopback: reachable only from the host (or through a tunnel).
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortList {
    pub host_id: Uuid,
    pub taken_at_ms: u64,
    /// `ss`, `netstat` or `proc` (the kernel's tables, which name no process).
    pub source: String,
    /// Some owners are missing because the tool only shows a user their own processes.
    pub owners_hidden: bool,
    pub items: Vec<ListeningPort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginEntry {
    pub user: String,
    pub tty: String,
    /// The address or name the login came from, when the host recorded one.
    pub from: Option<String>,
    /// When it began, milliseconds since 1970, from the host's own clock and zone.
    pub started_ms: Option<u64>,
    pub started: String,
    /// Still logged in, or how it ended.
    pub ended: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Accepted,
    Failed,
    InvalidUser,
    Other,
}

/// One SSH authentication event. Nothing here says it is suspicious: a failed password is a failed password.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthEvent {
    pub kind: AuthKind,
    pub user: Option<String>,
    pub from: Option<String>,
    pub method: Option<String>,
    /// The log's own timestamp text.
    pub time: String,
}

/// A part of the report that could not be read, and why, in words a person can act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub what: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginReport {
    pub host_id: Uuid,
    pub taken_at_ms: u64,
    pub sessions: Vec<LoginEntry>,
    pub recent: Vec<LoginEntry>,
    pub failed_logins: Vec<LoginEntry>,
    pub auth: Vec<AuthEvent>,
    /// Where the auth events came from (`journal`, `/var/log/auth.log`...), or none.
    pub auth_source: Option<String>,
    pub limits: Vec<Limit>,
}
