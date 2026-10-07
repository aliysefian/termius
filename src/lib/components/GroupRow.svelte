<script lang="ts">
  import { Activity, ChevronDown, ChevronRight, Folder, FolderOpen, LayoutGrid, ListPlus, Plus } from "lucide-svelte";
  import { MAX_PANES } from "$lib/layout";
  import { ui } from "$lib/stores/ui.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { acceptsHost, dropHostInto } from "$lib/hostdrag.svelte";
  import type { GroupNode } from "$lib/tree";

  // One group's row in the host list: its name and count, the buttons that appear on hover, and the drop target
  // for hosts dragged onto it. Used by the nested list and by the windowed list for very large vaults.
  let { node, depth, collapsed, focused }: { node: GroupNode; depth: number; collapsed: boolean; focused: boolean } = $props();
  const indent = $derived(`${depth * 12 + 8}px`);
  const treeKey = $derived(`g:${node.path}`);
  let dropping = $state(false);

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
</script>

<div
  class="group relative flex items-center gap-1.5 rounded-md py-1 pr-1 text-sm text-fg-muted hover:bg-panel-hover {dropping ? 'bg-accent/15 ring-1 ring-accent' : ''}"
  style:padding-left={indent}
  role="treeitem"
  aria-expanded={!collapsed}
  aria-selected={false}
  aria-level={depth + 1}
  data-tree-key={treeKey}
  tabindex={focused ? 0 : -1}
  ondragover={(e) => {
    if (acceptsHost(e)) {
      e.preventDefault();
      dropping = true;
    }
  }}
  ondragleave={() => (dropping = false)}
  ondrop={(e) => {
    dropping = false;
    void dropHostInto(e, node.path);
  }}
>
  <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" tabindex="-1" onclick={() => ui.toggleGroup(node.path)}>
    {#if collapsed}<ChevronRight size={14} class="shrink-0" />{:else}<ChevronDown size={14} class="shrink-0" />{/if}
    {#if collapsed}<Folder size={14} class="shrink-0 text-accent" />{:else}<FolderOpen size={14} class="shrink-0 text-accent" />{/if}
    <span class="truncate font-medium text-fg">{node.name}</span>
    <span class="shrink-0 text-xs">{node.hosts.length + node.children.length}</span>
  </button>
  <div class="absolute inset-y-0 right-1 my-auto hidden h-7 items-center rounded-md bg-panel-hover pl-1 group-hover:flex group-focus-within:flex">
    <button
      class="icon-btn h-6 w-6"
      title="Open every host in {node.name}, one tab each"
      onclick={() => openGroup(node, "tabs")}
    >
      <ListPlus size={12} />
    </button>
    <button
      class="icon-btn h-6 w-6"
      title="Open {node.name} tiled in one tab, typing into all (up to {MAX_PANES})"
      onclick={() => openGroup(node, "tiled")}
    >
      <LayoutGrid size={12} />
    </button>
    <button
      class="icon-btn h-6 w-6"
      title="View {node.name} in the Fleet view"
      onclick={() => { ui.view = "fleet"; ui.fleetGroup = node.path; }}
    >
      <Activity size={12} />
    </button>
    <button
      class="icon-btn h-6 w-6"
      title="New host in {node.name}"
      onclick={() => (ui.modal = { kind: "host", id: null, group: node.path })}
    >
      <Plus size={12} />
    </button>
  </div>
</div>
