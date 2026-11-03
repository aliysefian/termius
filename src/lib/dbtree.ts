// The Database Explorer's tree as a flat list of rows. A tree of thousands of tables is drawn by showing only the rows
// in view, so everything the explorer shows is first turned into one array here: connections, the objects below them
// grouped by kind (Tables, Views, Functions…), loading and error lines, and the search. Pure and tested.
import type { DbNodeKind, DbSearchHit, DbTreeNode, Uuid } from "./types";

export const pathKey = (path: string[]) => path.join("\u0000");

/** What the explorer knows about one open connection. */
export interface SessionView {
  nodes: Record<string, DbTreeNode[]>;
  open: Record<string, boolean>;
  loading: Record<string, boolean>;
  failed: Record<string, string>;
}

export interface ConnectionView {
  id: Uuid;
  name: string;
  group: string;
  engine: string;
  /** The open connection, or undefined when it is not connected. */
  session?: SessionView;
  connecting?: boolean;
}

/** The object groups a level is split into when it holds more than one kind. */
export type GroupId = "tables" | "views" | "functions" | "procedures" | "sequences" | "events" | "columns" | "indexes" | "triggers";

export const GROUP_LABEL: Record<GroupId, string> = {
  tables: "Tables",
  views: "Views",
  functions: "Functions",
  procedures: "Procedures",
  sequences: "Sequences",
  events: "Events",
  columns: "Columns",
  indexes: "Indexes",
  triggers: "Triggers",
};

const GROUP_OF: Partial<Record<DbNodeKind, GroupId>> = {
  table: "tables",
  view: "views",
  function: "functions",
  procedure: "procedures",
  sequence: "sequences",
  event: "events",
  column: "columns",
  index: "indexes",
  trigger: "triggers",
};

const GROUP_ORDER: GroupId[] = ["columns", "tables", "views", "functions", "procedures", "sequences", "events", "indexes", "triggers"];

/** Groups that start open. The rest stay closed until opened, so a schema with a hundred routines starts short. */
const OPEN_BY_DEFAULT: GroupId[] = ["tables", "views", "columns"];

/** The type filter of the search box. */
export type TypeFilter = "all" | "tables" | "views" | "routines" | "other";

export const TYPE_FILTERS: { id: TypeFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "tables", label: "Tables" },
  { id: "views", label: "Views" },
  { id: "routines", label: "Routines" },
  { id: "other", label: "Other" },
];

/** Whether an object of this kind passes the type filter. Containers (databases, schemas, folders) always do. */
export function passesType(kind: DbNodeKind, filter: TypeFilter): boolean {
  if (filter === "all") return true;
  switch (kind) {
    case "database":
    case "schema":
    case "folder":
      return true;
    case "table":
      return filter === "tables";
    case "view":
      return filter === "views";
    case "function":
    case "procedure":
      return filter === "routines";
    default:
      return filter === "other";
  }
}

export type RowKind = "section" | "connection" | "node" | "group" | "loading" | "error" | "empty" | "hits-header" | "hit";

export interface TreeRow {
  /** Stable across redraws, for keyed lists and focus. */
  key: string;
  kind: RowKind;
  depth: number;
  connId: Uuid;
  /** The tree path of a node or a group's parent; for a hit, the hit's full path. */
  path: string[];
  label: string;
  node?: DbTreeNode;
  group?: GroupId;
  hit?: DbSearchHit;
  expandable: boolean;
  open: boolean;
  /** A group's object count. */
  count?: number;
  /** For a connection row: whether it is connected. */
  connected?: boolean;
  connecting?: boolean;
  engine?: string;
}

export interface BuildOptions {
  query: string;
  type: TypeFilter;
  /** Open/closed state of groups, by [`groupKey`]. A missing entry means the group's default. */
  groups: Record<string, boolean>;
  /** Connections collapsed in the list, by id. */
  collapsed: Record<string, boolean>;
  /** The server-side search results of each connection, by id. */
  hits?: Record<string, DbSearchHit[]>;
  /** Whether a search of the server is still running, by connection id. */
  searching?: Record<string, boolean>;
}

