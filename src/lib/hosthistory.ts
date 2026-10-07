// The last 15 minutes of a host's readings, kept in memory only, and the
// geometry to draw them as lines. Nothing here is written to disk or synced.
import type { HostDetail } from "./hostdetail";
import { totalRate } from "./hostdetail";
import type { HostMetrics } from "./hostmetrics";

/** How far back the charts reach. */
export const HISTORY_MS = 15 * 60_000;
/** A hard cap, so a fast refresh can't grow without limit. */
export const HISTORY_MAX = 1000;
/** Samples closer together than this are one reading. */
const MIN_GAP_MS = 500;
/** A hole longer than this in the data is shown as a hole, not joined by a line. */
export const GAP_MS = 75_000;

export interface Sample {
  /** Milliseconds since the epoch, on this computer's clock. */
  t: number;
  cpu: number | null;
  mem: number | null;
  /** Bytes per second received and sent, over everything that isn't loopback. */
  rx: number | null;
  tx: number | null;
}

/** The slim, 30-second summary reading: CPU and memory only. */
export const fromSummary = (m: HostMetrics, t: number): Sample => ({ t, cpu: m.cpuPct, mem: m.memPct, rx: null, tx: null });

/** A detail reading: CPU, memory and total network throughput. */
export function fromDetail(d: HostDetail, t: number): Sample {
  const net = d.rates.length ? totalRate(d.rates) : null;
  return { t, cpu: d.cpuPct, mem: d.memPct, rx: net?.rxBps ?? null, tx: net?.txBps ?? null };
}

/**
 * The list with `s` added: oldest samples past the window dropped, a sample out
 * of order or a duplicate of the last moment merged instead of added. Returns a
 * new array, so a reactive reader sees the change.
 */
export function addSample(list: Sample[], s: Sample, windowMs = HISTORY_MS, max = HISTORY_MAX): Sample[] {
  const last = list[list.length - 1];
  let next: Sample[];
  if (last && s.t < last.t) {
    // The clock stepped back: what came before it can't be ordered against this, so start over.
    next = [s];
  } else if (last && s.t - last.t < MIN_GAP_MS) {
    // Two readings in the same instant: keep the newer one, filling a field the newer one lacks.
    next = [...list.slice(0, -1), { t: s.t, cpu: s.cpu ?? last.cpu, mem: s.mem ?? last.mem, rx: s.rx ?? last.rx, tx: s.tx ?? last.tx }];
  } else {
    next = [...list, s];
  }
  const from = s.t - windowMs;
  let start = 0;
  while (start < next.length - 1 && next[start].t < from) start++;
  const kept = start > 0 ? next.slice(start) : next;
  return kept.length > max ? kept.slice(kept.length - max) : kept;
}

export interface Point {
  x: number;
  y: number;
  /** Index of the sample it came from. */
  i: number;
}

export interface Box {
  width: number;
  height: number;
  /** The time range drawn, left to right. */
  from: number;
  to: number;
  /** The value at the top; 0 is the bottom. */
  max: number;
}

/**
 * Pixel points of one series, as separate runs. A run ends at a missing value
 * and at a gap in time, so the line shows the hole.
 */
export function chartSegments(samples: Sample[], pick: (s: Sample) => number | null, box: Box, gapMs = GAP_MS): Point[][] {
  const span = box.to - box.from;
  if (span <= 0 || box.max <= 0) return [];
  const out: Point[][] = [];
  let cur: Point[] = [];
  let prevT: number | null = null;
  samples.forEach((s, i) => {
    const v = pick(s);
    if (v === null || s.t < box.from || s.t > box.to) {
      if (cur.length) out.push(cur);
      cur = [];
      prevT = null;
      return;
    }
    if (prevT !== null && s.t - prevT > gapMs && cur.length) {
      out.push(cur);
      cur = [];
    }
    const x = ((s.t - box.from) / span) * box.width;
    const y = box.height - Math.max(0, Math.min(1, v / box.max)) * box.height;
    cur.push({ x, y, i });
    prevT = s.t;
  });
  if (cur.length) out.push(cur);
  return out;
}

/** A path for a run of points. A single point is a zero-length line, so a lone reading still shows (with round caps). */
export function pathOf(points: Point[]): string {
  if (points.length === 0) return "";
  const p = points.map((q) => `${q.x.toFixed(1)} ${q.y.toFixed(1)}`);
  return points.length === 1 ? `M${p[0]} L${p[0]}` : `M${p.join(" L")}`;
}

/** The same run closed down to the baseline, for the faint area under a single series. */
export function areaOf(points: Point[], height: number): string {
  if (points.length < 2) return "";
  const first = points[0];
  const last = points[points.length - 1];
  return `${pathOf(points)} L${last.x.toFixed(1)} ${height} L${first.x.toFixed(1)} ${height} Z`;
}

/** The smallest "round" number (1, 2 or 5 times a power of ten) at or above `v`, never below `floor`. */
export function niceMax(v: number, floor = 1): number {
  const target = Math.max(v, floor);
  if (!(target > 0) || !Number.isFinite(target)) return floor;
  const pow = 10 ** Math.floor(Math.log10(target));
  for (const m of [1, 2, 5, 10]) if (m * pow >= target) return m * pow;
  return 10 * pow;
}

/** How many equal steps divide a maximum from [`niceMax`] into round numbers: 1 into fifths, 2 into quarters, 5 into fifths. */
export function stepsFor(max: number): number {
  const lead = Math.round(max / 10 ** Math.floor(Math.log10(max)));
  return lead === 2 ? 4 : 5;
}

/** Evenly spaced tick values from 0 to `max`, in `count` steps (round numbers when `max` came from [`niceMax`] and `count` from [`stepsFor`]). */
export function ticks(max: number, count = 4): number[] {
  return Array.from({ length: count + 1 }, (_, i) => (max * i) / count);
}

/** Index of the sample nearest in time to `t`, or -1 when there are none. */
export function nearestIndex(samples: Sample[], t: number): number {
  if (samples.length === 0) return -1;
  let lo = 0;
  let hi = samples.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (samples[mid].t < t) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0 && Math.abs(samples[lo - 1].t - t) <= Math.abs(samples[lo].t - t)) return lo - 1;
  return lo;
}

/** "−15 min" … "now" labels for the time axis. */
export function timeLabel(minutesAgo: number): string {
  return minutesAgo === 0 ? "now" : `−${minutesAgo} min`;
}

/** An axis label for `ms` ago, in the unit that fits: minutes, hours or days. */
export function agoLabel(ms: number): string {
  if (ms <= 0) return "now";
  const min = Math.round(ms / 60_000);
  if (min < 90) return `−${min} min`;
  const h = Math.round(ms / 3_600_000);
  if (h < 48) return `−${h} h`;
  return `−${Math.round(ms / 86_400_000)} d`;
}

/** The clock time of a sample, for a tooltip or a table row. */
export function clock(t: number): string {
  return new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}
