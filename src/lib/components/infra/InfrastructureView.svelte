<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { AlertTriangle, Boxes, ChevronRight, List, Loader2, Network, PanelLeftClose, PanelLeftOpen, PanelRightClose, PanelRightOpen, RefreshCw, Search, Square } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import RelationshipGraph from "./RelationshipGraph.svelte";
  import DiscoveryStatus from "./DiscoveryStatus.svelte";
  import ResourceInspector from "./ResourceInspector.svelte";
  import ResourceSummary from "./ResourceSummary.svelte";
  import ResourceToolbar from "./ResourceToolbar.svelte";
  import ResourceTree from "./ResourceTree.svelte";
  import { ageText, statusLine } from "$lib/infra/inventory";
  import { TYPE_LABEL, type ResourceType } from "$lib/infra/model";
  import { infra } from "$lib/stores/infra.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { FRESH_LABEL, FRESH_TONE, STATUS_TONE, TYPE_ICON, show } from "./style";

  // The saved hosts and their connections follow the vault and the panes on their own; nothing here asks a host.
  $effect(() => infra.syncHosts());
  $effect(() => infra.syncContainers());
  onMount(() => infra.start());
  onDestroy(() => infra.stop());

  let width = $state(typeof window === "undefined" ? 1400 : window.innerWidth);
  let height = $state(typeof window === "undefined" ? 900 : window.innerHeight);
  // The window's size drives the layout: three panels when wide, a drawer for the inspector when not.
  const narrow = $derived(width < 1100);
  const compactFilters = $derived(width < 1280);
  const short = $derived(height < 820);
  let treeW = $state(300);
  let inspectorW = $state(320);
  let treeOpen = $state(true);
  let inspectorOpen = $state(true);
  let drawer = $state(false);
  let query = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Typing is searched locally, a moment after the last key; no host is asked per keystroke. */
  function typed() {
    clearTimeout(timer);
    timer = setTimeout(() => (infra.filters.query = query), 150);
  }
  function clearFilters() {
    query = "";
    infra.filters = { query: "", types: [], statuses: [], environment: "", hostId: null };
  }
  function drag(set: (w: number) => void, from: number, dir: 1 | -1) {
    return (e: PointerEvent) => {
      e.preventDefault();
      const x0 = e.clientX;
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);
      const move = (m: PointerEvent) => set(Math.min(480, Math.max(240, from + dir * (m.clientX - x0))));
      const up = () => {
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
    };
  }

  const sel = $derived(infra.selected);
  const tabs = $derived([
    { id: "overview" as const, label: "Overview", show: true },
    { id: "relationships" as const, label: "Relationships", show: !!sel && (infra.parents.length > 0 || infra.children.length > 0) },
    { id: "metrics" as const, label: "Metrics", show: !!infra.metrics },
  ].filter((t) => t.show));
  const tab = $derived(tabs.some((t) => t.id === infra.tab) ? infra.tab : "overview");

  /** Related resources shown as groups of a bounded size; the rest are behind "Show more", never rendered all at once. */
  const PAGE = 25;
  let more = $state<Record<string, number>>({});
  const nodes = $derived(infra.relatedType ? infra.children.filter((c) => c.type === infra.relatedType) : infra.children);
  const grouped = $derived.by(() => {
    const by = new Map<ResourceType, typeof nodes>();
    for (const n of nodes) by.set(n.type, [...(by.get(n.type) ?? []), n]);
    return [...by];
  });
  const allProps = $derived(sel ? Object.entries(sel.attributes) : []);
</script>

