// The readings of the shared host-metrics engine (src-tauri/src/hostmetrics), as the window receives them, and the few
// pure helpers every view needs to show them the same way. Every figure that could not be measured is `null` and is shown
// as unavailable: never 0, never a guess.
//
// Not to be confused with `hostmetrics.ts` (the older 30-second summary) and `hostdetail.ts` (the detail window's own
// script): those predate the shared engine and are moved over to it view by view.
import type { Uuid } from "$lib/types";

export type Baseline =
  | { kind: "first" }
  | { kind: "rebooted" }
  | { kind: "stale"; gap_secs: number }
  | { kind: "too_close" }
  | { kind: "measured"; elapsed_secs: number };

export type Platform = { kind: "linux" } | { kind: "unsupported"; os: string };

export interface Capabilities {
  proc_stat: boolean;
  meminfo: boolean;
  loadavg: boolean;
  net_dev: boolean;
  diskstats: boolean;
  sys_block: boolean;
  sys_net: boolean;
  filesystems: boolean;
  cpufreq: boolean;
  /** Temperature sensors are exposed. Virtual machines usually have none. */
  sensors: boolean;
  amd_gpu: boolean;
  nvidia_smi: boolean;
  tools: string[];
}

export interface CpuUsage {
  total_pct: number | null;
  iowait_pct: number | null;
  steal_pct: number | null;
  cores: { index: number; pct: number | null }[];
  logical: number;
  freq_mhz: number | null;
}

export interface Memory {
  total: number;
  used: number;
  available: number;
  free: number;
  buffers: number;
  cached: number;
  shared: number;
  swap_total: number;
  swap_used: number;
  committed: number | null;
  slab: number | null;
  used_pct: number;
  available_exact: boolean;
}

export interface Filesystem {
  device: string;
  mount: string;
  fstype: string | null;
  total: number;
  used: number;
  available: number;
  used_pct: number;
}

export interface DiskIo {
  name: string;
  model: string | null;
  size: number | null;
  rotational: boolean | null;
  virtual_device: boolean;
  read_bps: number | null;
  write_bps: number | null;
  read_iops: number | null;
  write_iops: number | null;
  busy_pct: number | null;
  await_ms: number | null;
  read_total: number;
  write_total: number;
}

export type IfaceKind = "physical" | "wireless" | "bond" | "bridge" | "vlan" | "tunnel" | "container" | "loopback" | "other";

export interface NetIface {
  name: string;
  kind: IfaceKind;
  state: string;
  master: string | null;
  mtu: number | null;
  link_mbps: number | null;
  rx_bps: number | null;
  tx_bps: number | null;
  rx_pps: number | null;
  tx_pps: number | null;
  rx_bytes: number;
  tx_bytes: number;
  rx_errors: number;
  tx_errors: number;
  rx_drops: number;
  tx_drops: number;
  /** Whether this interface is part of `Network.total`. */
  counted: boolean;
}

export interface Network {
  interfaces: NetIface[];
  default_iface: string | null;
  total: { rx_bps: number; tx_bps: number; from: string[] } | null;
}

export interface StaticInfo {
  hostname: string | null;
  os_name: string | null;
  kernel: string | null;
  arch: string | null;
  cpu_model: string | null;
  sockets: number | null;
  physical_cores: number | null;
  logical_cpus: number | null;
  max_mhz: number | null;
  base_mhz: number | null;
  virtualization: string | null;
  hypervisor: string | null;
  caches: { level: string; size: string }[];
}

export interface HostSample {
  host_id: Uuid;
  /** Milliseconds since 1970 on this computer when the reading arrived. */
  taken_at_ms: number;
  platform: Platform;
  baseline: Baseline;
  uptime_secs: number | null;
  cpu: CpuUsage | null;
  memory: Memory | null;
  load: { m1: number; m5: number; m15: number } | null;
  processes: { processes: number | null; threads: number | null; running: number | null } | null;
  filesystems: Filesystem[];
  disks: DiskIo[];
  network: Network | null;
  temperatures: TempSensor[];
  gpus: Gpu[];
  /** Things the host could not do that are worth telling the person (a GPU tool that timed out, say). */
  notes: string[];
  capabilities: Capabilities;
  info: StaticInfo;
}

export type TempKind = "cpu_package" | "cpu_core" | "gpu" | "storage" | "system" | "network" | "battery" | "other";

export interface TempSensor {
  source: string;
  chip: string;
  label: string;
  kind: TempKind;
  celsius: number | null;
  high: number | null;
  critical: number | null;
}

