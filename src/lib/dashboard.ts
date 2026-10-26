// What the dashboard says, worked out from the numbers the stores already hold. Pure, so the wording and the
// colour thresholds are tested rather than eyeballed.
import type { ConnectionLogEntry } from "./stores/connectionlog.svelte";
import type { ForwardStatus, Health, Uuid } from "./types";

// -- the build -------------------------------------------------------------------

/** "STABLE" for a plain release; the first word of a pre-release tag otherwise (0.37.0-beta.1 is "BETA"). */
export function releaseChannel(version: string): string {
  const pre = version.split("+")[0].split("-").slice(1).join("-");
  return pre ? pre.split(".")[0].toUpperCase() : "STABLE";
}

// -- the vault -------------------------------------------------------------------

export interface VaultHealth {
  label: string;
  tone: "success" | "warning" | "danger";
  /** What the label means, for a tooltip. */
  detail: string;
}

const plural = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** How the vault (this app's database) is doing, from what the sync check already counts. A folder gone backwards outranks conflicts. */
export function vaultHealth(v: { unlocked: boolean; conflicts: number; rollbacks: number }): VaultHealth {
  if (!v.unlocked) return { label: "Locked", tone: "warning", detail: "Unlock the vault to read it." };
  if (v.rollbacks > 0) {
    return { label: "Rolled back", tone: "danger", detail: `${plural(v.rollbacks, "record is", "records are")} older than, or missing since, what this computer last saw. Open the Vault screen.` };
  }
  if (v.conflicts > 0) {
    return { label: "Conflicts", tone: "warning", detail: `${plural(v.conflicts, "item was", "items were")} changed on two devices at once and still need a decision. Open the Vault screen.` };
  }
  return { label: "Healthy", tone: "success", detail: "The vault opened, and its synced folder has no conflicts or rollbacks." };
}

// -- hosts -------------------------------------------------------------------------

export type HostStatus = "online" | "offline" | "unknown";

/** Only a probe that connected is "online" and only one that failed is "offline"; no probe yet, or a host behind a jump host, is unknown. */
export function hostStatus(h: Health | undefined): HostStatus {
  if (h?.state === "up") return "online";
  if (h?.state === "down") return "offline";
  return "unknown";
}

export function countOnline(statuses: HostStatus[]): number {
  return statuses.filter((s) => s === "online").length;
}

/** Natural order, so web-2 comes before web-10. */
export function byLabel<T extends { label: string }>(a: T, b: T): number {
  return a.label.localeCompare(b.label, undefined, { numeric: true, sensitivity: "base" });
}

// -- usage bars ----------------------------------------------------------------------

export type UsageLevel = "low" | "moderate" | "high";

export const MODERATE_FROM = 50;
export const HIGH_FROM = 80;

export function clampPct(pct: number): number {
  return Number.isFinite(pct) ? Math.min(100, Math.max(0, Math.round(pct))) : 0;
}

export function usageLevel(pct: number): UsageLevel {
  const p = clampPct(pct);
  return p >= HIGH_FROM ? "high" : p >= MODERATE_FROM ? "moderate" : "low";
}

/** The bar's fill: green while low, orange when moderate, yellow when high. */
export const LEVEL_FILL: Record<UsageLevel, string> = {
  low: "bg-success",
  moderate: "bg-usage-moderate",
  high: "bg-usage-high",
};

// -- tunnels -------------------------------------------------------------------------

export function activeTunnels(status: Record<Uuid, ForwardStatus>): number {
  return Object.values(status).filter((s) => s.state === "active").length;
}

// -- recent activity -----------------------------------------------------------------

export interface ActivityRow {
  id: string;
  host: string;
  /** What was opened: ">_ Terminal", "Files", "RDP". */
  what: string;
  /** Time since it started, compact: "12s", "5m", "2h", "3d". */
  ago: string;
  failed: boolean;
}

const WHAT: Record<ConnectionLogEntry["kind"], string> = {
  host: ">_ Terminal",
  adhoc: ">_ Terminal",
  local: ">_ Local",
  telnet: "Telnet",
  serial: "Serial",
  files: "Files",
  rdp: "RDP",
  vnc: "VNC",
};

export function elapsedShort(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

/** The newest `limit` entries of the connection log (which is newest first already), as lines for the activity feed. */
export function activityRows(entries: ConnectionLogEntry[], now: number, limit = 8): ActivityRow[] {
  return entries.slice(0, limit).map((e) => ({
    id: e.id,
    host: e.label,
    what: WHAT[e.kind] ?? e.kind,
    ago: elapsedShort(now - e.startedAt),
    failed: e.reason === "failed",
  }));
}
