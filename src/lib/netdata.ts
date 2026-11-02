// Network traffic of a host, as the Monitoring page shows it: speeds in the unit a person asked for, totals, the
// recent readings kept for a chart, and the geometry to draw them. The readings themselves come from the host's own
// interface counters (src-tauri/src/monitoring/net.rs); nothing here invents or smooths a value.
//
// RX is what the monitored host's interface received (a download, for that host) and TX what it sent (an upload).

export interface NetInterface {
  name: string;
  /** `up`, `down` or `unknown`. */
  state: string;
  /** A real device, not a bridge, a virtual pair or a tunnel. */
  physical: boolean;
  link_mbps: number | null;
  rx_bps: number | null;
  tx_bps: number | null;
  rx_pps: number | null;
  tx_pps: number | null;
  rx_total: number;
  tx_total: number;
  rx_errors: number;
  tx_errors: number;
  rx_drops: number;
  tx_drops: number;
}

export interface NetReading {
  interfaces: NetInterface[];
  primary: string | null;
  window_secs: number;
}

// -- units -----------------------------------------------------------------------------------------------------------

/** Bytes per second (B/s, MB/s) or bits per second (bit/s, Mbit/s), the way network gear quotes a link. */
export type RateUnit = "bytes" | "bits";

const BYTE_UNITS = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];
const BIT_UNITS = ["bit/s", "Kbit/s", "Mbit/s", "Gbit/s", "Tbit/s"];

/** A speed scaled to a readable unit (powers of 1000, as links are quoted): `{ value: "12.8", unit: "MB/s" }`. */
export function formatRate(bytesPerSec: number | null, unit: RateUnit = "bytes"): { value: string; unit: string } {
  if (bytesPerSec === null || !Number.isFinite(bytesPerSec)) return { value: "–", unit: unit === "bytes" ? "B/s" : "bit/s" };
  const units = unit === "bytes" ? BYTE_UNITS : BIT_UNITS;
  let v = Math.max(0, unit === "bytes" ? bytesPerSec : bytesPerSec * 8);
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  // Whole numbers stay whole; a small one keeps a decimal so 0.4 MB/s isn't shown as 0.
  const text = i === 0 || v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2);
  return { value: text, unit: units[i] };
}

export function rateText(bytesPerSec: number | null, unit: RateUnit = "bytes"): string {
  const r = formatRate(bytesPerSec, unit);
  return bytesPerSec === null ? "–" : `${r.value} ${r.unit}`;
}