export const groupKey = (connId: Uuid, path: string[], group: GroupId) => `${connId}\u0001${pathKey(path)}\u0001${group}`;

/** Case-insensitive substring match; an empty query matches everything. */
export const matches = (name: string, query: string) => query === "" || name.toLowerCase().includes(query.toLowerCase());

/** The parts of `text` to draw, with the ones that match `query` marked. */
export function highlightParts(text: string, query: string): { text: string; match: boolean }[] {
  const q = query.trim();
  if (!q) return [{ text, match: false }];
  const out: { text: string; match: boolean }[] = [];
  const lower = text.toLowerCase();
  const needle = q.toLowerCase();
  let at = 0;
  for (;;) {
    const i = lower.indexOf(needle, at);
    if (i < 0) break;
    if (i > at) out.push({ text: text.slice(at, i), match: false });
    out.push({ text: text.slice(i, i + needle.length), match: true });
    at = i + needle.length;
  }
  if (at < text.length) out.push({ text: text.slice(at), match: false });
  return out.length ? out : [{ text, match: false }];
}

/**
 * The rows to draw. With no search it is the tree as opened; with one, only what matches (and the path to it), with
 * the groups opened, plus what the server's own search found below the connection.
 */
export function buildRows(connections: ConnectionView[], o: BuildOptions): TreeRow[] {
  const rows: TreeRow[] = [];
  const q = o.query.trim();
  const searching = q !== "" || o.type !== "all";
  let lastGroup: string | null = "";

  for (const c of connections) {
    const connRows: TreeRow[] = [];
    const s = c.session;
    const open = !!s && !o.collapsed[c.id];
    if (s && (open || searching)) {
      level(c, s, [], 1, o, q, connRows);
      if (q) {
        const seen = new Set(connRows.filter((r) => r.kind === "node").map((r) => r.key));
        const hits = (o.hits?.[c.id] ?? []).filter((h) => passesType(h.kind, o.type) && !seen.has(hitNodeKey(c.id, h)));
        if (hits.length || o.searching?.[c.id]) {
          connRows.push({ key: `${c.id}\u0002hits`, kind: "hits-header", depth: 1, connId: c.id, path: [], label: o.searching?.[c.id] ? "Searching the server…" : `More on the server (${hits.length})`, expandable: false, open: false });
          for (const h of hits) {
            connRows.push({ key: `${c.id}\u0002hit\u0000${pathKey([...h.parent, h.name])}\u0000${h.kind}`, kind: "hit", depth: 1, connId: c.id, path: [...h.parent, h.name], label: h.name, hit: h, expandable: false, open: false });
          }
        }
      }
    }
    // While searching, a connection shows only if it, or something below it, matches.
    if (searching && !matches(c.name, q) && connRows.length === 0) continue;
    if (c.group !== lastGroup) {
      lastGroup = c.group;
      if (c.group) rows.push({ key: `sec\u0000${c.group}`, kind: "section", depth: 0, connId: c.id, path: [], label: c.group, expandable: false, open: false });
    }
    rows.push({ key: `conn\u0000${c.id}`, kind: "connection", depth: 0, connId: c.id, path: [], label: c.name, expandable: !!s, open, connected: !!s, connecting: !!c.connecting, engine: c.engine });
    rows.push(...connRows);
  }
  return rows;
}

/** The key a loaded node has as a row, so a server hit for the same object is not listed twice. */
function hitNodeKey(connId: Uuid, h: DbSearchHit): string {
  return `${connId}\u0000${pathKey([...h.parent, h.name])}\u0000${h.kind}`;
}

