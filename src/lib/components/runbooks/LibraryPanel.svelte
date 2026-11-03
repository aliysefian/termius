<script lang="ts">
  import { ChevronDown, ChevronRight, ListChecks, PanelLeftClose, Plus, Search, Star, Upload, X } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import { ago, favorites as favoriteItems, groupByCategory, recent, searchLibrary, type LibraryItem } from "$lib/runbooklib";
  import { choose } from "$lib/dialogs.svelte";
  import { categoryChoices } from "$lib/runbooklib";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let { oncollapse, ontemplates }: { oncollapse: () => void; ontemplates: () => void } = $props();

  let query = $state("");
  let collapsed = $state<Record<string, boolean>>({});
  let menu = $state<{ item: LibraryItem; x: number; y: number } | null>(null);

  const all = $derived(rb.items);
  const found = $derived(searchLibrary(all, query));
  const searching = $derived(query.trim() !== "");
  const groups = $derived(groupByCategory(found));
  const favs = $derived(favoriteItems(found));
  const recents = $derived(searching ? [] : recent(all, 3));

  function context(e: MouseEvent, item: LibraryItem) {
    e.preventDefault();
    menu = { item, x: e.clientX, y: e.clientY };
  }

  async function pickCategory(item: LibraryItem) {
    const names = categoryChoices(all);
    const picked = await choose(`Category of "${item.name}"`, names.map((n) => ({ value: n, label: n })), { title: "Change category" });
    if (typeof picked === "string") await rb.setCategory(item.id, picked);
  }

  function key(e: KeyboardEvent, item: LibraryItem) {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      menu = { item, x: r.left + 24, y: r.bottom };
    }
  }
</script>

