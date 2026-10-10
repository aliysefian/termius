// What the Fleet sidebar panel lists: the hosts matching a search and a status filter, one section per group,
// the groups in name order with the hosts that have none last. Pure, so it is tested.
import type { Host, VaultRecord } from "./types";

export type FleetFilter = "all" | "up" | "down";
export interface Reach {
  state: string;
}
export interface FleetSection {
  name: string;
  hosts: VaultRecord<Host>[];
  up: number;
  down: number;
}

export const UNGROUPED = "Ungrouped";

export function fleetSections(hosts: VaultRecord<Host>[], health: Record<string, Reach | undefined>, query: string, filter: FleetFilter): FleetSection[] {
  const q = query.trim().toLowerCase();
  const by = new Map<string, VaultRecord<Host>[]>();
  for (const rec of hosts) {
    const d = rec.data;
    if (!d) continue;
    const state = health[rec.id]?.state;
    if (filter === "up" && state !== "up") continue;
    if (filter === "down" && state !== "down") continue;
    const group = d.group?.trim() || UNGROUPED;
    // A search matches the host, its address, its tags, or its group (which then brings the whole group).
    if (q && ![d.label, d.hostname, group, ...(d.tags ?? [])].some((s) => s?.toLowerCase().includes(q))) continue;
    (by.get(group) ?? by.set(group, []).get(group)!).push(rec);
  }
  return [...by.entries()]
    .sort(([a], [b]) => (a === UNGROUPED ? 1 : b === UNGROUPED ? -1 : a.localeCompare(b)))
    .map(([name, list]) => ({
      name,
      hosts: list.sort((a, b) => a.data!.label.localeCompare(b.data!.label)),
      up: list.filter((h) => health[h.id]?.state === "up").length,
      down: list.filter((h) => health[h.id]?.state === "down").length,
    }));
}
