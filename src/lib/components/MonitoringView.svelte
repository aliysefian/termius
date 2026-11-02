<script lang="ts">
  import { BellRing, HeartPulse, Plus, RefreshCw, Search, TriangleAlert } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import AlertsTab from "./monitoring/AlertsTab.svelte";
  import ChecksTab from "./monitoring/ChecksTab.svelte";
  import DetailsPanel, { type DetailTab } from "./monitoring/DetailsPanel.svelte";
  import HistoryTab from "./monitoring/HistoryTab.svelte";
  import HostsTab from "./monitoring/HostsTab.svelte";
  import HostTable from "./monitoring/HostTable.svelte";
  import RulesTab from "./monitoring/RulesTab.svelte";
  import SummaryCards from "./monitoring/SummaryCards.svelte";
  import TrafficNow from "./monitoring/TrafficNow.svelte";
  import { buildRow, matchesRow, sortRows, summarize, type HealthFilter, type SortKey } from "$lib/monitoringview";
  import { RANGES, rangeAvailable } from "$lib/netdata";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { DETAIL_MAX, DETAIL_MIN, detailWidth } from "$lib/tunnelslayout";

  type Tab = "overview" | "hosts" | "checks" | "alerts" | "rules" | "history";
  const TABS: { id: Tab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "hosts", label: "Hosts" },
    { id: "checks", label: "Checks" },
    { id: "alerts", label: "Alerts" },
    { id: "rules", label: "Rules" },
    { id: "history", label: "History" },
  ];

  let tab = $state<Tab>("overview");
  let now = $state(Date.now());
  let width = $state(1200);
  let query = $state("");
  let health = $state<HealthFilter>("all");
  let tag = $state("");
  let sort = $state<SortKey>("health");
  let dir = $state<"asc" | "desc">("desc");
  let selectedId = $state<string | null>(null);
  let detailTab = $state<DetailTab>("overview");
  let slideOver = $state(false);
  let dragging = $state(false);
  let refreshing = $state(false);

  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(t);
  });

  const all = $derived(monitoring.targets.filter((t) => t.data).map((rec) => buildRow(rec, monitoring.statuses[rec.id])));
  const counts = $derived(summarize(all));
  const tags = $derived([...new Set(all.flatMap((r) => r.target.tags))].sort());
  const shown = $derived(sortRows(all.filter((r) => matchesRow(r, query, health, tag)), sort, dir));
  const selected = $derived(selectedId ? (all.find((r) => r.id === selectedId) ?? null) : null);
  const filtering = $derived(query.trim() !== "" || health !== "all" || tag !== "");
  const channelsOn = $derived(
    monitoring.channels ? [monitoring.channels.channels.telegram.enabled, monitoring.channels.channels.bale.enabled, monitoring.channels.channels.email.enabled].filter(Boolean).length : 0,
  );
  const keep = $derived(settings.prefs.metricsKeep);

  const docked = $derived(width >= 1100);
  const panelWidth = $derived(detailWidth(settings.prefs.monitoringDetailWidth, width));
  $effect(() => {
    if (docked) slideOver = false;
  });
  // A host that was removed closes its panel.
  $effect(() => {
    if (selectedId && monitoring.targets.length && !all.some((r) => r.id === selectedId)) selectedId = null;
  });

  function pick(id: string, t?: DetailTab) {
    selectedId = id;
    if (t) detailTab = t;
    slideOver = !docked;
    if (tab !== "overview" && tab !== "hosts") tab = "overview";
  }
  function close() {
    selectedId = null;
    slideOver = false;
  }
  function onsort(k: SortKey) {
    if (sort === k) dir = dir === "asc" ? "desc" : "asc";
    else {
      sort = k;
      dir = k === "name" ? "asc" : "desc";
    }
  }
  async function refresh() {
    refreshing = true;
    try {
      await Promise.all(all.filter((r) => r.target.enabled).map((r) => monitoring.runNow(r.id).catch(() => {})));
      await monitoring.refreshSnapshot();
    } finally {
      refreshing = false;
    }
  }

  let startX = 0;
  let startWidth = 0;
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startWidth = panelWidth;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (dragging) settings.prefs.monitoringDetailWidth = detailWidth(startWidth - (e.clientX - startX), width);
  }
  function stop() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      settings.prefs.monitoringDetailWidth = detailWidth(panelWidth + (e.key === "ArrowLeft" ? step : -step), width);
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.monitoringDetailWidth = 460;
    }
  }
  function pageKey(e: KeyboardEvent) {
    if (e.key === "Escape" && selectedId && !(e.target as HTMLElement)?.closest("[role=dialog], [role=menu], input, select, textarea")) close();
  }
  function tabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((x) => x.id === tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    tab = next.id;
    queueMicrotask(() => document.getElementById(`mon-main-${next.id}`)?.focus());
  }
