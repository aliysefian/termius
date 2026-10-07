// Keeping the charts for longer than 15 minutes, when the person asks for it: readings folded into
// one-minute averages and kept for a day or a week, on this computer only. Pure functions and a small
// storage interface (IndexedDB in the app, a Map in tests).
import type { Sample } from "../hosthistory";

export type Keep = "off" | "day" | "week";

export const KEEP_MS: Record<Exclude<Keep, "off">, number> = { day: 24 * 3600_000, week: 7 * 24 * 3600_000 };
export const BUCKET_MS = 60_000;

/** A minute of readings: the average of each series, and how many readings of it went into the average. */
export interface Bucket {
  /** Start of the minute, ms since the epoch. */
  t: number;
  /** Readings of each series in this minute (a series can be missing from some readings). */
  n: { cpu: number; mem: number; rx: number; tx: number };
  cpu: number | null;
  mem: number | null;
  rx: number | null;
  tx: number | null;
}

const KEYS = ["cpu", "mem", "rx", "tx"] as const;

/** Running average that ignores a missing value. */
function mix(avg: number | null, count: number, v: number | null): number | null {
  if (v === null) return avg;
  if (avg === null || count === 0) return v;
  return (avg * count + v) / (count + 1);
}

/**
 * `buckets` with `s` folded in, and anything older than `keepMs` dropped. A reading that goes back in
 * time (the clock stepped) is ignored rather than rewriting history. Returns a new array.
 */
export function fold(buckets: Bucket[], s: Sample, keepMs: number): Bucket[] {
  const t = Math.floor(s.t / BUCKET_MS) * BUCKET_MS;
  const last = buckets[buckets.length - 1];
  let next: Bucket[];
  if (last && t < last.t) return buckets;
  if (last && last.t === t) {
    const merged: Bucket = { ...last, n: { ...last.n } };
    for (const k of KEYS) {
      merged[k] = mix(last[k], last.n[k], s[k]);
      if (s[k] !== null) merged.n[k]++;
    }
    next = [...buckets.slice(0, -1), merged];
  } else {
    const one = (v: number | null) => (v === null ? 0 : 1);
    next = [...buckets, { t, n: { cpu: one(s.cpu), mem: one(s.mem), rx: one(s.rx), tx: one(s.tx) }, cpu: s.cpu, mem: s.mem, rx: s.rx, tx: s.tx }];
  }
  const from = t - keepMs;
  let start = 0;
  while (start < next.length && next[start].t < from) start++;
  return start ? next.slice(start) : next;
}

/** Buckets as samples at the middle of their minute, for the charts. */
export const toSamples = (buckets: Bucket[]): Sample[] => buckets.map((b) => ({ t: b.t + BUCKET_MS / 2, cpu: b.cpu, mem: b.mem, rx: b.rx, tx: b.tx }));

/** The part of `buckets` inside the last `windowMs` before `now`. */
export const within = (buckets: Bucket[], now: number, windowMs: number): Bucket[] => buckets.filter((b) => b.t >= now - windowMs);

export interface ArchiveStore {
  load(hostId: string): Promise<Bucket[]>;
  save(hostId: string, buckets: Bucket[]): Promise<void>;
  /** One host, or all of them. */
  clear(hostId?: string): Promise<void>;
}

/** In memory; what the tests use, and what the app falls back to where IndexedDB isn't there. */
export class MemoryStore implements ArchiveStore {
  data = new Map<string, Bucket[]>();
  async load(id: string) {
    return this.data.get(id) ?? [];
  }
  async save(id: string, b: Bucket[]) {
    this.data.set(id, b);
  }
  async clear(id?: string) {
    if (id === undefined) this.data.clear();
    else this.data.delete(id);
  }
}

const DB = "sshvault-metrics";
const TABLE = "history";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(TABLE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function request<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return openDb().then(
    (db) =>
      new Promise<T>((resolve, reject) => {
        const tx = db.transaction(TABLE, mode);
        const r = run(tx.objectStore(TABLE));
        tx.oncomplete = () => {
          db.close();
          resolve(r.result);
        };
        tx.onerror = tx.onabort = () => {
          db.close();
          reject(tx.error);
        };
      }),
  );
}

/** The archive on disk. Any failure (private window, quota) just means no history; the app carries on. */
export const idbStore: ArchiveStore = {
  load: (id) => request<Bucket[] | undefined>("readonly", (s) => s.get(id)).then((v) => v ?? []).catch(() => []),
  save: (id, b) => request("readwrite", (s) => s.put(b, id)).then(() => undefined).catch(() => undefined),
  clear: (id) =>
    request("readwrite", (s) => (id === undefined ? s.clear() : s.delete(id))).then(() => undefined).catch(() => undefined),
};
