// The Monitoring page's rules, kept apart from the screen so they can be tested: how a host's checks add up to
// healthy, warning, critical or offline, what the summary counts, how the host table is searched and filtered, and
// where the details panel's tabs get their numbers.
import { worst, type MonitorTarget, type Status, type TargetStatus } from "./monitoring";
import { headlineInterface, type NetInterface } from "./netdata";
import type { Uuid, VaultRecord } from "./types";

export type Health = "healthy" | "warning" | "critical" | "offline" | "paused" | "waiting";

export const HEALTH_TEXT: Record<Health, string> = { healthy: "Healthy", warning: "Warning", critical: "Critical", offline: "Offline", paused: "Paused", waiting: "Waiting" };

/**
 * What a host's latest checks say about it. **Offline** means it could not be reached at all: every ping was lost, or
 * its ping could not run, or (with no ping check) every port refused. Otherwise the worst rule state decides; a host
 * that answers but has no rule judging it counts as healthy only if at least one check actually read something.
 */
export function healthOf(target: MonitorTarget, st: TargetStatus | undefined): Health {
  if (!target.enabled) return "paused";
  if (!st) return "waiting";
  if (st.ping && (st.ping.loss_pct >= 100 || st.ping.error)) return "offline";
  if (!st.ping && st.ports.length > 0 && st.ports.every((p) => !p.open)) return "offline";
  const states = st.states.map((s) => s.status);
  const w: Status | null = states.length ? worst(st.states) : null;
  if (w === "critical") return "critical";
  if (w === "warning") return "warning";
  const read = st.ping || st.ports.length || st.web.length || st.metrics;
  if (!read) return "waiting";
  // A web check that is failing is a problem whether or not a rule was written for it.
  if (st.web.some((x) => !x.ok)) return "critical";
  return "healthy";
}

export interface Summary {
  total: number;
  healthy: number;
  warning: number;
  critical: number;
  offline: number;
}

/** Counts over every host that is being watched. Paused and not-yet-checked hosts are in the total only. */
export function summarize(rows: { health: Health }[]): Summary {
  const s: Summary = { total: rows.length, healthy: 0, warning: 0, critical: 0, offline: 0 };
  for (const r of rows) if (r.health === "healthy" || r.health === "warning" || r.health === "critical" || r.health === "offline") s[r.health]++;
  return s;
}

export interface HostRow {
  id: Uuid;
  rec: VaultRecord<MonitorTarget>;
  target: MonitorTarget;
  status: TargetStatus | undefined;
  health: Health;
  /** The main interface's current speed, when the host's counters were read. */
  net: { name: string; rx: number | null; tx: number | null } | null;
}

export function buildRow(rec: VaultRecord<MonitorTarget>, status: TargetStatus | undefined): HostRow {
  const target = rec.data!;
  const m = status?.metrics;
  const iface: NetInterface | null = m?.net ? headlineInterface(m.net, m.net_headline) : null;
  return { id: rec.id, rec, target, status, health: healthOf(target, status), net: iface ? { name: iface.name, rx: iface.rx_bps, tx: iface.tx_bps } : null };
}

export type HealthFilter = "all" | "healthy" | "warning" | "critical" | "offline";

/** Every word typed must appear in the host's name, address, tags, notes or health. */
export function matchesRow(r: HostRow, query: string, health: HealthFilter, tag: string): boolean {
  if (health !== "all" && r.health !== health) return false;
  if (tag && !r.target.tags.includes(tag)) return false;
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = [r.target.label, r.target.address, ...r.target.tags, r.target.notes, HEALTH_TEXT[r.health]].join(" ").toLowerCase();
  return words.every((w) => hay.includes(w));
}

export type SortKey = "name" | "health" | "cpu" | "mem" | "rx" | "tx";

const ORDER: Record<Health, number> = { critical: 0, offline: 1, warning: 2, waiting: 3, healthy: 4, paused: 5 };

/** Rows in the order asked for; a missing value sorts last whichever way. */
export function sortRows(rows: HostRow[], key: SortKey, dir: "asc" | "desc"): HostRow[] {
  const sign = dir === "asc" ? 1 : -1;
  const num = (r: HostRow): number | null => (key === "cpu" ? (r.status?.metrics?.cpu ?? null) : key === "mem" ? (r.status?.metrics?.mem ?? null) : key === "rx" ? (r.net?.rx ?? null) : key === "tx" ? (r.net?.tx ?? null) : null);
  return [...rows].sort((a, b) => {
    if (key === "name") return sign * a.target.label.localeCompare(b.target.label);
    if (key === "health") return sign * (ORDER[a.health] - ORDER[b.health]) || a.target.label.localeCompare(b.target.label);
    const x = num(a);
    const y = num(b);
    if (x === null && y === null) return a.target.label.localeCompare(b.target.label);
    if (x === null) return 1;
    if (y === null) return -1;
    return sign * (x - y) || a.target.label.localeCompare(b.target.label);
  });
}

/** What the details panel's Checks tab lists for one host. */
export interface CheckLine {
  kind: "ping" | "port" | "web" | "ssh";
  name: string;
  ok: boolean | null;
  detail: string;
}

export function checkLines(st: TargetStatus | undefined): CheckLine[] {
  if (!st) return [];
  const out: CheckLine[] = [];
  if (st.ping) {
    const lost = st.ping.loss_pct;
    out.push({ kind: "ping", name: "Ping", ok: !st.ping.error && lost < 100, detail: st.ping.error ?? `${lost}% lost${st.ping.avg_ms !== null ? `, ${st.ping.avg_ms.toFixed(1)} ms` : ""}` });
  }
  for (const p of st.ports) out.push({ kind: "port", name: `Port ${p.port}`, ok: p.open, detail: p.open ? `open${p.latency_ms !== null ? `, ${p.latency_ms} ms` : ""}` : (p.error ?? "closed") });
  for (const w of st.web) out.push({ kind: "web", name: w.url, ok: w.ok, detail: w.ok ? `${w.status ?? ""}${w.time_ms !== null ? ` · ${w.time_ms} ms` : ""}${w.cert_days !== null ? ` · cert ${w.cert_days} d` : ""}`.trim() : (w.error ?? `status ${w.status ?? "none"}`) });
  for (const e of st.errors.filter((x) => x.startsWith("SSH:"))) out.push({ kind: "ssh", name: "SSH readings", ok: false, detail: e.replace(/^SSH:\s*/, "") });
  if (st.metrics && !st.errors.some((x) => x.startsWith("SSH:"))) out.push({ kind: "ssh", name: "SSH readings", ok: true, detail: "CPU, memory, disk and network read" });
  return out;
}
