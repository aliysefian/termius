// Saved queries: a name, a description, the SQL, and the connection it was written for. Kept in this browser
// profile (like the history) and never in the vault, so what people write in queries doesn't sync.
export const SAVED_KEY = "sshvault.db.saved";
export const SAVED_MAX = 500;

export interface SavedQuery {
  id: string;
  name: string;
  description: string;
  sql: string;
  /** The connection it was saved from, by id and by name (the name survives a deleted connection). */
  connectionId: string | null;
  connectionName: string | null;
  pinned: boolean;
  updatedAt: number;
}

export function newSaved(input: { name: string; sql: string; description?: string; connectionId?: string | null; connectionName?: string | null }, now = Date.now()): SavedQuery {
  return {
    id: typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `${now}-${Math.random().toString(16).slice(2)}`,
    name: input.name.trim() || "Untitled query",
    description: input.description?.trim() ?? "",
    sql: input.sql,
    connectionId: input.connectionId ?? null,
    connectionName: input.connectionName ?? null,
    pinned: false,
    updatedAt: now,
  };
}

/** Pinned first, then newest. */
export function sortSaved(list: SavedQuery[]): SavedQuery[] {
  return [...list].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt - a.updatedAt || a.name.localeCompare(b.name));
}

export function upsert(list: SavedQuery[], q: SavedQuery): SavedQuery[] {
  const rest = list.filter((x) => x.id !== q.id);
  return [q, ...rest].slice(0, SAVED_MAX);
}

export const remove = (list: SavedQuery[], id: string) => list.filter((q) => q.id !== id);

export function search(list: SavedQuery[], text: string): SavedQuery[] {
  const t = text.trim().toLowerCase();
  if (!t) return list;
  return list.filter((q) => [q.name, q.description, q.sql, q.connectionName ?? ""].some((f) => f.toLowerCase().includes(t)));
}

export function load(storage: Pick<Storage, "getItem"> | undefined = tryStorage()): SavedQuery[] {
  try {
    const raw = storage?.getItem(SAVED_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is SavedQuery => !!x && typeof x.id === "string" && typeof x.name === "string" && typeof x.sql === "string")
      .map((x) => ({
        id: x.id,
        name: x.name,
        description: typeof x.description === "string" ? x.description : "",
        sql: x.sql,
        connectionId: typeof x.connectionId === "string" ? x.connectionId : null,
        connectionName: typeof x.connectionName === "string" ? x.connectionName : null,
        pinned: x.pinned === true,
        updatedAt: typeof x.updatedAt === "number" ? x.updatedAt : 0,
      }))
      .slice(0, SAVED_MAX);
  } catch {
    return [];
  }
}

export function save(list: SavedQuery[], storage: Pick<Storage, "setItem"> | undefined = tryStorage()) {
  try {
    storage?.setItem(SAVED_KEY, JSON.stringify(list.slice(0, SAVED_MAX)));
  } catch {
    // Blocked storage: saved queries just don't persist.
  }
}

function tryStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}
