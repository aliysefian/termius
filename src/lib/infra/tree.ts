// The resource tree: saved hosts, and under each what was found, grouped by kind. Pure.
import type { Inventory, ResourceView, Filters } from "./inventory";
import { isFiltering, matches } from "./inventory";
import type { Freshness, ResourceType } from "./model";

export type RowKind = "host" | "group" | "resource" | "note";

export interface Row {
  /** Stable across refreshes: the resource id, or `<host id>#group-name`. */
  key: string;
  kind: RowKind;
  depth: number;
  label: string;
  /** For a resource row, the resource; for a group or note row, `null`. */
  resource: ResourceView | null;
  hostId: string | null;
  /** Whether it can be opened or closed, and whether it is. */
  expandable: boolean;
  open: boolean;
  /** How many resources are below a group. */
  count?: number;
  /** For a group: how its data stands. */
  freshness?: Freshness;
  /** What a group or note says, such as "Not read yet". */
  text?: string;
}

export const GROUPS = ["docker", "systemd", "tmux"] as const;
export type Group = (typeof GROUPS)[number];
const GROUP_LABEL: Record<Group, string> = { docker: "Docker", systemd: "systemd", tmux: "Tmux" };
const GROUP_TYPES: Record<Group, ResourceType[]> = { docker: ["docker_engine", "container"], systemd: ["systemd_service"], tmux: ["tmux_session"] };
const GROUP_SCOPE: Record<Group, string> = { docker: "docker:", systemd: "systemd:", tmux: "tmux:" };

export const groupKey = (hostId: string, g: Group) => `${hostId}#${g}`;

export interface TreeInput {
  inv: Inventory;
  views: ResourceView[];
  filters: Filters;
  envOf: (hostId: string | null) => string;
  expanded: Set<string>;
  now: number;
}

/** The rows to draw. With a search or a filter, what does not match is hidden and what is above a match is open. */
export function buildRows({ inv, views, filters, envOf, expanded, now }: TreeInput): Row[] {
  const filtering = isFiltering(filters);
  const hosts = views.filter((r) => r.type === "host").sort((a, b) => a.name.localeCompare(b.name));
  const byHost = new Map<string, ResourceView[]>();
  for (const r of views) {
    if (r.type === "host" || r.hostId === null) continue;
    (byHost.get(r.hostId) ?? byHost.set(r.hostId, []).get(r.hostId)!).push(r);
  }
  const rows: Row[] = [];
  for (const h of hosts) {
    const hostId = h.hostId!;
    const below = byHost.get(hostId) ?? [];
    const hostMatches = matches(h, filters, envOf);
    const hit = (r: ResourceView) => matches(r, filters, envOf);
    // Ports are not shown in the tree (they are the container's, drawn in its details and in the graph).
    const kids = below.filter((r) => r.type !== "port");
    const anyChildMatch = filtering && kids.some(hit);
    if (filtering && !hostMatches && !anyChildMatch) continue;
    // A host that matched on its own opens only if the person opened it; one shown because of what is below opens.
    const open = filtering ? anyChildMatch || expanded.has(h.id) : expanded.has(h.id);
    rows.push({ key: h.id, kind: "host", depth: 0, label: h.name, resource: h, hostId, expandable: true, open });
    if (!open) continue;
    for (const g of GROUPS) {
      const items = kids.filter((r) => GROUP_TYPES[g].includes(r.type));
      const shown = filtering ? items.filter(hit) : items;
      const scopes = inv.scopesOf(hostId).filter((s) => s.scopeKey.startsWith(GROUP_SCOPE[g]));
      if (filtering && shown.length === 0) continue;
      const gk = groupKey(hostId, g);
      const gOpen = filtering ? true : expanded.has(gk);
      const unsupported = scopes.find((s) => s.unsupported)?.unsupported;
      const failed = scopes.find((s) => s.lastError !== null);
      const freshness: Freshness | undefined = unsupported ? "unsupported" : scopes.length ? (scopes.some((s) => s.lastError !== null) ? (scopes.every((s) => s.lastSuccessAt === null) ? "unavailable" : "stale") : items[0]?.freshness ?? (scopes[0].lastSuccessAt === null ? "unavailable" : "live")) : undefined;
      rows.push({
        key: gk,
        kind: "group",
        depth: 1,
        label: GROUP_LABEL[g],
        resource: null,
        hostId,
        expandable: true,
        open: gOpen,
        count: g === "docker" ? items.filter((r) => r.type === "container").length : items.length,
        freshness,
        text: unsupported ?? (failed ? `Could not read: ${failed.lastError}` : scopes.length ? undefined : "Not read yet"),
      });
      if (!gOpen) continue;
      if (g === "docker") {
        // The engine is the parent of its containers; two runtimes on a host are two engines.
        for (const e of shown.filter((r) => r.type === "docker_engine")) {
          rows.push({ key: e.id, kind: "resource", depth: 2, label: e.name, resource: e, hostId, expandable: false, open: false });
        }
        for (const c of shown.filter((r) => r.type === "container").sort((a, b) => a.name.localeCompare(b.name))) {
          rows.push({ key: c.id, kind: "resource", depth: 3, label: c.name, resource: c, hostId, expandable: false, open: false });
        }
      } else {
        for (const r of shown.sort((a, b) => a.name.localeCompare(b.name))) {
          rows.push({ key: r.id, kind: "resource", depth: 2, label: r.name, resource: r, hostId, expandable: false, open: false });
        }
      }
      if (items.length === 0 && scopes.length && !unsupported && !failed) rows.push({ key: `${gk}#none`, kind: "note", depth: 2, label: "", resource: null, hostId, expandable: false, open: false, text: "None found" });
    }
  }
  void now;
  return rows;
}
