<script lang="ts">
  import { Check, Columns3, FolderTree, List, Loader2, Pause, Play, RefreshCw, Search } from "lucide-svelte";
  import { COLUMNS, STATUS_FILTERS, type ColumnId, type StatusFilter } from "$lib/containerview";
  import { containers, REFRESH_CHOICES } from "$lib/stores/containers.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  let { sourceKey, query = $bindable(), status = $bindable(), counts, hasProjects }: { sourceKey: string; query: string; status: StatusFilter; counts: Record<StatusFilter, number>; hasProjects: boolean } = $props();

  const src = $derived(containers.sources[sourceKey]);
  let colsOpen = $state(false);
  let refreshOpen = $state(false);
  let wrap = $state<HTMLDivElement>();

  const chosen = $derived(settings.prefs.containerColumns as ColumnId[]);
  function toggleColumn(id: ColumnId) {
    settings.prefs.containerColumns = chosen.includes(id) ? chosen.filter((c) => c !== id) : [...chosen, id];
  }
  const updated = $derived(src?.updatedAt ? new Date(src.updatedAt).toLocaleTimeString() : "");

  function outside(e: PointerEvent) {
    if (wrap && !wrap.contains(e.target as Node)) colsOpen = refreshOpen = false;
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => e.key === "Escape" && (colsOpen || refreshOpen) && ((colsOpen = refreshOpen = false), e.stopPropagation())} />

<div class="flex flex-wrap items-center gap-2" bind:this={wrap}>
  <div class="relative min-w-[11rem] flex-1">
    <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" aria-hidden="true" />
    <input class="input h-9 py-0 pl-8 text-sm" placeholder="Search name, ID, image, project, port…" aria-label="Search containers" bind:value={query} />
  </div>

  <select class="input h-9 w-auto py-0 text-sm" aria-label="Filter by status" bind:value={status}>
    {#each STATUS_FILTERS as f (f.id)}<option value={f.id}>{f.label} ({counts[f.id]})</option>{/each}
  </select>

  {#if hasProjects}
    <div class="flex overflow-hidden rounded-lg border border-line text-sm" role="group" aria-label="Grouping">
      <button class="flex h-9 items-center gap-1.5 px-2.5 {settings.prefs.containerGrouped ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" aria-pressed={settings.prefs.containerGrouped} title="Group by Compose project" onclick={() => (settings.prefs.containerGrouped = true)}><FolderTree size={14} /> <span class="max-lg:hidden">Projects</span></button>
      <button class="flex h-9 items-center gap-1.5 border-l border-line px-2.5 {!settings.prefs.containerGrouped ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" aria-pressed={!settings.prefs.containerGrouped} title="Flat list" onclick={() => (settings.prefs.containerGrouped = false)}><List size={14} /> <span class="max-lg:hidden">Flat</span></button>
    </div>
  {/if}

  <div class="relative">
    <button class="icon-btn h-9 w-9 border border-line {colsOpen ? 'text-accent' : ''}" title="Columns and monitoring" aria-label="Columns and monitoring" aria-haspopup="menu" aria-expanded={colsOpen} onclick={() => ((colsOpen = !colsOpen), (refreshOpen = false))}><Columns3 size={16} /></button>
    {#if colsOpen}
      <div class="anim-pop absolute right-0 top-full z-40 mt-1 w-64 rounded-xl border border-line bg-panel p-1 shadow-xl" role="menu" aria-label="Columns">
        <p class="px-2 pb-1 pt-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Columns</p>
        {#each COLUMNS as col (col.id)}
          {@const needs = col.needs === "monitoring" ? !src?.wantStats : col.needs === "sizes" ? !src?.sizes : false}
          <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-panel-hover" role="menuitemcheckbox" aria-checked={chosen.includes(col.id)} onclick={() => toggleColumn(col.id)}>
            <span class="w-3.5">{#if chosen.includes(col.id)}<Check size={13} class="text-accent" />{/if}</span>
            <span class="flex-1">{col.label}</span>
            {#if needs}<span class="text-[11px] text-fg-muted">{col.needs === "monitoring" ? "needs monitoring" : "needs sizes"}</span>{/if}
          </button>
        {/each}
        <div class="my-1 h-px bg-line"></div>
        <p class="px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Read from the host</p>
        <label class="flex items-start gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-panel-hover" title="One more command with each refresh, about a second.">
          <input type="checkbox" class="mt-0.5 accent-input" checked={src?.wantStats} onchange={(e) => containers.setStats(sourceKey, e.currentTarget.checked)} />
          <span>CPU and memory<span class="block text-[11px] text-fg-muted">One more command per refresh</span></span>
        </label>
        <label class="flex items-start gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-panel-hover" title="Slower on hosts with many large containers.">
          <input type="checkbox" class="mt-0.5 accent-input" checked={src?.sizes} onchange={(e) => containers.setSizes(sourceKey, e.currentTarget.checked)} />
          <span>Sizes<span class="block text-[11px] text-fg-muted">Slower on big hosts</span></span>
        </label>
      </div>
    {/if}
  </div>

  <div class="relative flex items-center">
    <button class="icon-btn h-9 w-9 rounded-r-none border border-line" title="Refresh now{updated ? ` · updated ${updated}` : ''}" aria-label="Refresh now" disabled={src?.loading} onclick={() => containers.refresh(sourceKey)}>
      {#if src?.loading}<Loader2 size={16} class="animate-spin" />{:else}<RefreshCw size={16} />{/if}
    </button>
    <button class="flex h-9 items-center gap-1 rounded-r-lg border border-l-0 border-line px-2 text-xs text-fg-muted hover:bg-panel-hover" aria-haspopup="menu" aria-expanded={refreshOpen} aria-label="Automatic refresh: {containers.paused ? 'paused' : `every ${containers.intervalSecs} seconds`}" onclick={() => ((refreshOpen = !refreshOpen), (colsOpen = false))}>
      {#if containers.paused}<Pause size={12} /> paused{:else}{containers.intervalSecs}s{/if}
    </button>
    {#if refreshOpen}
      <div class="anim-pop absolute right-0 top-full z-40 mt-1 w-52 rounded-xl border border-line bg-panel p-1 shadow-xl" role="menu" aria-label="Automatic refresh">
        <p class="px-2 pb-1 pt-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Refresh every</p>
        {#each REFRESH_CHOICES as n (n)}
          <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-panel-hover" role="menuitemradio" aria-checked={!containers.paused && containers.intervalSecs === n} onclick={() => ((containers.intervalSecs = n), (containers.paused = false), (refreshOpen = false))}>
            <span class="w-3.5">{#if !containers.paused && containers.intervalSecs === n}<Check size={13} class="text-accent" />{/if}</span>{n} seconds
          </button>
        {/each}
        <div class="my-1 h-px bg-line"></div>
        <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-panel-hover" role="menuitem" onclick={() => ((containers.paused = !containers.paused), (refreshOpen = false))}>
          {#if containers.paused}<Play size={13} /> Resume refreshing{:else}<Pause size={13} /> Pause refreshing{/if}
        </button>
        {#if updated}<p class="px-2 py-1.5 text-[11px] text-fg-muted">Updated {updated}</p>{/if}
      </div>
    {/if}
  </div>
</div>
