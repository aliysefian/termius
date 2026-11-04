// The log viewer's data: what the backend sends (src-tauri/src/opslogs.rs), the rows the viewer keeps, how serious a
// row is, and a bounded, time-ordered buffer of them. Pure, so none of it needs a connection to be tested.
import { detectLevel } from "./logs";

// -- what the backend sends ------------------------------------------------------------------------------

export type HostState = "connecting" | "streaming" | "reconnecting" | "stopped" | "failed";

export interface WireEntry {
  host_id: string;
  seq: number;
  /** Microseconds since 1970. */
  ts_us: number;
  priority?: number;
  unit?: string;
  ident?: string;
  pid?: number;
  message: string;
  cursor?: string;
  journal_host?: string;
  fields?: [string, string][];
  truncated?: boolean;
}

export type OpsEvent =
  | { event: "state"; host_id: string; state: HostState; attempt: number; retry_in_secs: number | null; message: string | null; notice: string | null }
  | { event: "batch"; host_id: string; entries: WireEntry[]; dropped: number }
  | { event: "gap"; host_id: string; reason: string };

// -- severity ----------------------------------------------------------------------------------------------------

/** The four buckets the viewer filters by, worst first. */
export type Sev = "error" | "warn" | "info" | "debug";
export const SEVS: Sev[] = ["error", "warn", "info", "debug"];
export const SEV_LABEL: Record<Sev, string> = { error: "Error", warn: "Warning", info: "Info", debug: "Debug" };

/** The journal's priority names, 0 to 7. */
export const PRIORITY_NAMES = ["Emergency", "Alert", "Critical", "Error", "Warning", "Notice", "Info", "Debug"] as const;
/** Short badge text for each priority. */
export const PRIORITY_BADGE = ["EMERG", "ALERT", "CRIT", "ERROR", "WARN", "NOTICE", "INFO", "DEBUG"] as const;

/** Which bucket a journal priority falls in: 0-3 error, 4 warning, 5-6 info, 7 debug. */
export function sevOfPriority(p: number): Sev {
  return p <= 3 ? "error" : p === 4 ? "warn" : p <= 6 ? "info" : "debug";
}

// -- rows -------------------------------------------------------------------------------------------------------------

export interface Row {
  /** `host:seq`, unique within a stream. */
  id: string;
  hostId: string;
  /** The SSHVault host's name, as it was when the row arrived. */
  host: string;
  seq: number;
  /** Milliseconds, for the time column and ranges; `us` has the microseconds for ordering. */
  ts: number;
  us: number;
  /** 0-7 for a journal entry; null for a line of a log file. */
  priority: number | null;
  sev: Sev;
  /** True when the severity was guessed from the words of a line (a log file has no priority). */
  guessed: boolean;
  unit: string;
  ident: string;
  pid: number | null;
  message: string;
  cursor: string | null;
  journalHost: string | null;
  fields: [string, string][];
  truncated: boolean;
  /** Everything searchable joined, made on first use. */
  hay?: string;
  hayLower?: string;
}

export function toRow(e: WireEntry, host: string): Row {
  const priority = typeof e.priority === "number" ? e.priority : null;
  return {
    id: `${e.host_id}:${e.seq}`,
    hostId: e.host_id,
    host,
    seq: e.seq,
    ts: Math.floor(e.ts_us / 1000),
    us: e.ts_us,
    priority,
    sev: priority === null ? detectLevel(e.message) : sevOfPriority(priority),
    guessed: priority === null,
    unit: e.unit ?? "",
    ident: e.ident ?? "",
    pid: typeof e.pid === "number" ? e.pid : null,
    message: e.message,
    cursor: e.cursor ?? null,
    journalHost: e.journal_host ?? null,
    fields: e.fields ?? [],
    truncated: !!e.truncated,
  };
}

/** The text a search looks in: message, host, unit, program, process id, and the other journal fields' values. */
export function hayOf(r: Row): string {
  return (r.hay ??= [r.message, r.host, r.unit, r.ident, r.pid ?? "", ...r.fields.map(([, v]) => v)].join("\n"));
}

export function hayLowerOf(r: Row): string {
  return (r.hayLower ??= hayOf(r).toLowerCase());
}

/** The order rows are shown in: by time, then host, then the order they came from the host. */
export function byTime(a: Row, b: Row): number {
  return a.us - b.us || (a.hostId < b.hostId ? -1 : a.hostId > b.hostId ? 1 : 0) || a.seq - b.seq;
}

