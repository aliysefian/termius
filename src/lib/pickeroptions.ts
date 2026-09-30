// Options for the searchable pickers (Combobox): hosts and group paths.
import type { ComboOption } from "./components/Combobox.svelte";
import { allGroupPaths } from "./popover";
import type { Host, HostGroup, VaultRecord } from "./types";

/** Hosts sorted by label; search also matches address, group and tags. */
export function hostOptions(hosts: VaultRecord<Host>[], keep: (h: VaultRecord<Host>) => boolean = () => true): ComboOption[] {
  return hosts
    .filter((h) => h.data && keep(h))
    .map((h) => ({
      value: h.id,
      label: h.data!.label,
      detail: h.data!.hostname,
      keywords: [h.data!.group, ...(h.data!.tags ?? [])].join(" "),
      color: h.data!.color,
    }))
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }));
}

/** Every existing group (saved or used by a host, plus parents). */
export function groupOptions(hosts: VaultRecord<Host>[], groups: VaultRecord<HostGroup>[]): ComboOption[] {
  const paths = allGroupPaths([...groups.map((g) => g.data?.path), ...hosts.map((h) => h.data?.group)]);
  const counts = new Map<string, number>();
  for (const h of hosts) {
    const g = h.data?.group?.trim();
    if (g) counts.set(g, (counts.get(g) ?? 0) + 1);
  }
  return paths.map((p) => {
    const n = counts.get(p) ?? 0;
    return { value: p, label: p, detail: n ? `${n} host${n === 1 ? "" : "s"}` : undefined };
  });
}
