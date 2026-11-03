// The runbook library: the saved runbooks as rows to search, group and sort. A row's description and category are read from the
// document itself (a runbook has them in its text), so they are the same wherever it is opened. Favorites are the person's own, on
// this computer.
import { load } from "./runbookdoc";

export const CATEGORIES = ["Deployments", "Maintenance", "Monitoring", "Backups", "Security", "Custom"] as const;
export const UNSORTED = "Custom";

export interface LibraryItem {
  id: string;
  name: string;
  description: string;
  category: string;
  /** Unix seconds. */
  updatedAt: number;
  favorite: boolean;
  steps: number;
  /** Its text doesn't read, so it can be opened and fixed but not run. */
  broken: boolean;
}

export interface RunbookRecordLike {
  id: string;
  updated_at: number;
  deleted?: boolean;
  data: { name: string; body: string } | null;
}

export function buildLibrary(records: RunbookRecordLike[], favorites: string[]): LibraryItem[] {
  const fav = new Set(favorites);
  return records
    .filter((r) => !r.deleted && r.data)
    .map((r) => {
      const l = load(r.data!.body);
      return {
        id: r.id,
        name: r.data!.name,
        description: l.model?.description ?? "",
        category: l.model?.category.trim() || UNSORTED,
        updatedAt: r.updated_at,
        favorite: fav.has(r.id),
        steps: l.model?.steps.length ?? 0,
        broken: !l.model,
      };
    });
}

/** Rows whose name, description or category contain every word typed. */
export function searchLibrary(items: LibraryItem[], query: string): LibraryItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return items;
  return items.filter((i) => {
    const hay = `${i.name}\n${i.description}\n${i.category}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export interface LibraryGroup {
  name: string;
  items: LibraryItem[];
}

const byName = (a: LibraryItem, b: LibraryItem) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

/** The standard categories first (in their order, only when they have runbooks), then the person's own alphabetically. */
export function groupByCategory(items: LibraryItem[]): LibraryGroup[] {
  const map = new Map<string, LibraryItem[]>();
  for (const i of items) map.set(i.category, [...(map.get(i.category) ?? []), i]);
  const std = (CATEGORIES as readonly string[]).filter((c) => map.has(c));
  const own = [...map.keys()].filter((c) => !(CATEGORIES as readonly string[]).includes(c)).sort((a, b) => a.localeCompare(b));
  return [...std, ...own].map((name) => ({ name, items: [...map.get(name)!].sort(byName) }));
}

/** The runbooks changed most recently, newest first. */
export const recent = (items: LibraryItem[], n = 3) => [...items].sort((a, b) => b.updatedAt - a.updatedAt || byName(a, b)).slice(0, n);

export const favorites = (items: LibraryItem[]) => items.filter((i) => i.favorite).sort(byName);

/** Every category in use, the standard ones and the person's own, for the category picker. */
export function categoryChoices(items: LibraryItem[]): string[] {
  const own = new Set(items.map((i) => i.category));
  return [...CATEGORIES, ...[...own].filter((c) => !(CATEGORIES as readonly string[]).includes(c)).sort((a, b) => a.localeCompare(b))];
}

export function toggleFavorite(favorites: string[], id: string): string[] {
  return favorites.includes(id) ? favorites.filter((f) => f !== id) : [...favorites, id];
}

/** A name for a copy that no runbook has. */
export function copyName(name: string, taken: string[]): string {
  const base = `${name} (copy)`;
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) if (!taken.includes(`${name} (copy ${n})`)) return `${name} (copy ${n})`;
}

/** "5 min ago", "yesterday", or a date, for the library rows. */
export function ago(unixSecs: number, nowMs = Date.now()): string {
  const s = Math.max(0, Math.floor(nowMs / 1000 - unixSecs));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 172800) return "yesterday";
  if (s < 86400 * 30) return `${Math.floor(s / 86400)} days ago`;
  return new Date(unixSecs * 1000).toLocaleDateString();
}