<svelte:window bind:innerWidth={width} bind:innerHeight={height} />

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" data-testid="infra-view">
  <header class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line px-4 py-2.5">
    <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-panel text-accent"><Boxes size={18} /></span>
    <div class="min-w-0">
      <h1 class="text-lg font-semibold leading-tight">Infrastructure Explorer</h1>
      <p class="truncate text-xs text-fg-muted">Discover and manage resources across your hosts.</p>
    </div>
    <div class="ml-auto flex min-w-0 flex-wrap items-center gap-2">
      <div class="relative w-64 max-w-full xl:w-80">
        <Search size={14} class="pointer-events-none absolute left-2.5 top-[9px] text-fg-muted" />
        <input class="input h-8 w-full pl-8 text-xs" placeholder="Filter resources by name, host, image…" aria-label="Search resources" title="Searches the resources already read; the global search is in the title bar" bind:value={query} oninput={typed} />
      </div>
      <DiscoveryStatus />
      {#if infra.discovering}
        <button class="btn-secondary h-8 py-0 text-xs" onclick={() => infra.cancel()}><Square size={12} /> Stop</button>
      {/if}
      <button class="btn-secondary h-8 py-0 text-xs" onclick={() => infra.refresh()} disabled={infra.discovering || (!!sel?.hostId && !!infra.busy[sel.hostId])} title={sel?.hostId ? `Read ${infra.hostLabel(sel.hostId)} again` : "Read every saved SSH host, a few at a time"}>
        {#if infra.discovering || (sel?.hostId && infra.busy[sel.hostId])}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}
        {sel?.hostId ? "Refresh host" : "Discover all"}
      </button>
    </div>
  </header>

  <div class="space-y-3 border-b border-line px-4 py-3">
    {#if !short}<ResourceSummary />{/if}
    <ResourceToolbar compact={compactFilters} onclear={clearFilters} />
    {#if infra.coverage.read < infra.coverage.total}
      <p class="text-[11px] text-fg-muted" title="Search and counts cover what has been read; nothing is asked of a host while you type">Searching {infra.coverage.read} of {infra.coverage.total} hosts read. Open a host or Discover all to read more.</p>
    {/if}
  </div>

  <div class="relative flex min-h-0 min-w-0 flex-1" data-testid="infra-content">
    {#if treeOpen}
      <section class="flex min-h-0 min-w-0 shrink-0 flex-col border-r border-line" style="width: {narrow ? 260 : treeW}px" aria-label="Resource explorer">
        <div class="flex h-9 items-center justify-between border-b border-line px-3">
          <h2 class="text-xs font-semibold">Resources</h2>
          <button class="icon-btn h-7 w-7" aria-label="Collapse the explorer" title="Collapse the explorer" onclick={() => (treeOpen = false)}><PanelLeftClose size={14} /></button>
        </div>
        <ResourceTree />
      </section>
      {#if !narrow}<div class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40" role="separator" aria-orientation="vertical" aria-label="Resize the explorer" onpointerdown={drag((w) => (treeW = w), treeW, 1)}></div>{/if}
    {/if}

    <main class="flex min-h-0 min-w-0 flex-1 flex-col">
      <div class="flex h-9 shrink-0 items-center gap-1 border-b border-line px-2" role="tablist" aria-label="Resource workspace">
        {#if !treeOpen}<button class="icon-btn h-7 w-7" aria-label="Show the explorer" title="Show the explorer" onclick={() => (treeOpen = true)}><PanelLeftOpen size={14} /></button>{/if}
        {#each tabs as t (t.id)}
          {#if sel}<button role="tab" aria-selected={tab === t.id} class="h-9 border-b-2 px-3 text-xs {tab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}" onclick={() => (infra.tab = t.id)}>{t.label}</button>{/if}
        {/each}
        <div class="ml-auto flex items-center gap-1">
          {#if tab === "relationships" && sel}
            <button class="icon-btn h-7 w-7 {infra.view === 'graph' ? 'text-accent' : ''}" title="Graph" aria-label="Graph view" aria-pressed={infra.view === "graph"} onclick={() => (infra.view = "graph")}><Network size={14} /></button>
            <button class="icon-btn h-7 w-7 {infra.view === 'list' ? 'text-accent' : ''}" title="List" aria-label="List view" aria-pressed={infra.view === "list"} onclick={() => (infra.view = "list")}><List size={14} /></button>
          {/if}
          {#if narrow}
            <button class="btn-secondary h-7 py-0 text-xs" aria-expanded={drawer} onclick={() => (drawer = !drawer)}>Details</button>
          {:else}
            <button class="icon-btn h-7 w-7" aria-label={inspectorOpen ? "Collapse the inspector" : "Show the inspector"} title={inspectorOpen ? "Collapse the inspector" : "Show the inspector"} onclick={() => (inspectorOpen = !inspectorOpen)}>{#if inspectorOpen}<PanelRightClose size={14} />{:else}<PanelRightOpen size={14} />{/if}</button>
          {/if}
        </div>
      </div>

      {#if !sel}
        <div class="m-auto"><EmptyState icon={TYPE_ICON.host} text={infra.hostIds.length ? "Open a host in the list, or search, then select a resource." : "No saved SSH hosts yet."} /></div>
      {:else}
        <div class="min-h-0 flex-1 overflow-auto p-4 text-xs" role="tabpanel">
          {#if tab === "overview"}
            <div class="space-y-4">
              <div class="rounded-lg border border-line bg-panel p-4">
                <div class="flex items-center gap-2"><span class="h-2.5 w-2.5 rounded-full {STATUS_TONE[sel.status]}"></span><span class="text-[1rem] font-semibold">{sel.name}</span></div>
                <p class="mt-1 text-fg-muted">{statusLine(sel, infra.now)}</p>
                <p class="{FRESH_TONE[sel.freshness]}">{FRESH_LABEL[sel.freshness]}{sel.observedAt ? ` · read ${ageText(sel.observedAt, infra.now)}` : ""}</p>
              </div>
              {#if sel.freshness === "stale" || sel.freshness === "unavailable"}
                <p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-warning"><AlertTriangle size={13} class="mt-0.5 shrink-0" />This is the last known state, not what the host reports now. Refresh the host to read it again.</p>
              {/if}
              {#if infra.relatedGroups.length}
                <div class="grid grid-cols-2 gap-3 xl:grid-cols-4">
                  {#each infra.relatedGroups as g (g.type)}
                    {@const GI = TYPE_ICON[g.type]}
                    <button class="flex items-center gap-3 rounded-lg border border-line bg-panel px-3 py-2.5 text-left hover:bg-panel-hover" onclick={() => infra.openRelated(g.type)}>
                      <GI size={16} class="shrink-0 text-fg-muted" /><span class="min-w-0 flex-1"><span class="block text-lg font-semibold leading-tight tabular-nums">{g.count}</span><span class="block truncate text-fg-muted">{TYPE_LABEL[g.type]}{g.count === 1 ? "" : "s"}</span></span><ChevronRight size={14} class="shrink-0 text-fg-muted" />
                    </button>
                  {/each}
                </div>
              {/if}
              <section aria-labelledby="ws-props">
                <h2 id="ws-props" class="mb-2 text-sm font-semibold">Properties</h2>
                <dl class="grid grid-cols-[9rem_minmax(0,1fr)] gap-x-4 gap-y-2 rounded-lg border border-line bg-panel p-4 2xl:grid-cols-[9rem_minmax(0,1fr)_9rem_minmax(0,1fr)]">
                  {#each allProps as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="min-w-0 break-words">{show(k, v)}</dd>{/each}
                </dl>
              </section>
            </div>
          {:else if tab === "relationships"}
            {#if infra.view === "graph" && infra.graph}
              <div class="h-[min(36rem,calc(100%-2rem))] min-h-64"><RelationshipGraph graph={infra.graph} /></div>
              <p class="mt-2 text-[11px] text-fg-muted">Lines are only what the owning module reported: a host and its engine, an engine and its containers, a container and the ports it publishes. Similar names are not connected.</p>
            {:else}
              <div class="space-y-4">
                {#if infra.relatedType}
                  <p class="flex items-center gap-2 text-fg-muted">Showing {TYPE_LABEL[infra.relatedType]}s of {sel.name}.<button class="text-accent hover:underline" onclick={() => (infra.relatedType = null)}>Show all kinds</button></p>
                {/if}
                {#if infra.parents.length}
                  <section><h2 class="mb-1 text-sm font-semibold">Runs it</h2>
                    <ul>{#each infra.parents as n (n.id)}{@const I = TYPE_ICON[n.type]}<li><button class="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-panel-hover" onclick={() => infra.select(n.id)}><I size={13} class="text-fg-muted" /><span class="truncate">{n.name}</span><span class="text-fg-muted">· {TYPE_LABEL[n.type]}</span></button></li>{/each}</ul>
                  </section>
                {/if}
                {#each grouped as [type, list] (type)}
                  {@const I = TYPE_ICON[type]}
                  {@const shown = PAGE + (more[type] ?? 0)}
                  <section>
                    <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><I size={14} class="text-fg-muted" />{TYPE_LABEL[type]}s <span class="font-normal tabular-nums text-fg-muted">{list.length}</span></h2>
                    <ul>
                      {#each list.slice(0, shown) as n (n.id)}
                        <li><button class="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-panel-hover" onclick={() => infra.select(n.id)}><span class="h-2 w-2 shrink-0 rounded-full {STATUS_TONE[n.status]}"></span><span class="truncate">{n.name}</span><span class="ml-auto shrink-0 text-fg-muted">{n.statusText}</span></button></li>
                      {/each}
                    </ul>
                    {#if list.length > shown}<button class="mt-1 px-2 text-accent hover:underline" onclick={() => (more[type] = (more[type] ?? 0) + PAGE * 4)}>Show {Math.min(PAGE * 4, list.length - shown)} more of {list.length - shown}</button>{/if}
                  </section>
                {/each}
              </div>
            {/if}
          {:else if tab === "metrics" && infra.metrics}
            {@const m = infra.metrics}
            <div class="grid max-w-xl grid-cols-2 gap-3">
              <div class="rounded-lg border border-line bg-panel p-4"><div class="text-fg-muted">{m.kind === "host" ? "Host CPU" : "Container CPU"}</div><div class="text-2xl font-semibold tabular-nums">{m.cpu === null ? "—" : `${m.cpu.toFixed(1)} %`}</div></div>
              <div class="rounded-lg border border-line bg-panel p-4"><div class="text-fg-muted">{m.kind === "host" ? "Host memory" : "Container memory"}</div><div class="text-2xl font-semibold tabular-nums">{m.memPct === null ? "—" : `${m.memPct.toFixed(1)} %`}</div>{#if m.kind === "container" && m.mem}<div class="text-fg-muted">{m.mem}</div>{/if}</div>
            </div>
            <p class="mt-2 text-[11px] text-fg-muted">From the monitoring engine's latest reading{m.at ? `, ${ageText(m.at, infra.now)}` : ""}. {m.kind === "container" ? "Container figures are this container's own, not the host's. They exist only while the Containers view reads statistics." : "Charts and history are in Monitoring."}</p>
          {/if}
        </div>
      {/if}
    </main>

    {#if !narrow && inspectorOpen}
      <div class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40" role="separator" aria-orientation="vertical" aria-label="Resize the inspector" onpointerdown={drag((w) => (inspectorW = w), inspectorW, -1)}></div>
      <div class="min-h-0 min-w-0 shrink-0 border-l border-line" style="width: {inspectorW}px"><ResourceInspector onclose={() => (inspectorOpen = false)} /></div>
    {:else if narrow && drawer}
      <div class="absolute inset-y-0 right-0 z-20 w-[22rem] max-w-full border-l border-line shadow-2xl"><ResourceInspector onclose={() => (drawer = false)} /></div>
    {/if}
  </div>
  <p class="border-t border-line px-4 py-1 text-[11px] text-fg-muted">Read-only. Reads what the Containers, Ops and Tmux tools already know and, when you open or refresh a host, asks them to read it again over the connection they keep. Nothing is installed, and nothing is started or stopped from here.</p>
</div>
