// What reaches what, drawn from relationships that are really stored: a host's jump host, the proxy a host (or the
// start of its jump chain) goes through, a tunnel rule and the host it runs over, a database and the SSH host it is
// reached through. Nothing is inferred: if the data does not say two things are linked, there is no line between them.
// Pure, so it is tested without a window.

export type NodeType = "host" | "proxy" | "tunnel" | "database";
export type EdgeKind = "jump" | "proxy" | "tunnel" | "database";

export interface GraphNode {
  /** Unique across kinds: `host:<id>`, `proxy:<id>`, `tunnel:<id>`, `database:<id>`. */
  id: string;
  type: NodeType;
  label: string;
  /** The record's own id, for opening it. */
  ref: string;
  /** "production", "staging", "development" or "". */
  env: string;
  /** One line under the label: address, proxy kind, forward summary, engine. */
  sub: string;
  tags: string[];
  group: string;
}

export interface GraphEdge {
  id: string;
  /** Traffic goes from `from` to `to`: proxy -> first hop -> ... -> host -> the tunnels and databases that ride it. */
  from: string;
  to: string;
  kind: EdgeKind;
  label: string;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface GraphInput {
  hosts: { id: string; label: string; hostname: string; port: number; env: string; tags: string[]; group: string; jumpId?: string; proxyId?: string }[];
  proxies: { id: string; name: string; kind: string }[];
  forwards: { id: string; label: string; hostId: string; summary: string }[];
  databases: { id: string; name: string; engine: string; env: string; sshHostId?: string }[];
}

const hostNode = (h: GraphInput["hosts"][number]): GraphNode => ({
  id: `host:${h.id}`,
  type: "host",
  label: h.label,
  ref: h.id,
  env: h.env,
  sub: h.port === 22 ? h.hostname : `${h.hostname}:${h.port}`,
  tags: h.tags,
  group: h.group,
});

export function buildGraph(input: GraphInput): Graph {
  const nodes = new Map<string, GraphNode>();
  const edges = new Map<string, GraphEdge>();
  const hosts = new Map(input.hosts.map((h) => [h.id, h]));
  for (const h of input.hosts) nodes.set(`host:${h.id}`, hostNode(h));
  const proxies = new Map(input.proxies.map((p) => [p.id, p]));

  const link = (from: string, to: string, kind: EdgeKind, label: string) => {
    const id = `${from}>${to}:${kind}`;
    if (!edges.has(id) && from !== to) edges.set(id, { id, from, to, kind, label });
  };

  /** The first host of a jump chain: where traffic enters. A chain that loops stops where it came round. */
  const chainStart = (id: string): string => {
    const seen = new Set<string>();
    let cur = id;
    while (!seen.has(cur)) {
      seen.add(cur);
      const next = hosts.get(cur)?.jumpId;
      if (!next || !hosts.has(next)) break;
      cur = next;
    }
    return cur;
  };

  for (const h of input.hosts) {
    if (h.jumpId && hosts.has(h.jumpId)) link(`host:${h.jumpId}`, `host:${h.id}`, "jump", "jump host");
    if (h.proxyId) {
      const p = proxies.get(h.proxyId);
      if (p) {
        const pid = `proxy:${p.id}`;
        nodes.set(pid, { id: pid, type: "proxy", label: p.name, ref: p.id, env: "", sub: p.kind, tags: [], group: "" });
        link(pid, `host:${chainStart(h.id)}`, "proxy", "proxy");
      }
    }
  }
  for (const f of input.forwards) {
    if (!hosts.has(f.hostId)) continue;
    const tid = `tunnel:${f.id}`;
    nodes.set(tid, { id: tid, type: "tunnel", label: f.label, ref: f.id, env: hosts.get(f.hostId)?.env ?? "", sub: f.summary, tags: [], group: "" });
    link(`host:${f.hostId}`, tid, "tunnel", "tunnel");
  }
  for (const d of input.databases) {
    if (!d.sshHostId || !hosts.has(d.sshHostId)) continue;
    const did = `database:${d.id}`;
    nodes.set(did, { id: did, type: "database", label: d.name, ref: d.id, env: d.env, sub: d.engine, tags: [], group: "" });
    link(`host:${d.sshHostId}`, did, "database", "through");
  }
  return { nodes: [...nodes.values()], edges: [...edges.values()] };
}

/** Only what has at least one link: with hundreds of hosts, most stand alone and would bury the picture. */
export function connectedOnly(g: Graph): Graph {
  const linked = new Set(g.edges.flatMap((e) => [e.from, e.to]));
  return { nodes: g.nodes.filter((n) => linked.has(n.id)), edges: g.edges };
}

/** The graph limited to what matches (and what it is linked to, so a match is shown in context). */
export function focus(g: Graph, match: (n: GraphNode) => boolean): Graph {
  const keep = new Set(g.nodes.filter(match).map((n) => n.id));
  for (const e of g.edges) {
    if (keep.has(e.from) || keep.has(e.to)) {
      // Both ends of a link to a match come with it.
      keep.add(e.from);
      keep.add(e.to);
    }
  }
  return { nodes: g.nodes.filter((n) => keep.has(n.id)), edges: g.edges.filter((e) => keep.has(e.from) && keep.has(e.to)) };
}

// -- layout --------------------------------------------------------------------------------------------

export const COLUMN = 280;
export const ROW = 76;
export const NODE_W = 210;
export const NODE_H = 52;

export interface Placed {
  x: number;
  y: number;
  layer: number;
}

const TYPE_ORDER: Record<NodeType, number> = { proxy: 0, host: 1, tunnel: 2, database: 3 };

/** Columns by how far along a path a node is (a proxy first, then jump hosts, the host, what rides it), rows by name. */
export function layout(g: Graph): Map<string, Placed> {
  const layer = new Map(g.nodes.map((n) => [n.id, 0]));
  // Longest path, relaxed repeatedly; a cycle (a jump host loop) cannot push it past the number of nodes.
  for (let pass = 0; pass < g.nodes.length; pass++) {
    let changed = false;
    for (const e of g.edges) {
      const next = (layer.get(e.from) ?? 0) + 1;
      if (next > (layer.get(e.to) ?? 0) && next <= g.nodes.length) {
        layer.set(e.to, next);
        changed = true;
      }
    }
    if (!changed) break;
  }
  const byLayer = new Map<number, GraphNode[]>();
  for (const n of g.nodes) byLayer.set(layer.get(n.id)!, [...(byLayer.get(layer.get(n.id)!) ?? []), n]);
  const out = new Map<string, Placed>();
  for (const [l, list] of byLayer) {
    list.sort((a, b) => TYPE_ORDER[a.type] - TYPE_ORDER[b.type] || a.label.localeCompare(b.label));
    list.forEach((n, i) => out.set(n.id, { x: l * COLUMN, y: i * ROW, layer: l }));
  }
  return out;
}

/** The size of the drawing, for fitting it in the window. */
export function bounds(placed: Map<string, Placed>): { width: number; height: number } {
  let width = 0;
  let height = 0;
  for (const p of placed.values()) {
    width = Math.max(width, p.x + NODE_W);
    height = Math.max(height, p.y + NODE_H);
  }
  return { width, height };
}
