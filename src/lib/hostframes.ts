// What the Host Metrics views draw: the shared engine's readings (hostsample.ts) reduced to one small frame each, kept
// per host in memory while the views are open. A frame holds only what was measured: a rate that could not be measured
// is null, so a chart shows a gap, never a drop to zero.
import { cpuTemperature, gpuVramPct, type HostSample, type HostSamplePoint } from "./hostsample";

export interface Frame {
  /** Milliseconds since 1970 when the reading was taken. */
  t: number;
  cpu: number | null;
  mem: number | null;
  load1: number | null;
  /** Bytes per second over the host's counted interfaces (see `Network.total`). */
  rx: number | null;
  tx: number | null;
  /** Bytes per second summed over the real (not virtual) disks. */
  diskRead: number | null;
  diskWrite: number | null;
  /** Busy share of the busiest real disk. */
  diskBusy: number | null;
  /** The CPU's own temperature (see `cpuTemperature`), degrees Celsius. */
  temp: number | null;
  /** The busiest GPU's utilisation. */
  gpu: number | null;
  /** Per GPU: [utilisation, VRAM use, temperature]. */
  gpus: Record<string, [number | null, number | null, number | null]>;
  /** Per interface: [received, sent] bytes per second. */
  ifaces: Record<string, [number | null, number | null]>;
}

/** How far back frames are kept, and the most of them. At the 2-second live rate that is about an hour. */
export const FRAME_WINDOW_MS = 3_600_000;
export const FRAME_MAX = 2000;
/** A hole longer than this in the data is drawn as a hole. */
export const FRAME_GAP_MS = 30_000;

const sum = (v: (number | null)[]): number | null => {
  const n = v.filter((x): x is number => x !== null);
  // Unknown if any disk has no figure: a sum missing a disk would be too low without saying so.
  return n.length && n.length === v.length ? n.reduce((a, b) => a + b, 0) : null;
};

export function frameOf(s: HostSample): Frame {
  const real = s.disks.filter((d) => !d.virtual_device);
  const busy = real.map((d) => d.busy_pct).filter((x): x is number => x !== null);
  const ifaces: Frame["ifaces"] = {};
  for (const i of s.network?.interfaces ?? []) ifaces[i.name] = [i.rx_bps, i.tx_bps];
  const gpus: Frame["gpus"] = {};
  for (const g of s.gpus) gpus[g.id] = [g.util_pct, gpuVramPct(g), g.celsius];
  const util = s.gpus.map((g) => g.util_pct).filter((x): x is number => x !== null);
  return {
    t: s.taken_at_ms,
    cpu: s.cpu?.total_pct ?? null,
    mem: s.memory?.used_pct ?? null,
    load1: s.load?.m1 ?? null,
    rx: s.network?.total?.rx_bps ?? null,
    tx: s.network?.total?.tx_bps ?? null,
    diskRead: real.length ? sum(real.map((d) => d.read_bps)) : null,
    diskWrite: real.length ? sum(real.map((d) => d.write_bps)) : null,
    diskBusy: busy.length ? Math.max(...busy) : null,
    temp: cpuTemperature(s),
    gpu: util.length ? Math.max(...util) : null,
    gpus,
    ifaces,
  };
}

/** A point from the engine's kept history: totals only (it has no per-interface figures). */
export const frameOfPoint = (p: HostSamplePoint): Frame => ({ t: p.t, cpu: p.cpu, mem: p.mem, load1: p.load1, rx: p.rx_bps, tx: p.tx_bps, diskRead: null, diskWrite: null, diskBusy: p.disk_busy, temp: p.temp_cpu, gpu: p.gpu_util, gpus: {}, ifaces: {} });

/**
 * The list with `f` added, in time order. A frame already there (same time: the same reading arrived by the event and by
 * the reply) is not added twice, and one older than the newest is placed where it belongs. Old frames fall off the front.
 * Returns a new array so a reactive reader sees the change.
 */
export function addFrame(list: Frame[], f: Frame, windowMs = FRAME_WINDOW_MS, max = FRAME_MAX): Frame[] {
  const last = list[list.length - 1];
  let next: Frame[];
  if (!last || f.t > last.t) next = [...list, f];
  else if (list.some((x) => x.t === f.t)) return list;
  else next = [...list, f].sort((a, b) => a.t - b.t);
  const from = next[next.length - 1].t - windowMs;
  let start = 0;
  while (start < next.length - 1 && next[start].t < from) start++;
  const kept = start > 0 ? next.slice(start) : next;
  return kept.length > max ? kept.slice(kept.length - max) : kept;
}

/** Put the engine's kept history in front of what the window has seen itself, without repeating a moment. */
export function backfill(list: Frame[], points: HostSamplePoint[], windowMs = FRAME_WINDOW_MS): Frame[] {
  const first = list[0]?.t ?? Infinity;
  const older = points.map(frameOfPoint).filter((f) => f.t < first);
  if (!older.length) return list;
  const all = [...older, ...list];
  const from = all[all.length - 1].t - windowMs;
  return all.filter((f) => f.t >= from).slice(-FRAME_MAX);
}

/** The frames of the last `ms` before `now`. */
export const within = (list: Frame[], now: number, ms: number): Frame[] => list.filter((f) => f.t >= now - ms && f.t <= now);

/** The highest value of a series over frames, or null if none was measured. */
export function peak(list: Frame[], pick: (f: Frame) => number | null): number | null {
  let m: number | null = null;
  for (const f of list) {
    const v = pick(f);
    if (v !== null && (m === null || v > m)) m = v;
  }
  return m;
}

export const ifacePick = (name: string, dir: "rx" | "tx") => (f: Frame): number | null => f.ifaces[name]?.[dir === "rx" ? 0 : 1] ?? null;

/** What the Network view shows when no interface is chosen: the counted interfaces added together. */
export const TOTAL = "__total__";

/** The interface to show first: the saved choice if it still exists, else the default route's, else the total. */
export function pickInterface(saved: string | null | undefined, names: string[], defaultIface: string | null, hasTotal: boolean): string {
  if (saved === TOTAL && hasTotal) return TOTAL;
  if (saved && names.includes(saved)) return saved;
  if (hasTotal) return TOTAL;
  if (defaultIface && names.includes(defaultIface)) return defaultIface;
  return names[0] ?? TOTAL;
}

/**
 * How much time a chart should cover: the range asked for, but no more than what there is (and never under a minute),
 * so a window that has just opened shows a line, not a dot at the edge of fifteen empty minutes. It widens as readings
 * come in until it reaches the range.
 */
export function fitWindow(list: Frame[], now: number, full: number): number {
  const oldest = list.find((f) => f.t >= now - full)?.t ?? now;
  return Math.min(full, Math.max(60_000, now - oldest + 5_000));
}