function level(c: ConnectionView, s: SessionView, path: string[], depth: number, o: BuildOptions, q: string, out: TreeRow[]): void {
  const k = pathKey(path);
  if (s.failed[k]) {
    out.push({ key: `${c.id}\u0000${k}\u0000err`, kind: "error", depth, connId: c.id, path, label: s.failed[k], expandable: false, open: false });
    return;
  }
  if (s.loading[k] && !s.nodes[k]) {
    out.push({ key: `${c.id}\u0000${k}\u0000load`, kind: "loading", depth, connId: c.id, path, label: "Loading…", expandable: false, open: false });
    return;
  }
  const nodes = (s.nodes[k] ?? []).filter((n) => passesType(n.kind, o.type));
  const searching = q !== "" || o.type !== "all";

  // Which nodes to show. A node shows when it matches, or when something loaded below it matches.
  const show = (n: DbTreeNode): boolean => {
    if (!searching) return true;
    if (q === "") return true;
    if (matches(n.name, q)) return true;
    return n.expandable && subtreeMatches(s, [...path, n.name], q, o.type);
  };
  const shown = nodes.filter(show);

  const kinds = new Set(shown.map((n) => GROUP_OF[n.kind]).filter((g): g is GroupId => !!g));
  const grouped = kinds.size >= 2 || (kinds.size === 1 && (kinds.has("columns") || kinds.has("indexes") || kinds.has("triggers")) && shown.length > 0 && path.length >= 2);
  if (!grouped) {
    for (const n of shown) node(c, s, path, n, depth, o, q, out);
    if (shown.length === 0 && s.nodes[k] && !searching) out.push({ key: `${c.id}\u0000${k}\u0000empty`, kind: "empty", depth, connId: c.id, path, label: "Nothing here", expandable: false, open: false });
    return;
  }
  for (const g of GROUP_ORDER) {
    const members = shown.filter((n) => GROUP_OF[n.kind] === g);
    if (members.length === 0) continue;
    const gk = groupKey(c.id, path, g);
    const open = searching ? true : (o.groups[gk] ?? OPEN_BY_DEFAULT.includes(g));
    out.push({ key: `grp\u0000${gk}`, kind: "group", depth, connId: c.id, path, label: GROUP_LABEL[g], group: g, expandable: true, open, count: members.length });
    if (open) for (const n of members) node(c, s, path, n, depth + 1, o, q, out);
  }
  // Anything of a kind with no group (a folder, a database) is shown plainly after the groups.
  for (const n of shown) if (!GROUP_OF[n.kind]) node(c, s, path, n, depth, o, q, out);
}

function node(c: ConnectionView, s: SessionView, path: string[], n: DbTreeNode, depth: number, o: BuildOptions, q: string, out: TreeRow[]): void {
  const child = [...path, n.name];
  const ck = pathKey(child);
  const searching = q !== "" || o.type !== "all";
  // While searching, a node whose loaded children match is shown open.
  const open = !!s.open[ck] || (searching && q !== "" && n.expandable && !matches(n.name, q) && subtreeMatches(s, child, q, o.type));
  out.push({ key: `${c.id}\u0000${ck}\u0000${n.kind}`, kind: "node", depth, connId: c.id, path: child, label: n.name, node: n, expandable: n.expandable, open });
  if (n.expandable && open) level(c, s, child, depth + 1, o, q, out);
}

function subtreeMatches(s: SessionView, path: string[], q: string, type: TypeFilter): boolean {
  const kids = s.nodes[pathKey(path)];
  if (!kids) return false;
  return kids.some((n) => passesType(n.kind, type) && (matches(n.name, q) || (n.expandable && subtreeMatches(s, [...path, n.name], q, type))));
}

/** The row to focus after an arrow key, or the same one at the ends. */
export function moveFocus(rows: TreeRow[], from: number, key: "ArrowDown" | "ArrowUp" | "Home" | "End"): number {
  if (rows.length === 0) return -1;
  if (key === "Home") return 0;
  if (key === "End") return rows.length - 1;
  const next = from + (key === "ArrowDown" ? 1 : -1);
  return Math.min(rows.length - 1, Math.max(0, next));
}

/** The index of the row's parent, for the Left arrow on a closed row. */
export function parentIndex(rows: TreeRow[], i: number): number {
  const d = rows[i]?.depth ?? 0;
  for (let j = i - 1; j >= 0; j--) if (rows[j].depth < d) return j;
  return -1;
}