{#snippet row(item: LibraryItem)}
  <div class="group flex items-center gap-1 pr-1.5 {rb.selectedId === item.id ? 'bg-accent/15' : 'hover:bg-panel-hover'}" role="presentation" oncontextmenu={(e) => context(e, item)}>
    <button class="flex min-w-0 flex-1 items-start gap-2 px-3 py-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent" aria-current={rb.selectedId === item.id ? "true" : undefined} onclick={() => rb.select(item.id).then((ok) => ok && (rb.tab = "runbooks"))} onkeydown={(e) => key(e, item)} title={item.description || item.name} data-testid="library-row">
      <ListChecks size={14} class="mt-0.5 shrink-0 {item.broken ? 'text-danger' : 'text-accent'}" />
      <span class="min-w-0 flex-1">
        <span class="block truncate text-sm {rb.selectedId === item.id ? 'font-medium' : ''}">{item.name}</span>
        <span class="block truncate text-[11px] text-fg-muted">{item.broken ? "Can't be read: open it to fix" : item.description || `${item.steps} step${item.steps === 1 ? "" : "s"}`} · {ago(item.updatedAt)}</span>
      </span>
    </button>
    <button class="icon-btn h-6 w-6 shrink-0 {item.favorite ? '' : 'opacity-0 focus:opacity-100 group-hover:opacity-100'}" title={item.favorite ? "Remove from favorites" : "Add to favorites"} aria-label={item.favorite ? `Remove ${item.name} from favorites` : `Add ${item.name} to favorites`} aria-pressed={item.favorite} onclick={() => rb.favorite(item.id)}>
      <Star size={13} class={item.favorite ? "fill-warning text-warning" : ""} />
    </button>
  </div>
{/snippet}

<div class="flex h-full min-h-0 flex-col bg-panel" data-testid="library">
  <header class="space-y-2.5 border-b border-line px-3 pb-2.5 pt-3">
    <div class="flex items-center gap-2">
      <h2 class="flex-1 text-sm font-semibold">Runbooks</h2>
      <button class="icon-btn h-7 w-7" title="New runbook" aria-label="New runbook" onclick={() => rb.newBlank()} data-testid="new-runbook"><Plus size={15} /></button>
      <button class="icon-btn h-7 w-7" title="Hide the library" aria-label="Hide the library" onclick={oncollapse}><PanelLeftClose size={15} /></button>
    </div>
    <div class="relative">
      <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input h-8 w-full pl-8 pr-7 text-xs" type="search" placeholder="Search runbooks" aria-label="Search runbooks" bind:value={query} />
      {#if query}<button class="icon-btn absolute right-1 top-1/2 h-6 w-6 -translate-y-1/2" aria-label="Clear the search" onclick={() => (query = "")}><X size={12} /></button>{/if}
    </div>
    <div class="flex gap-1.5">
      <button class="btn-secondary h-7 flex-1 justify-center px-2 text-xs" onclick={ontemplates}>Templates</button>
      <button class="btn-secondary h-7 flex-1 justify-center px-2 text-xs" onclick={() => rb.importFile()}><Upload size={12} /> Import</button>
    </div>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto py-1">
    {#if all.length === 0}
      <div class="px-4 py-8 text-center">
        <p class="text-sm font-medium">Create your first automation</p>
        <p class="mt-1 text-xs text-fg-muted">Nothing is saved yet.</p>
        <div class="mt-3 flex flex-col gap-1.5">
          <button class="btn-primary justify-center text-xs" onclick={() => rb.newBlank()}>Create blank runbook</button>
          <button class="btn-secondary justify-center text-xs" onclick={ontemplates}>Browse templates</button>
          <button class="btn-secondary justify-center text-xs" onclick={() => rb.importFile()}>Import YAML</button>
        </div>
      </div>
    {:else if found.length === 0}
      <p class="px-4 py-6 text-center text-xs text-fg-muted">No runbook matches "{query}".</p>
    {:else}
      {#if favs.length}
        <section aria-label="Favorites">
          <h3 class="px-3 pb-0.5 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Favorites</h3>
          {#each favs as item (item.id)}{@render row(item)}{/each}
        </section>
      {/if}
      {#if recents.length}
        <section aria-label="Recently changed">
          <h3 class="px-3 pb-0.5 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Recently changed</h3>
          {#each recents as item (item.id)}{@render row(item)}{/each}
        </section>
      {/if}
      {#each groups as g (g.name)}
        <section aria-label={g.name}>
          <button class="flex w-full items-center gap-1 px-3 pb-0.5 pt-2.5 text-left text-[11px] font-medium uppercase tracking-wide text-fg-muted hover:text-fg" aria-expanded={!collapsed[g.name] || searching} onclick={() => (collapsed[g.name] = !collapsed[g.name])}>
            {#if collapsed[g.name] && !searching}<ChevronRight size={12} />{:else}<ChevronDown size={12} />{/if}{g.name}<span class="font-normal normal-case tracking-normal text-fg-muted/70">({g.items.length})</span>
          </button>
          {#if !collapsed[g.name] || searching}{#each g.items as item (item.id)}{@render row(item)}{/each}{/if}
        </section>
      {/each}
    {/if}
  </div>
  <footer class="border-t border-line px-3 py-1.5 text-[11px] text-fg-muted">{all.length} runbook{all.length === 1 ? "" : "s"}</footer>
</div>

{#if menu}
  {@const it = menu.item}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
  <div class="fixed z-50 w-52 rounded-xl border border-line bg-panel p-1 text-sm shadow-xl" use:keepInView={menu} role="menu" aria-label="Actions for {it.name}">
    {#snippet entry(label: string, run: () => void, danger = false)}
      <button class="block w-full rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover {danger ? 'text-danger' : ''}" role="menuitem" onclick={() => { menu = null; run(); }}>{label}</button>
    {/snippet}
    {@render entry("Open", () => void rb.select(it.id).then((ok) => ok && (rb.tab = "runbooks")))}
    {@render entry("Rename…", () => void rb.rename(it.id))}
    {@render entry("Duplicate", () => void rb.duplicate(it.id))}
    {@render entry(it.favorite ? "Remove from favorites" : "Add to favorites", () => rb.favorite(it.id))}
    {@render entry("Change category…", () => void pickCategory(it))}
    {@render entry("Export…", () => void rb.exportFile(it.id))}
    <div class="my-1 h-px bg-line" role="separator"></div>
    {@render entry("Delete…", () => void rb.remove(it.id), true)}
  </div>
{/if}
