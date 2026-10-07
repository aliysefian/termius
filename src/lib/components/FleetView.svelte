<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Activity, Gauge, Loader2, RefreshCw, Server } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import { buildTree, groupPaths } from "$lib/tree";
  import { formatUptime } from "$lib/hostmetrics";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo } from "$lib/types";

  const AUTO_REFRESH_MS = 30_000;

  const tree = $derived(buildTree(vaultStore.hosts));
  const groups = $derived(groupPaths(tree).sort());
  // svelte-ignore state_referenced_locally
  let groupFilter = $state(ui.fleetGroup);

  const shown = $derived(
    vaultStore.hosts.filter((h) => h.data && (!groupFilter || h.data.group === groupFilter || h.data.group.startsWith(`${groupFilter}/`))),
  );

  let autoRefresh = $state(true);
  let checking = $state(false);

  /** Like vaultStore.checkHealth, but without its "N of M reachable" toast — unwelcome every 30 seconds. */
  async function quietHealthCheck(ids: string[]) {
    if (!ids.length) return;
    const results = await api.health.check(ids);
    const at = Date.now();
    for (const r of results) {
      const { host_id, ...h } = r;
      vaultStore.health[host_id] = { ...h, at };
    }
  }

  async function checkNow() {
    const ids = shown.map((h) => h.id);
    if (!ids.length) return;
    checking = true;
    try {
      await vaultStore.checkHealth(ids);
    } finally {
      checking = false;
    }
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    ui.fleetGroup = ""; // one-shot: don't keep pre-selecting a group on later visits
    timer = setInterval(() => {
      if (autoRefresh) void quietHealthCheck(shown.map((h) => h.id));
    }, AUTO_REFRESH_MS);
  });
  onDestroy(() => clearInterval(timer));

  function dotClass(id: string): string {
    const h = vaultStore.health[id];
    if (!h) return "bg-fg-muted/40";
    return h.state === "up" ? "bg-success" : h.state === "down" ? "bg-danger" : "bg-warning";
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-5xl space-y-5">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="flex items-center gap-2 text-lg font-semibold"><Activity size={20} class="text-accent" /> Fleet</h1>
        <p class="mt-1 text-sm text-fg-muted">Reachability for every host shown, and live metrics for the ones you've turned monitoring on for.</p>
      </div>
      <div class="flex items-center gap-2">
        <select class="input w-48" bind:value={groupFilter}>
          <option value="">All groups</option>
          {#each groups as g (g)}<option value={g}>{g}</option>{/each}
        </select>
        <label class="flex items-center gap-1.5 text-xs text-fg-muted">
          <input type="checkbox" class="accent-input" bind:checked={autoRefresh} /> Auto-refresh
        </label>
        <button class="btn-secondary py-1.5 text-xs" disabled={checking} onclick={checkNow}>
          {#if checking}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Check now
        </button>
      </div>
    </div>

    {#if shown.length === 0}
      <div class="rounded-xl border border-line bg-panel p-8">
        <EmptyState art="hosts" text={groupFilter ? "No hosts in this group." : "No hosts yet."} />
      </div>
    {:else}
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
        {#each shown as rec (rec.id)}
          {@const d = rec.data!}
          {@const env = envInfo(vaultStore.effectiveEnv(d))}
          {@const health = vaultStore.health[rec.id]}
          {@const monitored = hostMetrics.isMonitored(rec.id)}
          {@const reading = hostMetrics.readings[rec.id]}
          <div class="rounded-lg border border-line bg-panel p-3 {env.value === 'production' ? 'ring-1 ring-danger/30' : ''}">
            <button class="flex w-full items-center gap-2 text-left" onclick={() => ui.openTerminal(rec.id, d.label)}>
              <span class="h-2 w-2 shrink-0 rounded-full {dotClass(rec.id)}" title={health?.state ?? "not checked"}></span>
              <span class="min-w-0 flex-1 truncate text-sm font-medium">{d.label}</span>
            </button>
            <div class="mt-0.5 truncate pl-4 text-xs text-fg-muted">{d.hostname}</div>
            {#if health?.state === "up"}
              <div class="pl-4 text-[11px] text-fg-muted">{health.latency_ms} ms</div>
            {:else if health?.state === "down"}
              <div class="truncate pl-4 text-[11px] text-danger" title={health.reason}>{health.reason}</div>
            {/if}
            <div class="mt-2 flex items-center justify-between gap-2 border-t border-line pt-2">
              {#if monitored && reading?.metrics}
                {@const m = reading.metrics}
                <div class="flex gap-2 font-mono text-[11px] text-fg-muted">
                  {#if m.cpuPct != null}<span title="CPU">{m.cpuPct}%c</span>{/if}
                  {#if m.memPct != null}<span title="Memory">{m.memPct}%m</span>{/if}
                  {#if m.diskPct != null}<span title="Disk">{m.diskPct}%d</span>{/if}
                  {#if m.uptimeSecs != null}<span title="Uptime">{formatUptime(m.uptimeSecs)}</span>{/if}
                </div>
              {:else if monitored}
                <span class="text-[11px] text-fg-muted">Waiting…</span>
              {:else}
                <span class="text-[11px] text-fg-muted">Not monitored</span>
              {/if}
              <button
                class="icon-btn h-5 w-5 shrink-0"
                title={hostMetrics.canMonitor(rec.id) ? "Open the detail view: network, processes, ports, interfaces" : "Needs saved credentials"}
                aria-label="Open the detail view of {d.label}"
                disabled={!hostMetrics.canMonitor(rec.id)}
                onclick={() => (ui.modal = { kind: "host-monitor", id: rec.id })}
              >
                <Gauge size={12} />
              </button>
              <button
                class="icon-btn h-5 w-5 shrink-0 {monitored ? 'text-accent' : ''}"
                title={monitored ? "Stop monitoring" : hostMetrics.canMonitor(rec.id) ? "Start monitoring" : "Needs saved credentials"}
                disabled={!monitored && !hostMetrics.canMonitor(rec.id)}
                onclick={() => hostMetrics.setMonitored(rec.id, !monitored)}
              >
                <Activity size={12} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
