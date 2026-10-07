<script lang="ts">
  import { ui } from "$lib/stores/ui.svelte";
  import type { GroupNode } from "$lib/tree";
  import GroupRow from "./GroupRow.svelte";
  import HostTreeNode from "./HostTreeNode.svelte";
  import HostRow from "./HostRow.svelte";

  let { node, depth, effectiveFocusedKey = null }: { node: GroupNode; depth: number; effectiveFocusedKey?: string | null } = $props();
</script>

{#each node.children as child (child.path)}
  {@const collapsed = !ui.search.trim() && ui.collapsedGroups.has(child.path)}
  <div>
    <GroupRow node={child} {depth} {collapsed} focused={`g:${child.path}` === effectiveFocusedKey} />
    {#if !collapsed}
      <HostTreeNode node={child} depth={depth + 1} {effectiveFocusedKey} />
    {/if}
  </div>
{/each}

{#each node.hosts as host (host.id)}
  <HostRow {host} {depth} treeKey="h:{host.id}" focused={`h:${host.id}` === effectiveFocusedKey} />
{/each}