export interface GpuProcess {
  pid: number;
  name: string;
  vram: number | null;
}

export interface Gpu {
  /** Stable across readings: nvidia0, amd0. */
  id: string;
  vendor: "nvidia" | "amd";
  name: string;
  driver: string | null;
  util_pct: number | null;
  mem_util_pct: number | null;
  vram_used: number | null;
  vram_total: number | null;
  celsius: number | null;
  power_watts: number | null;
  power_limit_watts: number | null;
  fan_pct: number | null;
  processes: GpuProcess[];
}

export interface ProcessInfo {
  pid: number;
  ppid: number;
  user: string;
  state: string;
  name: string;
  command: string;
  /** Share of one core, so a busy multi-threaded program passes 100. Null until two readings exist. */
  cpu_pct: number | null;
  rss: number;
  mem_pct: number | null;
  threads: number;
  /** With the pid, what identifies a process: a reused pid has a different start. */
  start_ticks: number;
  age_secs: number | null;
}

export interface ProcessList {
  host_id: Uuid;
  taken_at_ms: number;
  total: number;
  truncated: boolean;
  baseline: Baseline;
  commands: boolean;
  items: ProcessInfo[];
}

export interface ListeningPort {
  proto: "tcp" | "udp";
  address: string;
  port: number;
  ipv6: boolean;
  pid: number | null;
  process: string | null;
  user: string | null;
  state: string;
  /** Bound to the host's own loopback: reachable only from the host, or through a tunnel. */
  local_only: boolean;
}

export interface PortList {
  host_id: Uuid;
  taken_at_ms: number;
  source: "ss" | "netstat" | "proc";
  owners_hidden: boolean;
  items: ListeningPort[];
}

export interface LoginEntry {
  user: string;
  tty: string;
  from: string | null;
  started_ms: number | null;
  started: string;
  ended: string;
}

export interface AuthEvent {
  kind: "accepted" | "failed" | "invalid_user" | "other";
  user: string | null;
  from: string | null;
  method: string | null;
  time: string;
}

export interface LoginReport {
  host_id: Uuid;
  taken_at_ms: number;
  sessions: LoginEntry[];
  recent: LoginEntry[];
  failed_logins: LoginEntry[];
  auth: AuthEvent[];
  auth_source: string | null;
  limits: { what: string; reason: string }[];
}

export type SignalName = "TERM" | "KILL" | "HUP" | "INT" | "STOP" | "CONT" | "USR1" | "USR2";

export type SignalOutcome = { kind: "sent" } | { kind: "gone" } | { kind: "changed" } | { kind: "refused"; reason: string } | { kind: "unknown" };

export interface HostSamplePoint {
  t: number;
  cpu: number | null;
  mem: number | null;
  disk_used: number | null;
  load1: number | null;
  rx_bps: number | null;
  tx_bps: number | null;
  disk_busy: number | null;
  temp_cpu: number | null;
  gpu_util: number | null;
}

export interface LatestSample {
  sample: HostSample;
  age_ms: number;
}

export type MetricsFailure = { kind: "exec" | "unreadable"; message: string } | { kind: "timeout" } | { kind: "cancelled" } | { kind: "cooling"; retry_in_secs: number; last: string };

export interface MetricsFailureEvent {
  host_id: Uuid;
  error: MetricsFailure;
  message: string;
}

// -- showing it ------------------------------------------------------------------------------------------------

export const UNAVAILABLE = "Unavailable";

/** Percent with one decimal below 100: "24.8%". Null is "Unavailable". */
export function pctText(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return UNAVAILABLE;
  return `${v >= 99.95 ? Math.round(v) : v.toFixed(1)}%`;
}

export type RateUnit = "bytes" | "bits";

/**
 * A speed, with its unit spelled out so MB/s (megabytes) is never read as Mbps (megabits). `bytes` per second is the
 * default; `bits` multiplies by 8 and says Kbps/Mbps/Gbps. Decimal prefixes (1 MB = 1,000,000 B), as network gear does.
 * Null is "Unavailable", not "0 B/s".
 */
export function rateText(bps: number | null | undefined, unit: RateUnit = "bytes"): string {
  if (bps === null || bps === undefined || !Number.isFinite(bps) || bps < 0) return UNAVAILABLE;
  const bits = unit === "bits";
  let v = bits ? bps * 8 : bps;
  const steps = bits ? ["bps", "Kbps", "Mbps", "Gbps", "Tbps"] : ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];
  let i = 0;
  while (v >= 1000 && i < steps.length - 1) {
    v /= 1000;
    i++;
  }
  return `${i === 0 ? Math.round(v) : v >= 100 ? Math.round(v) : v.toFixed(v >= 10 ? 1 : 2)} ${steps[i]}`;
}

