import type { Host, VaultRecord } from "./types";

export interface GroupNode {
  name: string;
  path: string;
  children: GroupNode[];
  hosts: VaultRecord<Host>[];
}

export type HostSort = "name" | "hostname" | "updated" | "lastused";
export type HostComparator = (a: VaultRecord<Host>, b: VaultRecord<Host>) => number;

const comparators: Record<HostSort, HostComparator> = {
  name: (a, b) => (a.data?.label ?? "").localeCompare(b.data?.label ?? ""),
  hostname: (a, b) => (a.data?.hostname ?? "").localeCompare(b.data?.hostname ?? ""),
  updated: (a, b) => b.updated_at - a.updated_at,
  // Needs per-computer usage data; callers pass a comparator for this one.
  lastused: (a, b) => (a.data?.label ?? "").localeCompare(b.data?.label ?? ""),
};

/** Every group path in the tree, including nested ones. */
export function groupPaths(node: GroupNode): string[] {
  return node.children.flatMap((c) => [c.path, ...groupPaths(c)]);
}

/** One row of a flattened tree, in the same order the tree renders them. */
export type TreeRow =
  | { kind: "group"; key: string; path: string; name: string; depth: number; parentKey: string | null }
  | { kind: "host"; key: string; id: string; depth: number; parentKey: string | null };

/**
 * Flattens a tree into the order it's drawn in, respecting which groups are
 * collapsed, so arrow-key navigation can move through exactly what's on
 * screen. Each child group (with its subtree, if expanded) comes before
 * this node's own hosts, matching HostTreeNode's render order.
 */
export function flattenTree(node: GroupNode, collapsed: ReadonlySet<string>, depth = 0, parentKey: string | null = null): TreeRow[] {
  const rows: TreeRow[] = [];
  for (const child of node.children) {
    const key = `g:${child.path}`;
    rows.push({ kind: "group", key, path: child.path, name: child.name, depth, parentKey });
    if (!collapsed.has(child.path)) rows.push(...flattenTree(child, collapsed, depth + 1, key));
  }
  for (const host of node.hosts) {
    rows.push({ kind: "host", key: `h:${host.id}`, id: host.id, depth, parentKey });
  }
  return rows;
}

/** Fold a flat host list into a tree keyed by each host's slash-separated group path. */
export function buildTree(hosts: VaultRecord<Host>[], sortBy: HostSort | HostComparator = "name"): GroupNode {
  const root: GroupNode = { name: "", path: "", children: [], hosts: [] };
  for (const h of hosts) {
    if (!h.data) continue;
    const parts = h.data.group.split("/").map((p) => p.trim()).filter(Boolean);
    let node = root;
    let path = "";
    for (const part of parts) {
      path = path ? `${path}/${part}` : part;
      let child = node.children.find((c) => c.name === part);
      if (!child) {
        child = { name: part, path, children: [], hosts: [] };
        node.children.push(child);
      }
      node = child;
    }
    node.hosts.push(h);
  }
  const sort = (n: GroupNode) => {
    n.children.sort((a, b) => a.name.localeCompare(b.name));
    n.hosts.sort(typeof sortBy === "function" ? sortBy : comparators[sortBy]);
    n.children.forEach(sort);
  };
  sort(root);
  return root;
}
