<script lang="ts">
  import { Loader2, PanelLeftOpen, PanelRightOpen, Plus, SquareCode, Table2, X } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import { databases, type QueryTab } from "$lib/stores/databases.svelte";
  import { hasPending } from "$lib/dbstaging";
  import { dbWorkspace } from "$lib/stores/dbworkspace.svelte";

  let {
    explorerHidden,
    inspectorHidden,
    onshowexplorer,
    onshowinspector,
    onnew,
  }: { explorerHidden: boolean; inspectorHidden: boolean; onshowexplorer: () => void; onshowinspector: () => void; onnew: () => void } = $props();

  let menu = $state<{ tab: QueryTab; x: number; y: number } | null>(null);

  function select(t: QueryTab) {
    databases.activeTabId = t.id;
    dbWorkspace.selectDoc(t.id);
  }

  function key(e: KeyboardEvent, i: number) {
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? databases.tabs.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = databases.tabs[(to + databases.tabs.length) % databases.tabs.length];
    select(next);
    queueMicrotask(() => document.getElementById(`doc-tab-${next.id}`)?.focus());
  }

  async function pick(action: "duplicate" | "save" | "close" | "others") {
    const t = menu!.tab;
    menu = null;
    if (action === "duplicate") await databases.duplicateTab(t.id);
    else if (action === "save") await databases.saveQuery(t.id);
    else if (action === "close") await databases.closeTab(t.id);
    else await databases.closeOthers(t.id);
  }
</script>

<div class="flex shrink-0 items-stretch border-b border-line bg-panel">
  {#if explorerHidden}
    <button class="icon-btn m-1 h-7 w-7 shrink-0" title="Show the explorer" aria-label="Show the explorer" onclick={onshowexplorer}><PanelLeftOpen size={15} /></button>
  {/if}
  <div class="flex min-w-0 flex-1 overflow-x-auto" role="tablist" aria-label="Open documents">
    {#each databases.tabs as t, i (t.id)}
      {@const prod = databases.isProduction(t.connId)}
      {@const dirty = databases.isDirty(t)}
      <div class="group flex shrink-0 items-center border-r border-line {t.id === databases.activeTabId ? 'bg-base' : 'hover:bg-panel-hover'} {prod ? 'border-t-2 border-t-danger' : 'border-t-2 border-t-transparent'}" oncontextmenu={(e) => { e.preventDefault(); menu = { tab: t, x: e.clientX, y: e.clientY }; }} role="presentation">
        <button
          id="doc-tab-{t.id}"
          role="tab"
          aria-selected={t.id === databases.activeTabId}
          tabindex={t.id === databases.activeTabId ? 0 : -1}
          class="flex max-w-56 items-center gap-1.5 px-3 py-2 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent"
          title="{t.title} on {databases.connName(t.connId)}{dirty ? ' · unsaved query' : ''}{hasPending(t.staging) ? ' · changes not saved to the database' : ''}"
          onclick={() => select(t)}
          onkeydown={(e) => key(e, i)}
          onauxclick={(e) => e.button === 1 && void databases.closeTab(t.id)}
        >
          {#if t.running}<Loader2 size={12} class="shrink-0 animate-spin" />{:else if t.table}<Table2 size={12} class="shrink-0 text-fg-muted" />{:else}<SquareCode size={12} class="shrink-0 text-fg-muted" />{/if}
          <span class="truncate">{t.title}</span>
          {#if dirty}<span class="h-1.5 w-1.5 shrink-0 rounded-full bg-warning" aria-label="Unsaved query"></span>{/if}
          {#if hasPending(t.staging)}<span class="h-1.5 w-1.5 shrink-0 rounded-full bg-danger" aria-label="Changes not saved to the database"></span>{/if}
        </button>
        <button class="icon-btn mr-1 h-5 w-5" title="Close (Ctrl+W)" aria-label="Close {t.title}" onclick={() => databases.closeTab(t.id)}><X size={12} /></button>
      </div>
    {/each}
    <button class="icon-btn m-1 h-7 w-7 shrink-0 self-center" title="New query" aria-label="New query" data-testid="db-new-tab" onclick={onnew}><Plus size={14} /></button>
  </div>
  {#if inspectorHidden}
    <button class="icon-btn m-1 h-7 w-7 shrink-0" title="Show the inspector" aria-label="Show the inspector" onclick={onshowinspector}><PanelRightOpen size={15} /></button>
  {/if}
</div>

{#if menu}
  {@const t = menu.tab}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
  <div class="fixed z-50 w-52 rounded-xl border border-line bg-panel p-1 text-sm shadow-xl" use:keepInView={{ x: menu.x, y: menu.y }} role="menu" aria-label="Tab actions">
    <button class="block w-full rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => pick("duplicate")}>Duplicate</button>
    <button class="block w-full rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40" role="menuitem" disabled={!!t.table || !t.sql.trim()} onclick={() => pick("save")}>Save query</button>
    <div class="my-1 h-px bg-line" role="separator"></div>
    <button class="block w-full rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => pick("close")}>Close</button>
    <button class="block w-full rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40" role="menuitem" disabled={databases.tabs.length < 2} onclick={() => pick("others")}>Close others</button>
  </div>
{/if}