/** A total in bytes, powers of 1000: `24.8 GB`. */
export function formatTotal(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return "–";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = Math.max(0, bytes);
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${i === 0 || v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${units[i]}`;
}

/** Share of the link a speed fills, or null when the link's speed isn't known. */
export function utilization(bytesPerSec: number | null, linkMbps: number | null): number | null {
  if (bytesPerSec === null || !linkMbps || linkMbps <= 0) return null;
  return Math.min(100, (bytesPerSec * 8) / (linkMbps * 1_000_000) * 100);
}

// -- picking what to show ---------------------------------------------------------------------------------------------

/** The interface the headline numbers come from: the one named (the default route), else the first real device. */
export function headlineInterface(r: NetReading | null, named: string | null): NetInterface | null {
  if (!r) return null;
  return r.interfaces.find((i) => i.name === named) ?? null;
}

/** Interfaces in the order worth reading: the main one first, then by traffic, real devices before virtual ones. */
export function sortInterfaces(list: NetInterface[], headline: string | null): NetInterface[] {
  const busy = (i: NetInterface) => (i.rx_bps ?? 0) + (i.tx_bps ?? 0);
  return [...list].sort((a, b) => Number(b.name === headline) - Number(a.name === headline) || Number(b.physical) - Number(a.physical) || busy(b) - busy(a) || a.name.localeCompare(b.name));
}

// -- readings over time -----------------------------------------------------------------------------------------------

export interface NetPoint {
  /** Milliseconds since the epoch, on this computer's clock. */
  t: number;
  rx: number | null;
  tx: number | null;
}

/** How far back in-memory readings are kept, and the most per interface. */
export const KEEP_MS = 60 * 60_000;
export const KEEP_MAX = 3600;

/** The list with `p` added: out of order or a repeat of the last instant is merged, the oldest past the window dropped. */
export function addPoint(list: NetPoint[], p: NetPoint, windowMs = KEEP_MS, max = KEEP_MAX): NetPoint[] {
  const last = list[list.length - 1];
  let next: NetPoint[];
  if (last && p.t < last.t) next = [p];
  else if (last && p.t - last.t < 400) next = [...list.slice(0, -1), { t: p.t, rx: p.rx ?? last.rx, tx: p.tx ?? last.tx }];
  else next = [...list, p];
  const from = p.t - windowMs;
  let start = 0;
  while (start < next.length - 1 && next[start].t < from) start++;
  const kept = start ? next.slice(start) : next;
  return kept.length > max ? kept.slice(kept.length - max) : kept;
}

export interface Range {
  id: "5m" | "15m" | "1h" | "6h" | "24h" | "7d";
  label: string;
  ms: number;
}

export const RANGES: Range[] = [
  { id: "5m", label: "5 min", ms: 5 * 60_000 },
  { id: "15m", label: "15 min", ms: 15 * 60_000 },
  { id: "1h", label: "1 hour", ms: 60 * 60_000 },
  { id: "6h", label: "6 hours", ms: 6 * 3_600_000 },
  { id: "24h", label: "24 hours", ms: 24 * 3_600_000 },
  { id: "7d", label: "7 days", ms: 7 * 24 * 3_600_000 },
];

/** Which ranges can be answered: those within what is held in memory, and longer ones only if history is kept that long. */
export function rangeAvailable(r: Range, keep: "off" | "day" | "week"): { ok: boolean; why?: string } {
  if (r.ms <= KEEP_MS) return { ok: true };
  if (keep === "off") return { ok: false, why: "Turn on \"Keep history\" in Settings to look further back" };
  if (keep === "day" && r.ms > 24 * 3_600_000) return { ok: false, why: "History is kept for a day; choose a week in Settings for this" };
  return { ok: true };
}

export interface Bucketed {
  t: number;
  rx: number | null;
  tx: number | null;
}

/**
 * Readings reduced to at most `n` columns across [from, to]. Each column shows the **peak** of its readings, so a short
 * burst is never averaged away; a column with no reading is empty, so a hole in the data shows as a hole.
 */
export function bucketPeaks(points: NetPoint[], from: number, to: number, n: number): Bucketed[] {
  if (n < 1 || to <= from) return [];
  const width = (to - from) / n;
  const out: Bucketed[] = Array.from({ length: n }, (_, i) => ({ t: from + (i + 0.5) * width, rx: null, tx: null }));
  for (const p of points) {
    if (p.t < from || p.t > to) continue;
    const b = out[Math.min(n - 1, Math.floor((p.t - from) / width))];
    if (p.rx !== null) b.rx = b.rx === null ? p.rx : Math.max(b.rx, p.rx);
    if (p.tx !== null) b.tx = b.tx === null ? p.tx : Math.max(b.tx, p.tx);
  }
  return out;
}

/** A tidy top for the speed axis: 1, 2, 5 or 10 times a power of ten, never below `floor`. */
export function niceMax(v: number, floor = 1000): number {
  const m = Math.max(v, floor);
  const pow = 10 ** Math.floor(Math.log10(m));
  const f = m / pow;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * pow;
}

/** Ticks from 0 to `max` for the axis, `count` steps. */
export function ticks(max: number, count = 4): number[] {
  return Array.from({ length: count + 1 }, (_, i) => (max * i) / count);
}

export interface Box {
  width: number;
  height: number;
  from: number;
  to: number;
  max: number;
}

/**
 * SVG path runs for one series: a run ends where a column has no value, so a gap is drawn as a gap. `area` closes each
 * run down to the baseline for a filled look.
 */
export function paths(cols: Bucketed[], pick: (b: Bucketed) => number | null, box: Box): { line: string; area: string } {
  const x = (t: number) => ((t - box.from) / (box.to - box.from)) * box.width;
  const y = (v: number) => box.height - (Math.min(v, box.max) / box.max) * box.height;
  const runs: [number, number][][] = [];
  let cur: [number, number][] = [];
  for (const c of cols) {
    const v = pick(c);
    if (v === null) {
      if (cur.length) runs.push(cur);
      cur = [];
    } else cur.push([x(c.t), y(v)]);
  }
  if (cur.length) runs.push(cur);
  const f = (n: number) => n.toFixed(1);
  const line = runs.map((r) => (r.length === 1 ? `M${f(r[0][0] - 0.5)},${f(r[0][1])} L${f(r[0][0] + 0.5)},${f(r[0][1])}` : r.map(([px, py], i) => `${i ? "L" : "M"}${f(px)},${f(py)}`).join(" "))).join(" ");
  const area = runs
    .filter((r) => r.length > 1)
    .map((r) => `${r.map(([px, py], i) => `${i ? "L" : "M"}${f(px)},${f(py)}`).join(" ")} L${f(r[r.length - 1][0])},${f(box.height)} L${f(r[0][0])},${f(box.height)} Z`)
    .join(" ");
  return { line, area };
}

/** The column nearest a time, for the hover readout; null when nothing is within a column of it. */
export function nearest(cols: Bucketed[], t: number): Bucketed | null {
  let best: Bucketed | null = null;
  let d = Infinity;
  for (const c of cols) {
    if (c.rx === null && c.tx === null) continue;
    const dist = Math.abs(c.t - t);
    if (dist < d) {
      d = dist;
      best = c;
    }
  }
  return best;
}

/** The highest reading in a set of points, for scaling and for the "peak" figure. */
export function peak(points: NetPoint[], from: number, to: number): { rx: number | null; tx: number | null } {
  let rx: number | null = null;
  let tx: number | null = null;
  for (const p of points) {
    if (p.t < from || p.t > to) continue;
    if (p.rx !== null) rx = rx === null ? p.rx : Math.max(rx, p.rx);
    if (p.tx !== null) tx = tx === null ? p.tx : Math.max(tx, p.tx);
  }
  return { rx, tx };
}

/** Compact sparkline points for a small inline chart (the latest `n` readings, scaled to their own peak). */
export function sparkline(points: NetPoint[], pick: (p: NetPoint) => number | null, w: number, h: number, n = 30): string {
  const tail = points.slice(-n);
  const vals = tail.map(pick);
  const max = Math.max(1, ...vals.filter((v): v is number => v !== null));
  const step = tail.length > 1 ? w / (tail.length - 1) : w;
  const pts = vals.map((v, i) => (v === null ? null : `${(i * step).toFixed(1)},${(h - (v / max) * (h - 2) - 1).toFixed(1)}`)).filter(Boolean);
  return pts.length > 1 ? pts.join(" ") : "";
}

/** "1 s ago", "12 s ago", "3 min ago". */
export function updatedAgo(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 60 ? `${s} s ago` : `${Math.floor(s / 60)} min ago`;
}
