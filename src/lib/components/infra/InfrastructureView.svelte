<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { AlertTriangle, List, Loader2, Network, RefreshCw, Search, Square, X } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import RelationshipGraph from "./RelationshipGraph.svelte";
  import ResourceInspector from "./ResourceInspector.svelte";
  import ResourceTree from "./ResourceTree.svelte";
  import { ageText, countText, statusLine } from "$lib/infra/inventory";
  import { TYPE_LABEL, STATUS_LABEL, type ResourceStatus, type ResourceType } from "$lib/infra/model";
  import { infra } from "$lib/stores/infra.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { FRESH_LABEL, FRESH_TONE, STATUS_TONE, TYPE_ICON, show } from "./style";

  const FILTER_TYPES: ResourceType[] = ["host", "container", "systemd_service", "tmux_session", "docker_engine", "port"];
  const FILTER_STATUS: ResourceStatus[] = ["running", "healthy", "stopped", "warning", "critical", "unknown"];

  // The saved hosts and their connections follow the vault and the panes on their own; nothing here asks a host.
  $effect(() => infra.syncHosts());
  $effect(() => infra.syncContainers());
  onMount(() => infra.start());
  onDestroy(() => infra.stop());

  let width = $state(typeof window === "undefined" ? 1400 : window.innerWidth);
  const narrow = $derived(width < 1000);
  let treeW = $state(320);
  let inspectorW = $state(320);
  let inspectorOpen = $state(true);
  let query = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Typing is searched locally, a moment after the last key; no host is asked per keystroke. */
  function typed() {
    clearTimeout(timer);
    timer = setTimeout(() => (infra.filters.query = query), 150);
  }
  function drag(set: (w: number) => void, from: number, dir: 1 | -1) {
    return (e: PointerEvent) => {
      e.preventDefault();
      const x0 = e.clientX;
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);
      const move = (m: PointerEvent) => set(Math.min(520, Math.max(220, from + dir * (m.clientX - x0))));
      const up = () => {
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
    };
  }
  function toggle<T>(list: T[], v: T): T[] {
    return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
  }

  const sel = $derived(infra.selected);
  const cards = $derived([
    { label: "Hosts", c: infra.counts.hosts, text: `${infra.counts.hosts.known} saved`, icon: TYPE_ICON.host },
    { label: "Containers", c: infra.counts.containers, text: countText(infra.counts.containers), icon: TYPE_ICON.container },
    { label: "Services", c: infra.counts.services, text: countText(infra.counts.services), icon: TYPE_ICON.systemd_service },
    { label: "Tmux sessions", c: infra.counts.tmux, text: countText(infra.counts.tmux), icon: TYPE_ICON.tmux_session },
  ]);
  const environments = $derived([...new Set(infra.hostIds.map((id) => infra.envOf(id)).filter(Boolean))].sort());
  const tabs = $derived([
    { id: "overview" as const, label: "Overview", show: true },
    { id: "relationships" as const, label: "Relationships", show: !!sel && (infra.parents.length > 0 || infra.children.length > 0) },
    { id: "metrics" as const, label: "Metrics", show: !!infra.metrics },
  ].filter((t) => t.show));
  const tab = $derived(tabs.some((t) => t.id === infra.tab) ? infra.tab : "overview");
</script>

<svelte:window bind:innerWidth={width} />