/** A temperature: "48 °C". Null is unavailable. */
export function tempText(c: number | null | undefined): string {
  return c === null || c === undefined || !Number.isFinite(c) ? UNAVAILABLE : `${Math.round(c)} °C`;
}

/**
 * The CPU's temperature: the package sensor, else the hottest core. Only sensors the driver says are a CPU's count, so a
 * motherboard or disk sensor is never shown as the processor's. Mirrors the engine's own `cpu_temperature`.
 */
export function cpuTemperature(s: HostSample): number | null {
  const of = (k: TempKind) => {
    const v = s.temperatures.filter((t) => t.kind === k && t.celsius !== null).map((t) => t.celsius as number);
    return v.length ? Math.max(...v) : null;
  };
  return of("cpu_package") ?? of("cpu_core");
}

export const gpuVramPct = (g: Gpu): number | null => (g.vram_used !== null && g.vram_total ? (g.vram_used * 100) / g.vram_total : null);

/** A size or total in bytes, decimal prefixes for traffic. */
export function bytesText(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n) || n < 0) return UNAVAILABLE;
  const steps = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = n;
  let i = 0;
  while (v >= 1000 && i < steps.length - 1) {
    v /= 1000;
    i++;
  }
  return `${i === 0 ? Math.round(v) : v >= 100 ? Math.round(v) : v.toFixed(v >= 10 ? 1 : 2)} ${steps[i]}`;
}

/** A memory size in binary units (the kernel's kB are KiB): "16.0 GiB". */
export function memText(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n) || n < 0) return UNAVAILABLE;
  const steps = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < steps.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? Math.round(v) : v >= 100 ? Math.round(v) : v.toFixed(1)} ${steps[i]}`;
}

/** Why the rates in a sample are missing, in a sentence; null when they are there. */
export function baselineNote(b: Baseline): string | null {
  switch (b.kind) {
    case "measured":
      return null;
    case "first":
      return "First reading: speeds and CPU use appear after the next one.";
    case "rebooted":
      return "The host restarted since the last reading, so speeds and CPU use start again.";
    case "stale":
      return `The last reading was ${Math.round(b.gap_secs / 60)} min ago: too long ago for a speed to mean anything.`;
    case "too_close":
      return "Read too soon after the last one to measure a speed.";
  }
}

export const hasRates = (s: HostSample): boolean => s.baseline.kind === "measured";

/** A reading is stale when it is older than a few of its own intervals (and always after 2 minutes). */
export function isStale(ageMs: number, intervalMs: number): boolean {
  return ageMs > Math.min(Math.max(intervalMs * 3, 10_000), 120_000);
}

export type RankBy = "cpu" | "mem" | "net" | "disk";

export interface Ranked {
  hostId: Uuid;
  /** The figure the list is ordered by, in the unit of `by`: percent for cpu, mem and disk, bytes per second for net. */
  value: number;
  by: RankBy;
  stale: boolean;
}

/** The figure of one kind in a sample, or null when it was not measured. Disk is the busiest real disk's busy share. */
export function figure(s: HostSample, by: RankBy): number | null {
  switch (by) {
    case "cpu":
      return s.cpu?.total_pct ?? null;
    case "mem":
      return s.memory?.used_pct ?? null;
    case "net": {
      const t = s.network?.total;
      return t ? t.rx_bps + t.tx_bps : null;
    }
    case "disk": {
      const v = s.disks.filter((d) => !d.virtual_device).map((d) => d.busy_pct).filter((p): p is number => p !== null);
      return v.length ? Math.max(...v) : null;
    }
  }
}

/**
 * The hosts ranked by one metric, from samples already collected (it connects to nothing). Hosts with no figure for it
 * are left out of the ranking rather than put at the bottom as zeros. Values of different kinds are never combined.
 */
export function busiest(latest: LatestSample[], by: RankBy, intervalMs = 30_000, limit = 5): Ranked[] {
  const out: Ranked[] = [];
  for (const l of latest) {
    const value = figure(l.sample, by);
    if (value !== null) out.push({ hostId: l.sample.host_id, value, by, stale: isStale(l.age_ms, intervalMs) });
  }
  return out.sort((a, b) => b.value - a.value).slice(0, limit);
}
