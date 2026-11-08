// What the Host Metrics window remembers between openings, on this computer only (never in the vault, never synced):
// how fast to read, how far back to chart, bytes or bits, and each host's chosen network interface. Presentation only:
// nothing here changes how a host is read beyond the live rate the person picked, or what is stored about it.
import type { RateUnit } from "./hostsample";

export const KEY = "sshvault.hostmetrics.view.v1";

export const LIVE_MS = [0, 1000, 2000, 5000] as const;
/** The ranges the in-memory frames can cover. Longer ones need saved history, which does not exist yet. */
export const RANGES = [
  { id: "5m", label: "5 min", ms: 5 * 60_000 },
  { id: "15m", label: "15 min", ms: 15 * 60_000 },
  { id: "1h", label: "1 hour", ms: 60 * 60_000 },
] as const;
export type RangeId = (typeof RANGES)[number]["id"];

export interface ViewPrefs {
  live: (typeof LIVE_MS)[number];
  range: RangeId;
  unit: RateUnit;
  /** The interface chosen per host id. */
  ifaces: Record<string, string>;
}

export const DEFAULTS: ViewPrefs = { live: 2000, range: "15m", unit: "bytes", ifaces: {} };

/** Whatever was stored, repaired field by field: a bad or old value becomes the default, never an error. */
export function readPrefs(raw: string | null | undefined): ViewPrefs {
  let o: Record<string, unknown> = {};
  try {
    const v: unknown = raw ? JSON.parse(raw) : {};
    if (v && typeof v === "object" && !Array.isArray(v)) o = v as Record<string, unknown>;
  } catch {
    // Unreadable: start again from the defaults.
  }
  const ifaces: Record<string, string> = {};
  if (o.ifaces && typeof o.ifaces === "object" && !Array.isArray(o.ifaces)) {
    for (const [k, v] of Object.entries(o.ifaces as Record<string, unknown>)) if (typeof v === "string" && v.length <= 64 && Object.keys(ifaces).length < 500) ifaces[k] = v;
  }
  return {
    live: (LIVE_MS as readonly unknown[]).includes(o.live) ? (o.live as ViewPrefs["live"]) : DEFAULTS.live,
    range: RANGES.some((r) => r.id === o.range) ? (o.range as RangeId) : DEFAULTS.range,
    unit: o.unit === "bits" ? "bits" : "bytes",
    ifaces,
  };
}

export function loadPrefs(): ViewPrefs {
  try {
    return readPrefs(localStorage.getItem(KEY));
  } catch {
    return { ...DEFAULTS, ifaces: {} };
  }
}

export function savePrefs(p: ViewPrefs) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    // Not fatal: the choices just won't be there next time.
  }
}

export const rangeMs = (id: RangeId): number => RANGES.find((r) => r.id === id)!.ms;