<div class="flex h-full min-h-0 flex-col" data-testid="infra-view">
  <header class="space-y-3 border-b border-line px-4 py-3">
    <div class="flex flex-wrap items-center gap-3">
      <div class="min-w-0">
        <h1 class="text-[15px] font-semibold">Infrastructure Explorer</h1>
        <p class="text-xs text-fg-muted">Discover and manage resources across your hosts.</p>
      </div>
      <div class="relative ml-auto w-full max-w-sm">
        <Search size={14} class="absolute left-2.5 top-2 text-fg-muted" />
        <input class="input h-8 w-full pl-8 text-xs" placeholder="Search name, host, image, service, tag…" aria-label="Search resources" bind:value={query} oninput={typed} />
      </div>
      {#if infra.discovering}
        <button class="btn-secondary py-1 text-xs" onclick={() => infra.cancel()}><Square size={12} /> Stop</button>
      {/if}
      <button class="btn-secondary py-1 text-xs" onclick={() => infra.refresh()} disabled={infra.discovering || (!!sel?.hostId && !!infra.busy[sel.hostId])} title={sel?.hostId ? `Read ${infra.hostLabel(sel.hostId)} again` : "Read every saved SSH host, a few at a time"}>
        {#if infra.discovering || (sel?.hostId && infra.busy[sel.hostId])}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}
        {sel?.hostId ? "Refresh host" : "Discover all"}
      </button>
    </div>

    <div class="grid grid-cols-2 gap-2 lg:grid-cols-4">
      {#each cards as card (card.label)}
        {@const I = card.icon}
        <div class="rounded-lg border border-line bg-panel px-3 py-2" title={card.label === "Hosts" ? "Saved hosts; none were contacted to count them" : "Counts cover only the hosts that have been read"}>
          <div class="flex items-center gap-2 text-xs text-fg-muted"><I size={13} />{card.label}</div>
          <div class="mt-0.5 text-sm font-semibold tabular-nums">{card.label === "Hosts" ? card.c.known : card.c.hostsObserved === 0 && card.c.hostsFailed === 0 ? "Not read yet" : card.c.known}</div>
          <div class="truncate text-[11px] text-fg-muted">{card.label === "Hosts" || card.c.hostsObserved > 0 || card.c.hostsFailed > 0 ? card.text : "Open a host or Discover all"}</div>
        </div>
      {/each}
    </div>

    <div class="flex flex-wrap items-center gap-1.5 text-xs">
      <span class="text-fg-muted">Show</span>
      {#each FILTER_TYPES as t (t)}
        <button class="rounded-full border px-2 py-0.5 {infra.filters.types.includes(t) ? 'border-accent bg-accent/15 text-accent' : 'border-line text-fg-muted hover:text-fg'}" aria-pressed={infra.filters.types.includes(t)} onclick={() => (infra.filters.types = toggle(infra.filters.types, t))}>{TYPE_LABEL[t]}</button>
      {/each}
      <span class="ml-2 text-fg-muted">Status</span>
      <select class="input h-7 w-28 py-0 text-xs" aria-label="Status" value={infra.filters.statuses[0] ?? ""} onchange={(e) => (infra.filters.statuses = e.currentTarget.value ? [e.currentTarget.value as ResourceStatus] : [])}>
        <option value="">Any</option>{#each FILTER_STATUS as s (s)}<option value={s}>{STATUS_LABEL[s]}</option>{/each}
      </select>
      {#if environments.length}
        <select class="input h-7 w-44 py-0 text-xs" aria-label="Environment" bind:value={infra.filters.environment}><option value="">Any environment</option>{#each environments as e (e)}<option value={e}>{e}</option>{/each}</select>
      {/if}
      {#if infra.filters.query || infra.filters.types.length || infra.filters.statuses.length || infra.filters.environment}
        <button class="inline-flex items-center gap-1 text-accent" onclick={() => { query = ""; infra.filters = { query: "", types: [], statuses: [], environment: "", hostId: null }; }}><X size={12} /> Clear</button>
      {/if}
      {#if infra.coverage.read < infra.coverage.total}
        <span class="ml-auto text-fg-muted" title="Search and counts cover what has been read; nothing is asked of a host while you type">Searching {infra.coverage.read} of {infra.coverage.total} hosts read</span>
      {/if}
    </div>
  </header>

  <div class="flex min-h-0 flex-1">
    <section class="flex min-h-0 shrink-0 flex-col border-r border-line" style="width: {narrow ? 240 : treeW}px" aria-label="Resource explorer">
      <ResourceTree />
    </section>
    {#if !narrow}<div class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40" role="separator" aria-orientation="vertical" aria-label="Resize the explorer" onpointerdown={drag((w) => (treeW = w), treeW, 1)}></div>{/if}

    <main class="flex min-w-0 flex-1 flex-col">
      {#if !sel}
        <div class="m-auto"><EmptyState icon={TYPE_ICON.host} text={infra.hostIds.length ? "Open a host in the list, or search, then select a resource." : "No saved SSH hosts yet."} /></div>
      {:else}
        <div class="flex items-center gap-1 border-b border-line px-3" role="tablist">
          {#each tabs as t (t.id)}<button role="tab" aria-selected={tab === t.id} class="border-b-2 px-3 py-2 text-xs {tab === t.id ? 'border-accent text-fg' : 'border-transparent text-fg-muted hover:text-fg'}" onclick={() => (infra.tab = t.id)}>{t.label}</button>{/each}
          <div class="ml-auto flex items-center gap-1">
            {#if narrow}<button class="btn-secondary py-0.5 text-xs" onclick={() => (inspectorOpen = !inspectorOpen)}>Details</button>{/if}
            {#if tab === "relationships"}
              <button class="icon-btn h-7 w-7 {infra.view === 'graph' ? 'text-accent' : ''}" title="Graph" aria-label="Graph view" aria-pressed={infra.view === "graph"} onclick={() => (infra.view = "graph")}><Network size={14} /></button>
              <button class="icon-btn h-7 w-7 {infra.view === 'list' ? 'text-accent' : ''}" title="List" aria-label="List view" aria-pressed={infra.view === "list"} onclick={() => (infra.view = "list")}><List size={14} /></button>
            {/if}
          </div>
        </div>
        <div class="min-h-0 flex-1 overflow-auto p-4 text-xs" role="tabpanel">
          {#if tab === "overview"}
            <div class="space-y-3">
              <div class="rounded-lg border border-line bg-panel p-3">
                <div class="flex items-center gap-2"><span class="h-2.5 w-2.5 rounded-full {STATUS_TONE[sel.status]}"></span><span class="text-sm font-semibold">{sel.name}</span></div>
                <p class="mt-1 text-fg-muted">{statusLine(sel, infra.now)}</p>
                <p class="{FRESH_TONE[sel.freshness]}">{FRESH_LABEL[sel.freshness]}{sel.observedAt ? ` · read ${ageText(sel.observedAt, infra.now)}` : ""}</p>
              </div>
              {#if sel.freshness === "stale" || sel.freshness === "unavailable"}
                <p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-warning"><AlertTriangle size={13} class="mt-0.5 shrink-0" />This is the last known state, not what the host reports now. Refresh the host to read it again.</p>
              {/if}
              <dl class="grid max-w-xl grid-cols-[8rem_1fr] gap-x-3 gap-y-1.5 rounded-lg border border-line bg-panel p-3">
                {#each Object.entries(sel.attributes) as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="break-words">{show(k, v)}</dd>{/each}
              </dl>
            </div>
          {:else if tab === "relationships"}
            {#if infra.view === "graph" && infra.graph}
              <div class="h-[min(32rem,70vh)]"><RelationshipGraph graph={infra.graph} /></div>
              <p class="mt-2 text-[11px] text-fg-muted">Lines are only what the owning module reported: a host and its engine, an engine and its containers, a container and the ports it publishes. Similar names are not connected.</p>
            {:else}
              <ul class="space-y-1">
                {#each [...infra.parents, ...(sel ? [sel] : []), ...infra.children] as n (n.id)}
                  {@const I = TYPE_ICON[n.type]}
                  <li><button class="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-panel-hover/50 {n.id === sel?.id ? 'font-semibold' : ''}" onclick={() => infra.select(n.id)}><I size={13} class="text-fg-muted" />{n.name}<span class="text-fg-muted">· {TYPE_LABEL[n.type]}</span></button></li>
                {/each}
              </ul>
            {/if}
          {:else if tab === "metrics" && infra.metrics}
            {@const m = infra.metrics}
            <div class="grid max-w-md grid-cols-2 gap-3">
              <div class="rounded-lg border border-line bg-panel p-3"><div class="text-fg-muted">{m.kind === "host" ? "Host CPU" : "Container CPU"}</div><div class="text-lg font-semibold tabular-nums">{m.cpu === null ? "—" : `${m.cpu.toFixed(1)} %`}</div></div>
              <div class="rounded-lg border border-line bg-panel p-3"><div class="text-fg-muted">{m.kind === "host" ? "Host memory" : "Container memory"}</div><div class="text-lg font-semibold tabular-nums">{m.memPct === null ? "—" : `${m.memPct.toFixed(1)} %`}</div>{#if m.kind === "container" && m.mem}<div class="text-fg-muted">{m.mem}</div>{/if}</div>
            </div>
            <p class="mt-2 text-[11px] text-fg-muted">From the monitoring engine's latest reading{m.at ? `, ${ageText(m.at, infra.now)}` : ""}. {m.kind === "container" ? "Container figures are this container's own, not the host's. They exist only while the Containers view reads statistics." : "Charts and history are in Monitoring."}</p>
          {/if}
        </div>
      {/if}
    </main>

    {#if !narrow}
      <div class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40" role="separator" aria-orientation="vertical" aria-label="Resize the details" onpointerdown={drag((w) => (inspectorW = w), inspectorW, -1)}></div>
      <div class="shrink-0" style="width: {inspectorW}px"><ResourceInspector /></div>
    {:else if inspectorOpen && sel}
      <div class="fixed inset-y-0 right-0 z-40 w-[22rem] max-w-full shadow-2xl"><ResourceInspector onclose={() => (inspectorOpen = false)} /></div>
    {/if}
  </div>
  <p class="border-t border-line px-4 py-1.5 text-[11px] text-fg-muted">Read-only. Reads what the Containers, Ops and Tmux tools already know and, when you open or refresh a host, asks them to read it again over the connection they keep. Nothing is installed, and nothing is started or stopped from here.</p>
</div>
