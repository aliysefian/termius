<script lang="ts">
  import { Copy, Download, Eye, FileCode2, Pencil, Plug, Plus, RefreshCw, Table2, Trash2, Unplug, Upload, FolderTree } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import type { ActionId, MenuItem } from "$lib/dbactions";

  let { items, x, y, label, onpick, onclose }: { items: MenuItem[]; x: number; y: number; label: string; onpick: (id: ActionId) => void; onclose: () => void } = $props();

  let menu = $state<HTMLDivElement>();
  const icon: Record<ActionId, typeof Copy> = {
    connect: Plug,
    disconnect: Unplug,
    "new-query": Plus,
    refresh: RefreshCw,
    edit: Pencil,
    delete: Trash2,
    "copy-name": Copy,
    "copy-qualified": Copy,
    "open-data": Table2,
    "open-structure": FolderTree,
    "open-ddl": FileCode2,
    "select-query": Eye,
    export: Download,
    import: Upload,
  };

  $effect(() => {
    menu?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
  });

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const list = [...(menu?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
    const at = list.indexOf(document.activeElement as HTMLElement);
    list[(at + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length]?.focus();
  }
</script>

<button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}></button>
<div bind:this={menu} class="fixed z-50 w-56 rounded-xl border border-line bg-panel p-1 shadow-xl" use:keepInView={{ x, y }} role="menu" aria-label="Actions for {label}" tabindex="-1" onkeydown={key}>
  {#each items as item, i (item.id)}
    {#if i > 0 && items[i - 1].group !== item.group}<div class="my-1 h-px bg-line" role="separator"></div>{/if}
    {@const Icon = icon[item.id]}
    <button
      class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none disabled:opacity-40 disabled:hover:bg-transparent {item.danger ? 'text-danger' : ''}"
      role="menuitem"
      disabled={!item.enabled}
      title={item.why}
      onclick={() => {
        onclose();
        onpick(item.id);
      }}
    >
      <Icon size={15} /> {item.label}
    </button>
  {/each}
</div>
