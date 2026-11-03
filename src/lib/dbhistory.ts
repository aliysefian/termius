// Query history: kept in this browser profile only, never in the vault, so nothing a user typed into a query
// (names, ids, sometimes secrets) syncs. Each entry says how the run ended, so the list is a record and not just
// a list of text. Statements that set a password are stored with the secret hidden.
export const HISTORY_KEY = "sshvault.db.history";
export const HISTORY_MAX = 100;
/** The retention choices offered in the settings. 0 keeps nothing. */
export const HISTORY_KEEP_CHOICES = [0, 50, 100, 500, 1000] as const;

export interface HistoryEntry {
  sql: string;
  at: number;
  /** Name of the connection it ran on. */
  connection: string;
  /** How long it took, when it finished. */
  ms?: number;
  /** Rows returned (a query) or affected (a statement). */
  rows?: number | null;
  ok?: boolean;
  /** The server's message when it failed. */
  error?: string;
}

/**
 * The statement with a password literal hidden: `IDENTIFIED BY '…'`, `PASSWORD '…'`, `PASSWORD = '…'`,
 * `WITH PASSWORD '…'`, `SET PASSWORD FOR … = '…'`. Not a promise that no secret is in a query, only the common ways
 * a password gets typed into one.
 */
export function redactSecrets(sql: string): string {
  return sql
    .replace(/(\b(?:identified\s+by|password)\s*(?:=\s*)?)(?:'(?:[^']|'')*'|"(?:[^"]|"")*")/gi, "$1'***'")
    .replace(/(\bset\s+password\s+for\s+[^=;]+=\s*)(?:'(?:[^']|'')*'|"(?:[^"]|"")*")/gi, "$1'***'");
}

/** Newest first. A run of the same statement right after itself on the same connection is one entry, updated. */
export function addEntry(list: HistoryEntry[], e: HistoryEntry, max = HISTORY_MAX): HistoryEntry[] {
  const sql = redactSecrets(e.sql.trim());
  if (!sql || max <= 0) return max <= 0 ? [] : list;
  const entry = { ...e, sql };
  const [first, ...rest] = list;
  if (first && first.sql === sql && first.connection === e.connection && first.ok === undefined) {
    // The entry written when the run started is completed when it ends.
    return [entry, ...rest].slice(0, max);
  }
  return [entry, ...list].slice(0, max);
}

/** Record how a run ended on the entry that was added when it started. */
export function finishEntry(list: HistoryEntry[], started: { sql: string; at: number; connection: string }, outcome: { ms: number; rows: number | null; ok: boolean; error?: string }): HistoryEntry[] {
  const sql = redactSecrets(started.sql.trim());
  return list.map((h) => (h.at === started.at && h.connection === started.connection && h.sql === sql ? { ...h, ...outcome, error: outcome.error?.slice(0, 500) } : h));
}

export function search(list: HistoryEntry[], text: string): HistoryEntry[] {
  const t = text.trim().toLowerCase();
  if (!t) return list;
  return list.filter((h) => h.sql.toLowerCase().includes(t) || h.connection.toLowerCase().includes(t) || (h.error ?? "").toLowerCase().includes(t));
}

export function loadHistory(storage: Pick<Storage, "getItem"> | undefined = tryStorage()): HistoryEntry[] {
  try {
    const raw = storage?.getItem(HISTORY_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is HistoryEntry => !!x && typeof x.sql === "string" && typeof x.at === "number" && typeof x.connection === "string")
      .slice(0, 1000);
  } catch {
    return [];
  }
}

export function saveHistory(list: HistoryEntry[], max = HISTORY_MAX, storage: Pick<Storage, "setItem"> | undefined = tryStorage()) {
  try {
    storage?.setItem(HISTORY_KEY, JSON.stringify(list.slice(0, Math.max(0, max))));
  } catch {
    // Private windows and blocked storage: history just doesn't persist.
  }
}

function tryStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}
