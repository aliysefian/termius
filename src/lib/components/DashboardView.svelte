<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowDown, ArrowLeftRight, ArrowUp, CircleUser, Clock, Database, KeyRound, Loader2, Plus, RefreshCw, Search, Server, Settings, SquareTerminal, Tag, Terminal, X } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import EmptyState from "./EmptyState.svelte";
  import PageSection from "./PageSection.svelte";
  import {
    DEFAULT_SORT,
    LEVEL_FILL,
    NO_FILTER,
    activeTunnels,
    activityRows,
    clampPct,
    countOnline,
    filterRows,
    hostStatus,
    isFiltering,
    nextSort,
    releaseChannel,
    sortRows,
    usageLevel,
    vaultHealth,
    type HostFilter,
    type SortKey,
  } from "$lib/dashboard";
  import { formatUptime } from "$lib/hostmetrics";
  import { connectionLog } from "$lib/stores/connectionlog.svelte";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { buildTree, groupPaths } from "$lib/tree";

  const CHECK_EVERY_MS = 30_000;
  const TICK_MS = 5_000;
  const ACTIVITY_LINES = 8;

  let now = $state(Date.now());

  let filter = $state<HostFilter>({ ...NO_FILTER });
  let sort = $state(DEFAULT_SORT);

  // Every host: the tiles and the health check count these, whatever the table is filtered to.
  const all = $derived(
    vaultStore.hosts
      .filter((h) => h.data)
      .map((h) => {
        const d = h.data!;
        const monitored = hostMetrics.isMonitored(h.id);
        // A reading left from before monitoring was turned off is stale: show none.
        const reading = monitored ? hostMetrics.readings[h.id] : undefined;
        const m = reading?.metrics;
        return {
          id: h.id,
          d,
          label: d.label,
          hostname: d.hostname,
          group: d.group,
          tags: d.tags,
          health: vaultStore.health[h.id],
          status: hostStatus(vaultStore.health[h.id]),
          monitored,
          canMonitor: hostMetrics.canMonitor(h.id),
          reading,
          cpu: m?.cpuPct ?? null,
          ram: m?.memPct ?? null,
          disk: m?.diskPct ?? null,
        };
      }),
  );
  const rows = $derived(sortRows(filterRows(all, filter), sort));
  const groups = $derived(groupPaths(buildTree(vaultStore.hosts)).sort());
  const filtering = $derived(isFiltering(filter));

  const online = $derived(countOnline(all.map((r) => r.status)));
  const checked = $derived(all.some((r) => r.status !== "unknown"));
  const db = $derived(vaultHealth({ unlocked: vaultStore.unlocked, conflicts: vaultStore.openConflicts, rollbacks: vaultStore.rollbacks }));
  const appVersion = __APP_VERSION__;
  const channel = releaseChannel(appVersion);
  // How long this app has been running: the page was loaded when the app started.
  const uptime = $derived(formatUptime(Math.max(0, Math.floor((now - performance.timeOrigin) / 1000))));
  const tunnels = $derived(activeTunnels(vaultStore.forwardStatus));
  const activity = $derived(activityRows(connectionLog.entries, now, ACTIVITY_LINES));

  async function refresh() {
    if (vaultStore.checking || !all.length) return;
    await vaultStore.checkHealth(
      all.map((r) => r.id),
      true,
    );
  }

  onMount(() => {
    vaultStore.refreshSyncProblems(0);
    void refresh();
    // Hosts already being monitored: read them now rather than at the store's next turn.
    void hostMetrics.poll();
    const check = setInterval(() => void refresh(), CHECK_EVERY_MS);
    const tick = setInterval(() => (now = Date.now()), TICK_MS);
    return () => {
      clearInterval(check);
      clearInterval(tick);
    };
  });

  const TONE_TEXT = { success: "text-success", warning: "text-warning", danger: "text-danger" };
</script>

