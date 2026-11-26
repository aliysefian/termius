// The relationship graph around one resource: verified edges only, a bounded neighbourhood, and a layered layout
// (host, then what it holds, then containers, then ports). No physics: a column per kind, rows in order.
import type { Relationship, ResourceType } from "./model";
import type { ResourceView } from "./inventory";

export interface GNode {
  id: string;
  resource: ResourceView;
  x: number;
  y: number;
}
export interface GEdge {
  from: string;
  to: string;
  type: Relationship["type"];
  evidence: string;
}
export interface Neighborhood {
  nodes: GNode[];
  edges: GEdge[];
  /** There were more neighbours than the limit; the rest are one step further and can be opened. */
  truncated: boolean;
  width: number;
  height: number;
}

export const NODE_W = 188;
export const NODE_H = 46;
const COL = 250;
const ROW = 64;

const COLUMN: Record<ResourceType, number> = { host: 0, docker_engine: 1, systemd_service: 1, tmux_session: 1, container: 2, port: 3 };

/** Everything within `depth` verified steps of `focus`, at most `maxNodes` of it, nearest first. */
export function neighborhood(views: ResourceView[], edges: Relationship[], focus: string, depth = 2, maxNodes = 40): Neighborhood {
  const byId = new Map(views.map((v) => [v.id, v]));
  const adj = new Map<string, Relationship[]>();
  for (const e of edges) {
    (adj.get(e.from) ?? adj.set(e.from, []).get(e.from)!).push(e);
    (adj.get(e.to) ?? adj.set(e.to, []).get(e.to)!).push(e);
  }
  const start = byId.get(focus);
  if (!start) return { nodes: [], edges: [], truncated: false, width: 0, height: 0 };
  const seen = new Map<string, number>([[focus, 0]]);
  const queue = [focus];
  let truncated = false;
  for (let i = 0; i < queue.length; i++) {
    const id = queue[i];
    const d = seen.get(id)!;
    if (d >= depth) continue;
    for (const e of adj.get(id) ?? []) {
      const other = e.from === id ? e.to : e.from;
      if (seen.has(other) || !byId.has(other)) continue;
      if (seen.size >= maxNodes) {
        truncated = true;
        continue;
      }
      seen.set(other, d + 1);
      queue.push(other);
    }
  }
  const members = [...seen.keys()].map((id) => byId.get(id)!);
  // One column per kind, in the order found, the focus's own column first so it is at the top of its column.
  const rows = new Map<number, number>();
  const nodes: GNode[] = members
    .sort((a, b) => COLUMN[a.type] - COLUMN[b.type] || (a.id === focus ? -1 : b.id === focus ? 1 : a.name.localeCompare(b.name)))
    .map((r) => {
      const c = COLUMN[r.type];
      const row = rows.get(c) ?? 0;
      rows.set(c, row + 1);
      return { id: r.id, resource: r, x: c * COL, y: row * ROW };
    });
  const keep = new Set(nodes.map((n) => n.id));
  const out: GEdge[] = edges.filter((e) => keep.has(e.from) && keep.has(e.to)).map((e) => ({ from: e.from, to: e.to, type: e.type, evidence: e.evidence }));
  const width = Math.max(...nodes.map((n) => n.x)) + NODE_W;
  const height = Math.max(...nodes.map((n) => n.y)) + NODE_H;
  return { nodes, edges: out, truncated, width, height };
}

/** The ancestors of a resource to the host, nearest last: what a "belongs to" line is made from. */
export function parentsOf(id: string, edges: Relationship[]): string[] {
  const parent = new Map<string, string>();
  for (const e of edges) if (e.type === "contains" || e.type === "publishes_port") parent.set(e.to, e.from);
  const chain: string[] = [];
  const seen = new Set([id]);
  for (let p = parent.get(id); p && !seen.has(p); p = parent.get(p)) {
    seen.add(p);
    chain.unshift(p);
  }
  return chain;
}
