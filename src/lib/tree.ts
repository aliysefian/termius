import type { Host, VaultRecord } from "./types";

export interface GroupNode {
  name: string;
  path: string;
  children: GroupNode[];
  hosts: VaultRecord<Host>[];
}

/** Fold a flat host list into a tree keyed by each host's slash-separated group path. */
export function buildTree(hosts: VaultRecord<Host>[]): GroupNode {
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
    n.hosts.sort((a, b) => (a.data?.label ?? "").localeCompare(b.data?.label ?? ""));
    n.children.forEach(sort);
  };
  sort(root);
  return root;
}
