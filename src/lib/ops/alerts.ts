// When to tell the person something is wrong with a host: it stopped answering, or CPU, memory or disk
// stayed high. Pure: it is given readings and says what, if anything, to announce. The polling, the
// notifications and the settings are around it (stores/alerts.svelte.ts).

export interface Quiet {
  on: boolean;
  /** "HH:MM", 24-hour. A window that ends before it starts runs overnight. */
  from: string;
  to: string;
}

export interface AlertRules {
  down: boolean;
  /** Percent thresholds; null turns that one off. */
  cpu: number | null;
  mem: number | null;
  disk: number | null;
  /** Tell me when a systemd service on the host enters the failed state. Off until switched on. */
  services: boolean;
  /** How many readings in a row must be over a threshold before it counts. */
  samples: number;
  quiet: Quiet;
}

export const DEFAULT_RULES: AlertRules = { down: true, cpu: 90, mem: 90, disk: 90, services: false, samples: 3, quiet: { on: false, from: "22:00", to: "07:00" } };

export type AlertKind = "down" | "up" | "cpu" | "mem" | "disk" | "service" | "cleared";

/** How much it matters: a recovery is information; a host that stopped answering, or a reading near the top, is critical. */
export type Severity = "info" | "warning" | "critical";
export const SEVERITIES: Severity[] = ["info", "warning", "critical"];
/** A usage reading at or above this is critical, not just over its limit. */
export const CRITICAL_AT = 97;

export interface Alert {
  id: number;
  at: number;
  hostId: string;
  host: string;
  kind: AlertKind;
  severity: Severity;
  message: string;
  /** Raised during quiet hours: kept in the list, but not announced. */
  quiet: boolean;
  /** When the person acknowledged it, or null. Recoveries need no acknowledging. */
  ack: number | null;
}

export function severityOf(kind: AlertKind, value?: number): Severity {
  if (kind === "up" || kind === "cleared") return "info";
  if (kind === "down") return "critical";
  return value !== undefined && value >= CRITICAL_AT ? "critical" : "warning";
}

/** Whether `s` is at least as serious as `floor`. */
export const atLeast = (s: Severity, floor: Severity) => SEVERITIES.indexOf(s) >= SEVERITIES.indexOf(floor);

/** The list with one alert (or, for "all", every open one) acknowledged at `at`. Leaves the others as they were. */
export function acknowledge(list: Alert[], which: number | "all", at: number): Alert[] {
  return list.map((a) => ((which === "all" || a.id === which) && a.ack === null ? { ...a, ack: at } : a));
}

/** Alerts that need a person's attention: not a recovery, not yet acknowledged. */
export const open = (list: Alert[]) => list.filter((a) => a.severity !== "info" && a.ack === null);

/** Alerts read back from storage: anything that isn't shaped like an alert is dropped, and old ones get the new fields. */
export function revive(raw: unknown, keep = 100): Alert[] {
  if (!Array.isArray(raw)) return [];
  const kinds: AlertKind[] = ["down", "up", "cpu", "mem", "disk", "service", "cleared"];
  const out: Alert[] = [];
  for (const r of raw) {
    if (!r || typeof r !== "object") continue;
    const a = r as Record<string, unknown>;
    if (typeof a.id !== "number" || typeof a.at !== "number" || typeof a.hostId !== "string" || typeof a.host !== "string" || typeof a.message !== "string" || !kinds.includes(a.kind as AlertKind)) continue;
    const kind = a.kind as AlertKind;
    out.push({
      id: a.id,
      at: a.at,
      hostId: a.hostId,
      host: a.host,
      kind,
      severity: SEVERITIES.includes(a.severity as Severity) ? (a.severity as Severity) : severityOf(kind),
      message: a.message.slice(0, 500),
      quiet: a.quiet === true,
      ack: typeof a.ack === "number" ? a.ack : null,
    });
  }
  return out.slice(0, keep);
}

const minutes = (hhmm: string): number => {
  const m = /^(\d{1,2}):(\d{2})$/.exec(hhmm);
  return m ? Math.min(23, Number(m[1])) * 60 + Math.min(59, Number(m[2])) : 0;
};

/** Whether `at` falls in the quiet window (local time). A window with equal ends is empty. */
export function inQuietHours(at: number, q: Quiet): boolean {
  if (!q.on) return false;
  const d = new Date(at);
  const now = d.getHours() * 60 + d.getMinutes();
  const from = minutes(q.from);
  const to = minutes(q.to);
  if (from === to) return false;
  return from < to ? now >= from && now < to : now >= from || now < to;
}

