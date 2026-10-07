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
  /** How many readings in a row must be over a threshold before it counts. */
  samples: number;
  quiet: Quiet;
}

export const DEFAULT_RULES: AlertRules = { down: true, cpu: 90, mem: 90, disk: 90, samples: 3, quiet: { on: false, from: "22:00", to: "07:00" } };

export type AlertKind = "down" | "up" | "cpu" | "mem" | "disk" | "cleared";

export interface Alert {
  id: number;
  at: number;
  hostId: string;
  host: string;
  kind: AlertKind;
  message: string;
  /** Raised during quiet hours: kept in the list, but not announced. */
  quiet: boolean;
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
    if (!s) this.#state.set(id, (s = { failures: 0, down: false, over: { cpu: 0, mem: 0, disk: 0 }, active: { cpu: false, mem: false, disk: false } }));
    return s;
  }

  #alert(hostId: string, host: string, kind: AlertKind, message: string, at: number): Alert {
    return { id: this.#next++, at, hostId, host, kind, message, quiet: inQuietHours(at, this.rules().quiet) };
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
  metrics(hostId: string, host: string, m: { cpuPct: number | null; memPct: number | null; diskPct: number | null }, at: number): Alert[] {
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
          out.push(this.#alert(hostId, host, key, `${host}: ${LABEL[key]} is at ${Math.round(value)}% (limit ${limit}%).`, at));
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
    return out;
  }
}