// -- the buffer ----------------------------------------------------------------------------------------------------------

/**
 * The newest `max` rows kept, in order. `rows` is replaced (never changed in place) whenever it changes, so a view
 * may hold on to an old one (a paused view does) and a derived list can tell that it changed.
 */
export class LogBuffer {
  rows: Row[] = [];
  /** Rows pushed out of the buffer because it was full. */
  evicted = 0;
  #seen = new Set<string>();

  constructor(
    public max = 20_000,
    /** "time" keeps rows in timestamp order; "arrival" in the order they came. */
    public order: "time" | "arrival" = "time",
  ) {}

  /** Adds rows (those already held are skipped). Returns how many were new. */
  add(batch: Row[]): number {
    const fresh = batch.filter((r) => !this.#seen.has(r.id));
    if (!fresh.length) return 0;
    for (const r of fresh) this.#seen.add(r.id);
    let next: Row[];
    const last = this.rows[this.rows.length - 1];
    if (this.order === "arrival") {
      next = this.rows.concat(fresh);
    } else {
      fresh.sort(byTime);
      next = !last || byTime(last, fresh[0]) <= 0 ? this.rows.concat(fresh) : merge(this.rows, fresh);
    }
    if (next.length > this.max) {
      const cut = next.length - this.max;
      for (let i = 0; i < cut; i++) this.#seen.delete(next[i].id);
      this.evicted += cut;
      next = next.slice(cut);
    }
    this.rows = next;
    return fresh.length;
  }

  /** Forget everything held (the viewer's "clear", not anything on a host). */
  clear() {
    this.rows = [];
    this.evicted = 0;
    this.#seen.clear();
  }

  /** Change how many rows are kept; the oldest go at once if it is now too many. */
  resize(max: number) {
    this.max = Math.max(100, Math.floor(max));
    if (this.rows.length > this.max) {
      const cut = this.rows.length - this.max;
      for (let i = 0; i < cut; i++) this.#seen.delete(this.rows[i].id);
      this.evicted += cut;
      this.rows = this.rows.slice(cut);
    }
  }
}

function merge(a: Row[], b: Row[]): Row[] {
  const out: Row[] = new Array(a.length + b.length);
  let i = 0;
  let j = 0;
  let k = 0;
  while (i < a.length && j < b.length) out[k++] = byTime(a[i], b[j]) <= 0 ? a[i++] : b[j++];
  while (i < a.length) out[k++] = a[i++];
  while (j < b.length) out[k++] = b[j++];
  return out;
}

// -- the event rate ----------------------------------------------------------------------------------------------------

/** Events received per second, in a ring of one-second buckets, so a rate is read from what really arrived. */
export class Rate {
  #counts = new Array<number>(60).fill(0);
  #stamp = new Array<number>(60).fill(-1);
  #first = 0;
  last = 0;

  constructor(private readonly windowSecs = 60) {}

  add(n: number, now = Date.now()) {
    if (n <= 0) return;
    const sec = Math.floor(now / 1000);
    const i = sec % 60;
    if (this.#stamp[i] !== sec) {
      this.#stamp[i] = sec;
      this.#counts[i] = 0;
    }
    this.#counts[i] += n;
    if (!this.#first) this.#first = now;
    this.last = now;
  }

  /** Events in the last `windowSecs` seconds, and the seconds that window really covers (shorter right after the start). */
  window(now = Date.now()): { count: number; secs: number } {
    const sec = Math.floor(now / 1000);
    let count = 0;
    for (let i = 0; i < 60; i++) if (this.#stamp[i] >= 0 && sec - this.#stamp[i] < this.windowSecs && this.#stamp[i] <= sec) count += this.#counts[i];
    const secs = this.#first ? Math.min(this.windowSecs, Math.max(1, (now - this.#first) / 1000)) : 0;
    return { count, secs };
  }

  /** Events a minute at the rate seen in the window, or null when there is too little time behind it to say. */
  perMinute(now = Date.now()): number | null {
    const { count, secs } = this.window(now);
    // Too little time behind it to say events a minute: a burst at the start would read as a rate.
    if (secs < 10) return null;
    return Math.round((count * 60) / secs);
  }

  reset() {
    this.#counts.fill(0);
    this.#stamp.fill(-1);
    this.#first = 0;
    this.last = 0;
  }
}