/** A check has to fail this many times in a row before a host counts as down: one dropped probe isn't an outage. */
export const DOWN_AFTER = 2;
/** A reading must fall this far below a threshold before the condition counts as over. Stops flapping at the line. */
export const CLEAR_MARGIN = 5;

interface HostState {
  failures: number;
  down: boolean;
  over: Record<"cpu" | "mem" | "disk", number>;
  active: Record<"cpu" | "mem" | "disk", boolean>;
  /** Units already reported as failed, so each is announced once. */
  failed: Set<string>;
}

const LABEL = { cpu: "CPU", mem: "Memory", disk: "Disk" } as const;

export class AlertEngine {
  #state = new Map<string, HostState>();
  #next = 1;

  constructor(
    private readonly rules: () => AlertRules,
    private readonly muted: (hostId: string) => boolean = () => false,
  ) {}

  #of(id: string): HostState {
    let s = this.#state.get(id);
    if (!s) this.#state.set(id, (s = { failures: 0, down: false, over: { cpu: 0, mem: 0, disk: 0 }, active: { cpu: false, mem: false, disk: false }, failed: new Set() }));
    return s;
  }

  #alert(hostId: string, host: string, kind: AlertKind, message: string, at: number, value?: number): Alert {
    return { id: this.#next++, at, hostId, host, kind, severity: severityOf(kind, value), message, quiet: inQuietHours(at, this.rules().quiet), ack: null };
  }

  /** Continue numbering after alerts that were kept from an earlier run, so ids stay unique. */
  resumeAfter(id: number) {
    this.#next = Math.max(this.#next, id + 1);
  }

  forget(hostId: string) {
    this.#state.delete(hostId);
  }

  /** A reachability check came back. */
  health(hostId: string, host: string, up: boolean, at: number): Alert[] {
    const r = this.rules();
    const s = this.#of(hostId);
    if (!r.down || this.muted(hostId)) {
      s.failures = 0;
      s.down = false;
      return [];
    }
    if (up) {
      s.failures = 0;
      if (!s.down) return [];
      s.down = false;
      return [this.#alert(hostId, host, "up", `${host} is reachable again.`, at)];
    }
    s.failures++;
    if (s.failures >= DOWN_AFTER && !s.down) {
      s.down = true;
      return [this.#alert(hostId, host, "down", `${host} is not answering.`, at)];
    }
    return [];
  }

  /** A CPU, memory and disk reading came back. A value that is null (not available) changes nothing. */
  metrics(hostId: string, host: string, m: { cpuPct: number | null; memPct: number | null; diskPct: number | null; failedUnits?: string[] | null }, at: number): Alert[] {
    const r = this.rules();
    if (this.muted(hostId)) return [];
    const s = this.#of(hostId);
    const out: Alert[] = [];
    const readings = { cpu: m.cpuPct, mem: m.memPct, disk: m.diskPct } as const;
    for (const key of ["cpu", "mem", "disk"] as const) {
      const limit = r[key];
      const value = readings[key];
      if (limit === null || value === null) continue;
      if (value >= limit) {
        s.over[key]++;
        if (s.over[key] >= Math.max(1, r.samples) && !s.active[key]) {
          s.active[key] = true;
          out.push(this.#alert(hostId, host, key, `${host}: ${LABEL[key]} is at ${Math.round(value)}% (limit ${limit}%).`, at, value));
        }
      } else if (value < limit - CLEAR_MARGIN) {
        s.over[key] = 0;
        if (s.active[key]) {
          s.active[key] = false;
          out.push(this.#alert(hostId, host, "cleared", `${host}: ${LABEL[key]} is back to ${Math.round(value)}%.`, at));
        }
      }
      // Between the limit and the margin: neither over nor clear, so the state holds.
    }
    // Failed services: each unit is announced when it first appears, and one note when the list empties.
    if (!r.services) s.failed = new Set();
    else if (m.failedUnits) {
      const now = new Set(m.failedUnits);
      const fresh = [...now].filter((u) => !s.failed.has(u));
      if (fresh.length) {
        const shown = fresh.slice(0, 5).join(", ") + (fresh.length > 5 ? ` and ${fresh.length - 5} more` : "");
        out.push(this.#alert(hostId, host, "service", `${host}: ${fresh.length === 1 ? "service" : "services"} failed: ${shown}.`, at));
      } else if (s.failed.size && now.size === 0) {
        out.push(this.#alert(hostId, host, "cleared", `${host}: no failed services any more.`, at));
      }
      s.failed = now;
    }
    return out;
  }
}
