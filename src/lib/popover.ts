// Placement math for floating UI (context menus, dropdowns) that must stay
// fully inside the window. Pure so it can be unit tested.

export const EDGE = 6;

export interface Viewport {
  width: number;
  height: number;
}

/**
 * Where to put a context menu of size `w`×`h` opened at the pointer.
 * Opens down-right like the OS; flips up or left when that would overflow,
 * and never leaves the window. `maxHeight` is set when even a flip can't fit.
 */
export function placeMenu(
  x: number,
  y: number,
  w: number,
  h: number,
  vp: Viewport,
): { left: number; top: number; maxHeight?: number } {
  const room = vp.height - EDGE * 2;
  let left = x + w + EDGE <= vp.width ? x : x - w;
  left = Math.max(EDGE, Math.min(left, vp.width - w - EDGE));
  if (h > room) return { left, top: EDGE, maxHeight: room };
  let top = y + h + EDGE <= vp.height ? y : y - h;
  top = Math.max(EDGE, Math.min(top, vp.height - h - EDGE));
  return { left, top };
}

export interface Anchor {
  left: number;
  top: number;
  bottom: number;
  width: number;
}

/**
 * Where to put a dropdown list under (or, when there is more room, above)
 * the field it belongs to. `wanted` is the list's natural height.
 */
export function placeDropdown(
  anchor: Anchor,
  wanted: number,
  vp: Viewport,
  gap = 4,
): { left: number; width: number; top?: number; bottom?: number; maxHeight: number } {
  const below = vp.height - anchor.bottom - gap - EDGE;
  const above = anchor.top - gap - EDGE;
  const width = Math.min(anchor.width, vp.width - EDGE * 2);
  const left = Math.max(EDGE, Math.min(anchor.left, vp.width - width - EDGE));
  if (wanted <= below || below >= above) {
    return { left, width, top: anchor.bottom + gap, maxHeight: Math.max(0, Math.min(wanted, below)) };
  }
  return { left, width, bottom: vp.height - anchor.top + gap, maxHeight: Math.max(0, Math.min(wanted, above)) };
}

/**
 * Every group path a host could be put in: saved groups, groups hosts already
 * use, and each of their parents ("a/b/c" also offers "a" and "a/b").
 */
export function allGroupPaths(paths: Iterable<string | undefined | null>): string[] {
  const out = new Set<string>();
  for (const raw of paths) {
    const parts = (raw ?? "").split("/").map((s) => s.trim()).filter(Boolean);
    for (let i = 1; i <= parts.length; i++) out.add(parts.slice(0, i).join("/"));
  }
  return [...out].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }) || (a < b ? -1 : a > b ? 1 : 0));
}

/** Case-insensitive match of every whitespace-separated word, in any field. */
export function matchesQuery(query: string, ...fields: (string | undefined | null)[]): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = fields.filter(Boolean).join(" ").toLowerCase();
  return words.every((w) => hay.includes(w));
}