</script>

<svelte:window onkeydown={pageKey} />

<div class="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-base" data-testid="monitoring" bind:clientWidth={width}>
  <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-5">
    <!-- Header -->
    <div class="flex flex-wrap items-center gap-3">
      <h1 class="flex items-center gap-2 text-lg font-semibold"><HeartPulse size={20} class="text-accent" /> Monitoring</h1>
      <div class="flex-1"></div>
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">Time range
        <select class="input !w-auto h-8 py-0 text-xs" bind:value={settings.prefs.monitoringRange} title="The span of the network charts">
          {#each RANGES as r (r.id)}{@const a = rangeAvailable(r, keep)}<option value={r.id} disabled={!a.ok}>{r.label}{a.ok ? "" : " (needs Keep history)"}</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">Live every
        <select class="input !w-auto h-8 py-0 text-xs" bind:value={settings.prefs.monitoringLiveSecs} title="How often the Network tab reads the host while it is open">
          {#each [2, 5, 10, 30] as s (s)}<option value={s}>{s} s</option>{/each}
        </select>
      </label>
      <button class="btn-secondary h-8 text-xs" disabled={refreshing || !monitoring.running} onclick={() => void refresh()}><RefreshCw size={14} class={refreshing ? "animate-spin" : ""} /> Refresh</button>
      <button class="btn-primary h-8 text-xs" data-testid="monitor-add" onclick={() => (ui.modal = { kind: "monitor-target", id: null })}><Plus size={14} /> Add Monitor</button>
    </div>

    {#if monitoring.configError}
      <p class="flex gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger"><TriangleAlert size={15} class="mt-0.5 shrink-0" /> The stored monitoring settings could not be read: {monitoring.configError}</p>
    {:else if !monitoring.running}
      <p class="flex gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning"><TriangleAlert size={15} class="mt-0.5 shrink-0" /> Monitoring is not running. It starts when the vault is unlocked.</p>
    {/if}
    {#if channelsOn === 0}
      <p class="flex flex-wrap items-center gap-2 rounded-md border border-line bg-panel px-3 py-2 text-sm">
        <BellRing size={15} class="text-fg-muted" /> No notification channel is switched on, so alerts show only here.
        <button class="text-accent hover:underline" onclick={() => (ui.view = "settings")}>Set up Telegram, Bale or email…</button>
      </p>
    {/if}

    <SummaryCards summary={counts} onpick={(h) => { health = h; tab = "overview"; }} />

    <!-- Main navigation -->
    <div class="flex w-fit max-w-full gap-1 overflow-x-auto rounded-lg border border-line bg-panel p-1 text-sm" role="tablist" aria-label="Monitoring sections">
      {#each TABS as t (t.id)}
        <button id="mon-main-{t.id}" role="tab" aria-selected={tab === t.id} tabindex={tab === t.id ? 0 : -1} class="flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent {tab === t.id ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}" onclick={() => (tab = t.id)} onkeydown={tabKey}>
          {t.label}
          {#if t.id === "history" && monitoring.unread > 0}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{monitoring.unread}</span>{/if}
          {#if t.id === "alerts" && counts.warning + counts.critical > 0}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{counts.warning + counts.critical}</span>{/if}
        </button>
      {/each}
    </div>

    <div class="flex min-h-0 flex-1 gap-3">
      <div class="min-h-0 min-w-0 flex-1 space-y-3 overflow-y-auto pr-1">
        {#if tab === "overview"}
          {#if all.length === 0}
            <EmptyState icon={HeartPulse} text="No host is being watched yet.">
              <button class="btn-primary mt-3" onclick={() => (ui.modal = { kind: "monitor-target", id: null })}><Plus size={14} /> Add a host</button>
            </EmptyState>
          {:else}
            <div class="flex flex-wrap items-center gap-2">
              <div class="relative min-w-48 flex-1">
                <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
                <input class="input h-8 w-full pl-8 text-sm" type="search" placeholder="Search hosts, addresses, tags" aria-label="Search monitored hosts" bind:value={query} />
              </div>
              <select class="input !w-auto h-8 py-0 text-xs" aria-label="Filter by health" bind:value={health}>
                <option value="all">All states</option><option value="healthy">Healthy</option><option value="warning">Warning</option><option value="critical">Critical</option><option value="offline">Offline</option>
              </select>
              {#if tags.length}
                <select class="input !w-auto h-8 py-0 text-xs" aria-label="Filter by tag" bind:value={tag}>
                  <option value="">All tags</option>{#each tags as x (x)}<option value={x}>{x}</option>{/each}
                </select>
              {/if}
              {#if filtering}<button class="text-xs text-accent hover:underline" onclick={() => { query = ""; health = "all"; tag = ""; }}>Clear filters</button>{/if}
              <span class="text-xs text-fg-muted">{shown.length} of {all.length}</span>
            </div>
            <TrafficNow rows={shown} unit={settings.prefs.netUnit} onpick={(id) => pick(id, "network")} />
            {#if shown.length === 0}
              <EmptyState icon={Search} text="No host matches these filters." />
            {:else}
              <HostTable rows={shown} selectedId={selected?.id ?? null} compact={!!selected && docked} {now} unit={settings.prefs.netUnit} {sort} {dir} {onsort} onpick={(id) => pick(id)} />
            {/if}
          {/if}
        {:else if tab === "hosts"}
          <HostsTab />
        {:else if tab === "checks"}
          <ChecksTab rows={all} {now} onpick={(id) => pick(id, "checks")} />
        {:else if tab === "alerts"}
          <AlertsTab rows={all} {now} onpick={(id) => pick(id, "alerts")} />
        {:else if tab === "rules"}
          <RulesTab />
        {:else}
          <HistoryTab {now} />
        {/if}
      </div>

      {#if docked && selected}
        <aside class="relative flex min-h-0 shrink-0 overflow-hidden rounded-xl border border-line bg-panel" style:width="{panelWidth}px" aria-label="Host details">
          {#key selected.id}<div class="min-h-0 min-w-0 flex-1"><DetailsPanel row={selected} {now} bind:tab={detailTab} onclose={close} /></div>{/key}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="absolute left-0 top-0 z-20 h-full w-2 cursor-col-resize outline-none after:absolute after:left-0 after:top-0 after:h-full after:w-0.5 after:transition-colors {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize the details panel"
            aria-valuenow={panelWidth}
            aria-valuemin={DETAIL_MIN}
            aria-valuemax={DETAIL_MAX}
            tabindex="0"
            title="Drag to resize · double-click to reset"
            onpointerdown={down}
            onpointermove={move}
            onpointerup={stop}
            onpointercancel={stop}
            onlostpointercapture={() => dragging && stop()}
            ondblclick={() => (settings.prefs.monitoringDetailWidth = 460)}
            onkeydown={handleKey}
          ></div>
        </aside>
      {/if}
    </div>
  </div>

  {#if !docked && selected && slideOver}
    <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close details" tabindex="-1" onclick={close}></button>
    <aside class="anim-pop-left absolute inset-y-0 right-0 z-40 flex w-[min(30rem,100%)] flex-col border-l border-line bg-panel shadow-2xl" aria-label="Host details">
      {#key selected.id}<DetailsPanel row={selected} {now} bind:tab={detailTab} onclose={close} />{/key}
    </aside>
  {/if}
</div>
