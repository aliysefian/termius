// Query history: kept in this browser profile only, never in the vault, so
// nothing a user typed into a query (names, ids, sometimes secrets) syncs.
export const HISTORY_KEY = "sshvault.db.history";
export const HISTORY_MAX = 100;

export interface HistoryEntry {
  sql: string;
  at: number;
  /** Name of the connection it ran on. */
  connection: string;
}

/** Newest first, with an identical statement on the same connection moved to the top rather than repeated. */
export function addEntry(list: HistoryEntry[], e: HistoryEntry, max = HISTORY_MAX): HistoryEntry[] {
  const sql = e.sql.trim();
  if (!sql) return list;
  const rest = list.filter((x) => !(x.sql === sql && x.connection === e.connection));
  return [{ ...e, sql }, ...rest].slice(0, max);
}

export function loadHistory(storage: Pick<Storage, "getItem"> | undefined = tryStorage()): HistoryEntry[] {
  try {
    const raw = storage?.getItem(HISTORY_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is HistoryEntry => !!x && typeof x.sql === "string" && typeof x.at === "number" && typeof x.connection === "string")
      .slice(0, HISTORY_MAX);
  } catch {
    return [];
  }
}

export function saveHistory(list: HistoryEntry[], storage: Pick<Storage, "setItem"> | undefined = tryStorage()) {
  try {
    storage?.setItem(HISTORY_KEY, JSON.stringify(list.slice(0, HISTORY_MAX)));
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
