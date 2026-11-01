<script lang="ts">
  // The Containers workspace: which host and runtime, a few counts, the resources as tabs, and for containers a table
  // grouped by Compose project with a details drawer beside it. Kubernetes stays one tab over (ContainerTabs).
  import { onMount } from "svelte";
  import { Download, Loader2, Play, RotateCw, Square, Trash2, X } from "lucide-svelte";
  import ContainerTabs from "./ContainerTabs.svelte";
  import EmptyState from "./EmptyState.svelte";
  import ComposeView from "./containers/ComposeView.svelte";
  import ContainerDrawer from "./containers/ContainerDrawer.svelte";
  import ContainerMetrics from "./containers/ContainerMetrics.svelte";
  import ContainerTable from "./containers/ContainerTable.svelte";
  import ContainerToolbar from "./containers/ContainerToolbar.svelte";
  import ErrorState from "./containers/ErrorState.svelte";
  import HostSelector from "./containers/HostSelector.svelte";
  import ResourcePanels from "./containers/ResourcePanels.svelte";
  import type { DrawerTab } from "./containers/actions";
  import { composeProject, filterImages, filterNetworks, filterVolumes } from "$lib/containerdata";
  import { groupSummaries, metrics as countMetrics, searchContainers, statusCounts, visibleColumns, type ColumnId, type StatusFilter } from "$lib/containerview";
  import { DETAIL_MAX, DETAIL_MIN, detailWidth } from "$lib/tunnelslayout";
  import { containers, LOCAL } from "$lib/stores/containers.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { CONTAINER_RUNTIMES, type ContainerRuntime } from "$lib/types";

  type Tab = "containers" | "images" | "volumes" | "networks" | "compose";
  let tab = $state<Tab>("containers");
  let query = $state("");
  let status = $state<StatusFilter>("all");
  let drawerTab = $state<DrawerTab>("overview");
  let now = $state(Date.now());
  let width = $state(1200);
  /** The table card's own width, which decides how many columns fit. */
  let cardWidth = $state(1000);

  // Ages tick without another round trip.
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 15_000);
    return () => clearInterval(t);
  });

  // The periodic refresh. Skipped while the window is hidden or the person paused it.
  $effect(() => {
    const key = containers.activeKey;
    const secs = containers.intervalSecs;
    if (!key || containers.paused) return;
    const t = setInterval(() => {
      if (!document.hidden) void containers.refresh(key, true);
    }, secs * 1000);
    return () => clearInterval(t);
  });

  const src = $derived(containers.active);
  const key = $derived(containers.activeKey);
  const isDocker = $derived(src?.runtime === "docker");
  const listing = $derived(src?.listing ?? null);
  const resources = $derived(src?.resources ?? null);
  const all = $derived(listing?.containers ?? []);
  const counts = $derived(countMetrics(all));
  const byStatus = $derived(statusCounts(all));
  const shown = $derived(searchContainers(all, query, status));
  const hasProjects = $derived(all.some((c) => composeProject(c)));
  const groups = $derived(settings.prefs.containerGrouped && shown.some((c) => composeProject(c)) ? groupSummaries(shown) : null);
  const projectCount = $derived(new Set(all.map(composeProject).filter(Boolean)).size);
  const filtering = $derived(query.trim() !== "" || status !== "all");
  const selected = $derived(containers.selectedId ? (all.find((c) => c.id === containers.selectedId) ?? null) : null);
  const checked = $derived(all.filter((c) => containers.checked.has(c.id)));
  const loadingKey = $derived(key && !src && containers.connecting[key]);

  // The drawer sits beside the table when there is room, and over it when there is not.
  const docked = $derived(width >= 1100);
  let slideOver = $state(false);
  const drawerWidth = $derived(detailWidth(settings.prefs.containerDrawerWidth, width));
  const columns = $derived(visibleColumns(settings.prefs.containerColumns as ColumnId[], cardWidth, !!src?.wantStats, !!src?.sizes));

  function showDrawer(t?: DrawerTab) {
    if (t) drawerTab = t;
    slideOver = !docked;
  }
  $effect(() => {
    if (docked) slideOver = false;
  });
  // A container that is gone (removed here or elsewhere) closes its drawer.
  $effect(() => {
    if (containers.selectedId && listing && !all.some((c) => c.id === containers.selectedId)) containers.select(null);
  });

  // Volumes and networks belong to Docker for now; Compose too. Fall back if the runtime changes under the tab.
  $effect(() => {
    if (src && !isDocker && (tab === "volumes" || tab === "networks" || tab === "compose")) tab = "containers";
  });
  // Volumes and networks cost two more commands, so they are only fetched while a tab shows them.
  $effect(() => {
    const s = src;
    if (!s) return;
    const want = s.runtime === "docker" && (tab === "volumes" || tab === "networks");
    if (s.wantResources !== want) {
      s.wantResources = want;
      if (want) void containers.refresh(s.key);
    }
  });

  const tabs = $derived<{ id: Tab; label: string; count: number | null }[]>([
    { id: "containers", label: "Containers", count: listing ? all.length : null },
    { id: "images", label: "Images", count: listing ? listing.images.length : null },
    ...(isDocker
      ? [
          { id: "volumes" as Tab, label: "Volumes", count: resources ? resources.volumes.length : null },
          { id: "networks" as Tab, label: "Networks", count: resources ? resources.networks.length : null },
          { id: "compose" as Tab, label: "Compose", count: listing ? projectCount : null },
        ]
      : []),
  ]);

  const shownImages = $derived(listing ? filterImages(listing.images, query) : []);
  const shownVolumes = $derived(resources ? filterVolumes(resources.volumes, query) : []);
  const shownNetworks = $derived(resources ? filterNetworks(resources.networks, query) : []);

  function clearFilters() {
    query = "";
    status = "all";
  }
  function pickStatus(s: "all" | "running" | "stopped" | "unhealthy") {
    tab = "containers";
    query = "";
    status = s;
  }
  async function reconnect() {
    if (!key) return;
    await containers.close(key);
    await containers.switchTo(key);
  }

  // -- the drawer's width ---------------------------------------------------------------------------------------
  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startWidth = drawerWidth;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (dragging) settings.prefs.containerDrawerWidth = detailWidth(startWidth - (e.clientX - startX), width);
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
      settings.prefs.containerDrawerWidth = detailWidth(drawerWidth + (e.key === "ArrowLeft" ? step : -step), width);
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.containerDrawerWidth = 480;
    }
  }

  function pageKey(e: KeyboardEvent) {
    if (e.key === "Escape" && containers.selectedId && !(e.target as HTMLElement)?.closest("[role=dialog], [role=menu], input, select, textarea")) {
      containers.select(null);
      slideOver = false;
    }
  }

  // Nothing is opened on its own: a host is chosen on purpose. This computer is the one offered first.
  onMount(() => {
    if (!containers.activeKey && Object.keys(containers.sources).length) containers.activeKey = Object.keys(containers.sources)[0];
  });