{#snippet tile(label: string, icon: typeof Server, title?: string)}
  {@const Icon = icon}
  <div class="flex items-center justify-between text-xs text-fg-muted" {title}>
    <span class="font-medium uppercase tracking-wide">{label}</span>
    <Icon size={14} />
  </div>
{/snippet}

{#snippet usage(pct: number | null, label: string)}
  {#if pct == null}
    <span class="text-fg-muted" title="{label} isn't available on this host">—</span>
  {:else}
    {@const p = clampPct(pct)}
    <div class="flex items-center gap-2">
      <div class="h-1.5 w-20 overflow-hidden rounded-full bg-line" role="progressbar" aria-label={label} aria-valuenow={p} aria-valuemin={0} aria-valuemax={100}>
        <div class="h-full rounded-full {LEVEL_FILL[usageLevel(p)]}" style="width: {p}%"></div>
      </div>
      <span class="w-8 font-mono text-[11px] text-fg-muted">{p}%</span>
    </div>
  {/if}
{/snippet}

{#snippet th(key: SortKey, label: string, cls: string)}
  <th class={cls} aria-sort={sort.key === key ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
    <button class="inline-flex items-center gap-1 rounded font-medium hover:text-fg {sort.key === key ? 'text-fg' : ''}" title="Sort by {label.toLowerCase()}" onclick={() => (sort = nextSort(sort, key))}>
      {label}
      {#if sort.key === key}{#if sort.dir === "asc"}<ArrowUp size={11} />{:else}<ArrowDown size={11} />{/if}{/if}
    </button>
  </th>
{/snippet}

{#snippet total(label: string, value: number, icon: typeof Server)}
  {@const Icon = icon}
  <div class="flex items-center gap-2">
    <Icon size={13} class="text-accent" />
    <span class="font-medium uppercase tracking-wide text-fg-muted">{label}</span>
    <span class="font-semibold">{value.toLocaleString()}</span>
  </div>
{/snippet}

{#snippet quick(label: string, icon: typeof Server, run: () => void)}
  {@const Icon = icon}
  <button class="flex flex-col items-center justify-center gap-2 rounded-lg border border-line bg-base px-3 py-4 text-xs font-medium transition-colors hover:bg-panel-hover" onclick={run}>
    <Icon size={20} class="text-accent" />
    {label}
  </button>
{/snippet}

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-7xl space-y-4">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-lg font-semibold">Dashboard</h1>
        <p class="mt-1 text-sm text-fg-muted">Your hosts, tunnels and recent sessions at a glance.</p>
      </div>
      <button class="btn-secondary py-1.5 text-xs" disabled={vaultStore.checking || !all.length} onclick={refresh}>
        {#if vaultStore.checking}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Check now
      </button>
    </div>

    <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
      <div class="rounded-xl border border-line bg-panel p-4">
        {@render tile("Version", Tag)}
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <span class="text-2xl font-semibold">v{appVersion}</span>
          <Badge tone={channel === "STABLE" ? "success" : "warning"}>{channel}</Badge>
        </div>
      </div>
      <div class="rounded-xl border border-line bg-panel p-4">
        {@render tile("Uptime", Clock, "How long SSHVault has been running")}
        <div class="mt-2 text-2xl font-semibold">{uptime}</div>
      </div>
      <div class="rounded-xl border border-line bg-panel p-4">
        {@render tile("Database", Database, "The vault and its synced folder")}
        <div class="mt-2 text-2xl font-semibold {TONE_TEXT[db.tone]}" title={db.detail}>{db.label}</div>
      </div>
      <div class="rounded-xl border border-line bg-panel p-4">
        {@render tile("Hosts Available", Server, "Hosts that answered a connection check just now")}
        <div class="mt-2 flex items-baseline gap-1.5">
          <span class="text-2xl font-semibold">{checked || !all.length ? online : "—"}</span>
          <span class="text-sm text-fg-muted">/ {all.length}</span>
        </div>
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-x-8 gap-y-2 rounded-lg border border-line bg-panel px-4 py-2 text-xs">
      {@render total("Total Hosts", vaultStore.hosts.length, Server)}
      {@render total("Total Credentials", vaultStore.identities.length, KeyRound)}
      {@render total("Active Tunnels", tunnels, ArrowLeftRight)}
    </div>

    <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <PageSection title="Host Status" icon={Server}>
        {#if all.length === 0}
          <EmptyState art="hosts" text="No hosts yet.">
            <button class="btn-primary mt-3" onclick={() => (ui.modal = { kind: "host", id: null })}><Plus size={14} /> Add Host</button>
          </EmptyState>
        {:else}
          <div class="mb-3 flex flex-wrap items-center gap-2">
            <div class="relative min-w-40 flex-1">
              <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
              <input class="input py-1 pl-7 text-xs" placeholder="Filter hosts…" bind:value={filter.query} aria-label="Filter hosts by name, address, group or tag" spellcheck="false" />
            </div>
            <select class="input w-auto py-1 text-xs" bind:value={filter.status} aria-label="Filter by status">
              <option value="all">All statuses</option>
              <option value="online">Online</option>
              <option value="offline">Offline</option>
              <option value="unknown">Unknown</option>
            </select>
            <select class="input w-auto py-1 text-xs" bind:value={filter.monitoring} aria-label="Filter by monitoring">
              <option value="all">Monitored or not</option>
              <option value="on">Monitored</option>
              <option value="off">Not monitored</option>
            </select>
            {#if groups.length}
              <select class="input w-auto max-w-44 py-1 text-xs" bind:value={filter.group} aria-label="Filter by group">
                <option value="">All groups</option>
                {#each groups as g (g)}<option value={g}>{g}</option>{/each}
              </select>
            {/if}
            {#if filtering}
              <button class="btn-ghost py-1 text-xs" onclick={() => (filter = { ...NO_FILTER })}><X size={12} /> Clear</button>
            {/if}
            <span class="ml-auto text-xs text-fg-muted" aria-live="polite">{rows.length === all.length ? `${all.length} ${all.length === 1 ? "host" : "hosts"}` : `${rows.length} of ${all.length} hosts`}</span>
          </div>
          <div class="-mx-5 -mb-5 max-h-[34rem] overflow-auto border-t border-line">
            <table class="w-full min-w-[46rem] text-left text-xs">
              <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
                <tr>
                  {@render th("host", "Host", "px-5 py-2")}
                  {@render th("address", "Address", "px-2 py-2")}
                  {@render th("cpu", "CPU", "px-2 py-2")}
                  {@render th("ram", "RAM", "px-2 py-2")}
                  {@render th("disk", "Disk", "px-2 py-2")}
                  {@render th("monitor", "Monitor", "px-2 py-2")}
                  {@render th("status", "Status", "px-5 py-2 text-right")}
                </tr>
              </thead>
              <tbody>
                {#each rows as r (r.id)}
                  <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                    <td class="max-w-56 px-5 py-2">
                      <button class="flex w-full items-center gap-2 text-left" title="Open a terminal on {r.d.label}" onclick={() => ui.openTerminal(r.id, r.d.label)}>
                        <span aria-hidden="true" class="h-2 w-2 shrink-0 rounded-full {r.status === 'online' ? 'bg-success' : r.status === 'offline' ? 'bg-danger' : 'bg-fg-muted/40'}"></span>
                        <span class="truncate text-sm font-medium">{r.d.label}</span>
                      </button>
                    </td>
                    <td class="max-w-48 truncate px-2 py-2 font-mono text-fg-muted" title="{r.d.hostname}:{r.d.port}">{r.d.hostname}</td>
                    {#if r.reading?.metrics}
                      <td class="px-2 py-2">{@render usage(r.cpu, "CPU")}</td>
                      <td class="px-2 py-2">{@render usage(r.ram, "RAM")}</td>
                      <td class="px-2 py-2">{@render usage(r.disk, "Disk")}</td>
                    {:else}
                      <td colspan="3" class="px-2 py-2 text-fg-muted">
                        {#if !r.canMonitor}
                          Needs saved credentials to monitor
                        {:else if !r.monitored}
                          Monitoring is off
                        {:else if r.reading?.error}
                          <span class="block max-w-72 truncate" title={r.reading.error}>Couldn't read usage: {r.reading.error}</span>
                        {:else}
                          Waiting for the first reading…
                        {/if}
                      </td>
                    {/if}
                    <td class="px-2 py-2">
                      <button
                        role="switch"
                        aria-checked={r.monitored}
                        aria-label="Monitor {r.label}"
                        class="relative h-4 w-7 shrink-0 rounded-full border transition-colors disabled:cursor-not-allowed disabled:opacity-50 {r.monitored ? 'border-accent bg-accent' : 'border-line bg-base'}"
                        disabled={!r.monitored && !r.canMonitor}
                        title={r.monitored ? "Stop monitoring this host" : r.canMonitor ? "Read CPU, RAM and disk over SSH every 30 seconds" : "Needs saved credentials to monitor"}
                        onclick={() => hostMetrics.setMonitored(r.id, !r.monitored)}
                      >
                        <span aria-hidden="true" class="absolute top-1/2 h-2.5 w-2.5 -translate-y-1/2 rounded-full transition-all {r.monitored ? 'left-[0.9rem] bg-white' : 'left-0.5 bg-fg-muted'}"></span>
                      </button>
                    </td>
                    <td class="px-5 py-2 text-right">
                      {#if r.status === "online"}
                        <Badge tone="success" title={r.health?.state === "up" ? `Answered in ${r.health.latency_ms} ms` : undefined}>Online</Badge>
                      {:else if r.status === "offline"}
                        <Badge tone="danger" title={r.health?.state === "down" ? r.health.reason : undefined}>Offline</Badge>
                      {:else}
                        <Badge tone="neutral" title={r.health ? "Reached only through a jump host, so it can't be checked directly" : "Not checked yet"}>Unknown</Badge>
                      {/if}
                    </td>
                  </tr>
                {:else}
                  <tr>
                    <td colspan="7" class="px-5 py-6 text-center text-fg-muted">
                      No hosts match these filters.
                      <button class="btn-ghost ml-1 py-0.5 text-xs" onclick={() => (filter = { ...NO_FILTER })}>Clear filters</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </PageSection>

      <div class="space-y-4">
        <PageSection title="Quick Actions">
          <div class="grid grid-cols-2 gap-3">
            {@render quick("Add Host", Plus, () => (ui.modal = { kind: "host", id: null }))}
            {@render quick("Add Credential", KeyRound, () => (ui.modal = { kind: "identity", id: null }))}
            {@render quick("Local Terminal", SquareTerminal, () => ui.openLocal())}
            {@render quick("Admin Settings", Settings, () => (ui.view = "settings"))}
            {@render quick("User Profile", CircleUser, () => (ui.view = "vault"))}
          </div>
        </PageSection>

        <PageSection title="Recent Activity" icon={Terminal}>
          {#snippet action()}
            <button class="btn-ghost py-0.5 text-xs" title="The full connection log is on the Vault screen" onclick={() => (ui.view = "vault")}>History</button>
          {/snippet}
          <div class="rounded-lg border border-line bg-base p-3 font-mono text-xs" role="log" aria-label="Recent activity">
            {#if activity.length === 0}
              <p class="text-fg-muted">No sessions yet. Open a host and it shows up here.</p>
            {:else}
              <ul class="space-y-1.5">
                {#each activity as a (a.id)}
                  <li class="flex items-baseline gap-2">
                    <span aria-hidden="true" class="text-success">$</span>
                    <span class="min-w-0 truncate {a.failed ? 'text-danger' : ''}" title={a.failed ? "The connection failed" : undefined}>{a.host}</span>
                    <span class="shrink-0 text-fg-muted">{a.what}</span>
                    <span class="ml-auto shrink-0 pl-2 text-fg-muted">{a.ago}</span>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        </PageSection>
      </div>
    </div>
  </div>
</div>
