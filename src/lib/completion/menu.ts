// What the popup lists for the line typed so far, and where it goes. Pure, so
// it can be tested without a terminal.
import { fuzzyScore } from "$lib/fuzzy";
import { promptedVariables } from "$lib/snippetvars";
import type { Suggestion } from "./command";
import type { Held } from "./history";

export interface SnippetLike {
  id: string;
  label: string;
  command: string;
  description?: string;
}

export interface MenuItem {
  /** Stable across refreshes, so the selection can stay on it. */
  id: string;
  kind: "history" | "snippet" | "command" | "subcommand" | "option" | "value";
  /** The heading it is listed under. */
  group: string;
  label: string;
  /** Character positions in `label` the typed text matched. */
  labelHit: number[];
  detail: string;
  detailHit: number[];
  /** What goes onto the line. For a snippet this is the unrendered body. */
  insert: string;
  /** The snippet asks for values first. */
  variables: boolean;
  /** Characters of the line it replaces; the whole line when absent. */
  erase?: number;
}

export const MAX_HISTORY_ITEMS = 8;
export const MAX_SNIPPET_ITEMS = 5;

/**
 * Where the characters of `query` matched in `text`: the whole query as one run if it occurs as such,
 * otherwise each character at its first place after the one before. Same rule as `fuzzyScore`, so
 * what is highlighted is what was scored.
 */
export function matchPositions(query: string, text: string): number[] {
  const q = query.toLowerCase().trim();
  if (!q) return [];
  const t = text.toLowerCase();
  const direct = t.indexOf(q);
  if (direct >= 0) return Array.from({ length: q.length }, (_, i) => direct + i);
  const out: number[] = [];
  let ti = 0;
  for (const ch of q) {
    if (ch === " ") continue;
    const found = t.indexOf(ch, ti);
    if (found < 0) return [];
    out.push(found);
    ti = found + 1;
  }
  return out;
}

const firstLine = (s: string) => s.split("\n", 1)[0];

function describe(e: Held): string {
  const parts: string[] = [];
  if (e.cwd) parts.push(e.cwd.split("/").filter(Boolean).pop() ?? "/");
  if (e.exit !== null && e.exit !== 0) parts.push(`exit ${e.exit}`);
  return parts.join(" · ");
}

export function historyItems(text: string, found: Held[]): MenuItem[] {
  const typed = text.trim();
  return found
    .filter((e) => e.command !== typed)
    .slice(0, MAX_HISTORY_ITEMS)
    .map((e) => ({
      id: `h:${e.command}`,
      kind: "history" as const,
      group: "History",
      label: e.command,
      labelHit: matchPositions(text, e.command),
      detail: describe(e),
      detailHit: [],
      insert: e.command,
      variables: false,
    }));
}

/** Snippets whose label or command matches, best first. Several-line snippets are left out: typing one would run it. */
export function snippetItems(text: string, snippets: SnippetLike[]): MenuItem[] {
  const scored: { item: MenuItem; s: number }[] = [];
  for (const sn of snippets) {
    if (sn.command.includes("\n") || !sn.command.trim()) continue;
    const byLabel = fuzzyScore(text, sn.label);
    const byCommand = fuzzyScore(text, sn.command);
    if (byLabel === null && byCommand === null) continue;
    // A match in the name counts for more than one in the body.
    const labelWins = byLabel !== null && byLabel + 100 >= (byCommand ?? -Infinity);
    const names = promptedVariables(sn.command);
    const own = sn.description?.trim() || firstLine(sn.command);
    scored.push({
      s: labelWins ? byLabel! + 100 : byCommand!,
      item: {
        id: `s:${sn.id}`,
        kind: "snippet",
        group: "Snippets",
        label: sn.label,
        labelHit: labelWins ? matchPositions(text, sn.label) : [],
        detail: names.length ? `${own} · asks for ${names.join(", ")}` : own,
        detailHit: !labelWins && own === firstLine(sn.command) ? matchPositions(text, own) : [],
        insert: sn.command,
        variables: names.length > 0,
      },
    });
  }
  scored.sort((a, b) => b.s - a.s || a.item.label.localeCompare(b.item.label));
  return scored.slice(0, MAX_SNIPPET_ITEMS).map((x) => x.item);
}

export interface MenuGroup {
  title: string;
  items: MenuItem[];
}

const SPEC_GROUP: Record<Suggestion["kind"], string> = { command: "Commands", subcommand: "Commands", option: "Options", value: "Values" };

/** What the command specs say can come next, replacing only the word being typed. */
export function specItems(typedWord: string, found: Suggestion[]): MenuItem[] {
  return found.map((f) => ({
    id: `c:${f.kind}:${f.name}`,
    kind: f.kind,
    group: SPEC_GROUP[f.kind],
    label: f.name,
    // The part of the name already typed.
    labelHit: f.name.startsWith(typedWord) ? Array.from({ length: typedWord.length }, (_, i) => i) : [],
    detail: f.description,
    detailHit: [],
    insert: f.insert,
    variables: false,
    erase: f.replaces,
  }));
}

/**
 * Bucket items under their headings, in the order the headings first appear. A snippet whose body is
 * already listed from history is dropped.
 */
export function groups(items: MenuItem[]): MenuGroup[] {
  const seen = new Set(items.filter((i) => i.kind === "history").map((i) => i.insert));
  const out: MenuGroup[] = [];
  for (const item of items) {
    if (item.kind === "snippet" && seen.has(item.insert)) continue;
    let g = out.find((x) => x.title === item.group);
    if (!g) out.push((g = { title: item.group, items: [] }));
    g.items.push(item);
  }
  return out;
}

export const flatten = (gs: MenuGroup[]): MenuItem[] => gs.flatMap((g) => g.items);

// -- placement ---------------------------------------------------------------

export const MENU_ROW = 24;
export const MENU_HEADER = 22;
export const MENU_PAD = 4;
export const MENU_WIDTH = 460;

export function menuHeight(gs: MenuGroup[]): number {
  return MENU_PAD * 2 + gs.length * MENU_HEADER + flatten(gs).length * MENU_ROW;
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Placement extends Box {
  above: boolean;
}

/**
 * The popup's box, in the same coordinates as `anchor` (the cursor's cell) and `bounds` (the area it
 * may use: the pane, cut down to the window). Under the line when it fits, above it when that has
 * more room; never outside the bounds, and shorter rather than overflowing.
 */
export function placeMenu(anchor: Box, size: { width: number; height: number }, bounds: Box): Placement {
  const width = Math.min(size.width, bounds.width);
  const left = Math.min(Math.max(anchor.left, bounds.left), bounds.left + bounds.width - width);
  const below = bounds.top + bounds.height - (anchor.top + anchor.height);
  const above = anchor.top - bounds.top;
  const goAbove = size.height > below && above > below;
  const room = goAbove ? above : below;
  const height = Math.max(0, Math.min(size.height, room));
  return { left, width, height, top: goAbove ? anchor.top - height : anchor.top + anchor.height, above: goAbove };
}
