// Helpers for the JSON tree in the Inspect tab.

/** `{3}` for an object with three keys, `[2]` for a two-item array. */
export function jsonPreview(v: unknown): string {
  if (Array.isArray(v)) return v.length === 0 ? "[]" : `[${v.length}]`;
  if (v && typeof v === "object") {
    const n = Object.keys(v).length;
    return n === 0 ? "{}" : `{${n}}`;
  }
  return String(v);
}

/** Whether a key or a value anywhere below this node contains `query` (case-insensitive). */
export function jsonMatches(value: unknown, label: string | null, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (label !== null && label.toLowerCase().includes(q)) return true;
  if (value !== null && typeof value === "object") {
    return Object.entries(value as Record<string, unknown>).some(([k, v]) => jsonMatches(v, k, query));
  }
  return String(value).toLowerCase().includes(q);
}
