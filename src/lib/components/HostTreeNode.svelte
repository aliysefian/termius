<script lang="ts">
  import { ChevronDown, ChevronRight, Folder, FolderOpen, LayoutGrid, ListPlus, Plus } from "lucide-svelte";
  import { MAX_PANES } from "$lib/layout";
  import { ui } from "$lib/stores/ui.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { acceptsHost, dropHostInto } from "$lib/hostdrag.svelte";
  import type { GroupNode } from "$lib/tree";
  import HostTreeNode from "./HostTreeNode.svelte";
  import HostRow from "./HostRow.svelte";

  let { node, depth }: { node: GroupNode; depth: number } = $props();
  const indent = $derived(`${depth * 12 + 8}px`);

  function allHosts(n: GroupNode): GroupNode["hosts"] {
    return [...n.hosts, ...n.children.flatMap(allHosts)];
  }

  async function openGroup(n: GroupNode, mode: "tabs" | "tiled") {
    const hosts = allHosts(n).map((h) => ({ id: h.id, label: h.data?.label ?? "", env: h.data?.environment }));
    if (hosts.length === 0) return;
    if (mode === "tabs" && hosts.length > 10 && !await ask(`Open ${hosts.length} tabs?`)) return;
    if (mode === "tiled") {
      if (hosts.length > MAX_PANES) ui.notify("info", `Tiling the first ${MAX_PANES} of ${hosts.length} hosts.`);
      const prod = hosts.slice(0, MAX_PANES).filter((h) => h.env === "production");
      if (prod.length && !await ask(`${prod.length} production host(s) will receive everything you type:\n\n${prod.map((h) => h.label).join("\n")}\n\nContinue?`)) return;
    }
    ui.openMany(hosts, mode, n.name);
  }
  let dropTarget = $state<string | null>(null);
</script>

{#each node.children as child (child.path)}
  {@const collapsed = ui.collapsedGroups.has(child.path)}
  <div>
    <div
      class="group flex items-center gap-1.5 rounded-md py-1 pr-1 text-sm text-fg-muted hover:bg-panel-hover {dropTarget === child.path ? 'bg-accent/15 ring-1 ring-accent' : ''}"
      style:padding-left={indent}
      role="group"
      ondragover={(e) => {
        if (acceptsHost(e)) {
          e.preventDefault();
          dropTarget = child.path;
        }
      }}
      ondragleave={() => (dropTarget = null)}
      ondrop={(e) => {
        dropTarget = null;
        void dropHostInto(e, child.path);
      }}
    >
      <button class="flex flex-1 items-center gap-1.5 text-left" onclick={() => ui.toggleGroup(child.path)}>
        {#if collapsed}<ChevronRight size={14} />{:else}<ChevronDown size={14} />{/if}
        {#if collapsed}<Folder size={14} class="text-accent" />{:else}<FolderOpen size={14} class="text-accent" />{/if}
        <span class="truncate font-medium text-fg">{child.name}</span>
        <span class="text-xs">{child.hosts.length + child.children.length}</span>
      </button>
      <button
        class="icon-btn h-6 w-6 opacity-0 group-hover:opacity-100"
        title="Open every host in {child.name}, one tab each"
        onclick={() => openGroup(child, "tabs")}
      >
        <ListPlus size={12} />
      </button>
      <button
        class="icon-btn h-6 w-6 opacity-0 group-hover:opacity-100"
        title="Open {child.name} tiled in one tab, typing into all (up to {MAX_PANES})"
        onclick={() => openGroup(child, "tiled")}
      >
        <LayoutGrid size={12} />
      </button>
      <button
        class="icon-btn h-6 w-6 opacity-0 group-hover:opacity-100"
        title="New host in {child.name}"
        onclick={() => (ui.modal = { kind: "host", id: null, group: child.path })}
      >
        <Plus size={12} />
      </button>
    </div>
    {#if !collapsed}
      <HostTreeNode node={child} depth={depth + 1} />
    {/if}
  </div>
{/each}

{#each node.hosts as host (host.id)}
  <HostRow {host} {depth} />
{/each}