</script>

<svelte:window onkeydown={pageKey} />

<div class="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" bind:clientWidth={width}>
  <ContainerTabs current="containers" />

  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden p-6 max-md:p-4">
    <!-- Header -->
    <header class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0">
        <h1 class="text-xl font-semibold tracking-tight">Containers</h1>
        <p class="mt-0.5 text-sm text-fg-muted">Manage containers, images, volumes, and networks.</p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <HostSelector />
        {#if src}
          <label class="flex items-center gap-1.5 text-xs text-fg-muted">
            <span class="sr-only">Runtime</span>
            <select class="input h-9 w-auto py-0 text-sm" value={src.runtime} aria-label="Container runtime" onchange={(e) => containers.setRuntime(src.key, e.currentTarget.value as ContainerRuntime)}>
              {#each src.detected.length ? src.detected : CONTAINER_RUNTIMES as r (r)}<option value={r}>{r}</option>{/each}
            </select>
          </label>
          {#if tab === "images"}
            <button class="btn-primary h-9" onclick={() => (ui.modal = { kind: "container-pull", sourceKey: src.key })}><Download size={14} /> Pull image</button>
          {/if}
          {#if tab === "images" || tab === "volumes" || tab === "networks"}
            <button class="btn-secondary h-9" onclick={() => (ui.modal = { kind: "container-prune", sourceKey: src.key, what: tab as "images" | "volumes" | "networks" })}><Trash2 size={14} /> Remove unused…</button>
          {/if}
        {/if}
      </div>
    </header>

    {#if loadingKey}
      <!-- The host being opened: nothing of the previous one is shown in its place. -->
      <div class="m-auto flex flex-col items-center gap-3 text-sm text-fg-muted" role="status"><Loader2 size={22} class="animate-spin" /> Connecting to {containers.label(key!)}…</div>
    {:else if !src}
      <div class="flex flex-1 items-center justify-center">
        <EmptyState art="containers" text="Choose this computer or one of your hosts to see its containers.">
          <button class="btn-primary" onclick={() => containers.switchTo(LOCAL)}>Open this computer</button>
        </EmptyState>
      </div>
    {:else}
      <ContainerMetrics metrics={counts} onpick={pickStatus} />

      <div class="flex min-h-0 flex-1 gap-4">
        <section bind:clientWidth={cardWidth} class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-panel" aria-label="{src.label} resources">
          <!-- Resource navigation -->
          <div class="flex gap-1 overflow-x-auto border-b border-line px-3 pt-2" role="tablist" aria-label="What to list">
            {#each tabs as t (t.id)}
              <button
                role="tab"
                aria-selected={tab === t.id}
                class="-mb-px shrink-0 rounded-t-md border-b-2 px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent {tab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
                onclick={() => (tab = t.id)}
              >
                {t.label}{#if t.count !== null}<span class="ml-1.5 rounded-full bg-fg-muted/10 px-1.5 py-px text-[11px] tabular-nums text-fg-muted">{t.count}</span>{/if}
              </button>
            {/each}
          </div>

          <!-- Toolbar -->
          {#if tab !== "compose"}
            <div class="border-b border-line p-3">
              {#if tab === "containers"}
                <ContainerToolbar sourceKey={src.key} bind:query bind:status counts={byStatus} {hasProjects} />
              {:else}
                <input class="input h-9 w-full py-0 text-sm" placeholder="Search…" aria-label="Search" bind:value={query} />
              {/if}
            </div>
          {/if}

          {#if checked.length && tab === "containers"}
            <div class="flex flex-wrap items-center gap-2 border-b border-accent/30 bg-accent/10 px-4 py-2 text-sm" role="toolbar" aria-label="Selected containers">
              <span class="font-medium">{checked.length} container{checked.length === 1 ? "" : "s"} selected</span>
              <div class="flex-1"></div>
              <button class="btn-secondary h-8" onclick={() => void containers.bulk(src.key, checked, "start")}><Play size={13} /> Start</button>
              <button class="btn-secondary h-8" onclick={() => void containers.bulk(src.key, checked, "stop")}><Square size={13} /> Stop</button>
              <button class="btn-secondary h-8" onclick={() => void containers.bulk(src.key, checked, "restart")}><RotateCw size={13} /> Restart</button>
              <button class="btn-secondary h-8 text-danger" onclick={() => void containers.bulk(src.key, checked, "remove")}><Trash2 size={13} /> Delete</button>
              <button class="icon-btn h-8 w-8" title="Clear selection" aria-label="Clear selection" onclick={() => (containers.checked = new Set())}><X size={15} /></button>
            </div>
          {/if}

          <!-- Content -->
          {#if src.error && !listing}
            <ErrorState message={src.error} loading={src.loading} onretry={() => containers.refresh(src.key)} onreconnect={reconnect} />
          {:else if !listing}
            <div class="flex flex-col gap-2 p-4" role="status" aria-label="Loading containers">
              {#each [0, 1, 2, 3, 4] as i (i)}<div class="h-11 animate-pulse rounded-md bg-fg-muted/10 motion-reduce:animate-none"></div>{/each}
            </div>
          {:else}
            {#if src.error}
              <p class="mx-4 mt-3 whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-warning" role="alert">The list may be out of date: {src.error}</p>
            {/if}
            {#if tab === "containers"}
              {#if all.length === 0}
                <EmptyState icon={Download} text="No containers on {src.label}." />
              {:else if shown.length === 0}
                <div class="m-auto p-8 text-center">
                  <p class="text-sm font-medium">No matching containers</p>
                  <p class="mt-1 text-xs text-fg-muted">No container matches your search or filter.</p>
                  <button class="btn-secondary mt-3" onclick={clearFilters}>Clear filters</button>
                </div>
              {:else}
                <ContainerTable sourceKey={src.key} rows={shown} {groups} {columns} {now} {isDocker} onopen={showDrawer} />
                <div class="border-t border-line px-4 py-2 text-xs text-fg-muted">{filtering ? `${shown.length} of ${all.length} containers` : `${all.length} container${all.length === 1 ? "" : "s"}`}</div>
              {/if}
            {:else if tab === "compose"}
              {#if projectCount === 0}
                <EmptyState icon={Download} text="No Compose projects on {src.label}. Containers started with docker compose appear here." />
              {:else}
                <ComposeView sourceKey={src.key} list={all} {isDocker} onshow={(p) => { tab = "containers"; query = p; }} />
              {/if}
            {:else}
              {#if src.resourcesError && (tab === "volumes" || tab === "networks")}
                <p class="mx-4 mt-3 whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-warning" role="alert">{src.resourcesError}</p>
              {/if}
              <ResourcePanels sourceKey={src.key} kind={tab as "images" | "volumes" | "networks"} images={shownImages} volumes={shownVolumes} networks={shownNetworks} imagesError={listing.images_error} resourcesLoaded={!!resources} {now} query={query.trim() !== ""} />
            {/if}
          {/if}
        </section>

        <!-- Details, beside the table -->
        {#if docked && tab === "containers" && selected}
          <aside class="relative flex min-h-0 shrink-0 overflow-hidden rounded-xl border border-line bg-panel" style:width="{drawerWidth}px" aria-label="Container details">
            {#key src.key + selected.id}
              <div class="min-h-0 min-w-0 flex-1"><ContainerDrawer sourceKey={src.key} c={selected} bind:tab={drawerTab} onclose={() => containers.select(null)} /></div>
            {/key}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="absolute left-0 top-0 z-20 h-full w-2 cursor-col-resize outline-none after:absolute after:left-0 after:top-0 after:h-full after:w-0.5 after:transition-colors {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize the details drawer"
              aria-valuenow={drawerWidth}
              aria-valuemin={DETAIL_MIN}
              aria-valuemax={DETAIL_MAX}
              tabindex="0"
              title="Drag to resize · double-click to reset"
              onpointerdown={down}
              onpointermove={move}
              onpointerup={stop}
              onpointercancel={stop}
              onlostpointercapture={() => dragging && stop()}
              ondblclick={() => (settings.prefs.containerDrawerWidth = 480)}
              onkeydown={handleKey}
            ></div>
          </aside>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Details, over the table, on a window too narrow to put them beside it -->
  {#if !docked && selected && slideOver && src && tab === "containers"}
    <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close details" tabindex="-1" onclick={() => { slideOver = false; containers.select(null); }}></button>
    <aside class="anim-pop-left absolute inset-y-0 right-0 z-40 flex w-[min(30rem,100%)] flex-col border-l border-line bg-panel shadow-2xl" aria-label="Container details">
      {#key src.key + selected.id}<ContainerDrawer sourceKey={src.key} c={selected} bind:tab={drawerTab} onclose={() => { slideOver = false; containers.select(null); }} />{/key}
    </aside>
  {/if}
</div>
