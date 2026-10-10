<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Activity, ChevronDown, ChevronRight, ExternalLink, Loader2, RefreshCw, Search } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fleetSections, type FleetFilter } from "$lib/fleetpanel";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo } from "$lib/types";

  const AUTO_REFRESH_MS = 30_000;

  let query = $state("");
  let filter = $state<FleetFilter>("all");
  let closed = $state(new Set<string>());
  let checking = $state(false);

  const sections = $derived(fleetSections(vaultStore.hosts, vaultStore.health, query, filter));
  const shownIds = $derived(sections.flatMap((s) => s.hosts.map((h) => h.id)));
  const total = $derived(sections.reduce((n, s) => n + s.hosts.length, 0));

  /** Like vaultStore.checkHealth, without its toast: unwelcome every 30 seconds. */
  async function quietCheck(ids: string[]) {
    if (!ids.length) return;
    const results = await api.health.check(ids);
    const at = Date.now();
    for (const r of results) {
      const { host_id, ...h } = r;
      vaultStore.health[host_id] = { ...h, at };
    }
  }

  async function checkNow() {
    checking = true;
    try {
      await quietCheck(vaultStore.hosts.filter((h) => h.data).map((h) => h.id));
    } finally {
      checking = false;
    }
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    // Hosts that were never checked would all show as grey; look once when the panel opens.
    const unchecked = vaultStore.hosts.filter((h) => h.data && !vaultStore.health[h.id]).map((h) => h.id);
    void quietCheck(unchecked).catch(() => {});
    timer = setInterval(() => void quietCheck(shownIds).catch(() => {}), AUTO_REFRESH_MS);
  });
  onDestroy(() => clearInterval(timer));

  function toggle(name: string) {
    const next = new Set(closed);
    if (!next.delete(name)) next.add(name);
    closed = next;
  }

  const dot = (id: string) => {
    const h = vaultStore.health[id];
    return !h ? "bg-fg-muted/40" : h.state === "up" ? "bg-success" : h.state === "down" ? "bg-danger" : "bg-warning";
  };
  const FILTERS: { value: FleetFilter; label: string }[] = [
    { value: "all", label: "All" },
    { value: "up", label: "Up" },
    { value: "down", label: "Down" },
  ];
</script>

<aside class="flex min-w-0 flex-1 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between gap-1 px-4 pt-4 pb-2 @max-[16rem]:px-3">
    <h2 class="flex min-w-0 items-center gap-1.5 truncate text-sm font-semibold"><Activity size={15} class="shrink-0 text-accent" /> Fleet</h2>
    <div class="flex shrink-0">
      <button class="icon-btn" title="Check which hosts are reachable" aria-label="Check now" disabled={checking} onclick={checkNow}>
        {#if checking}<Loader2 size={16} class="animate-spin text-accent" />{:else}<RefreshCw size={16} />{/if}
      </button>
      <button class="icon-btn" title="Open the full Fleet page, with live metrics" aria-label="Open the full Fleet page" onclick={() => (ui.view = "fleet")}>
        <ExternalLink size={16} />
      </button>
    </div>
  </div>

  <div class="px-3 pb-2">
    <div class="relative">
      <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input py-1.5 pl-8" placeholder="Search hosts, groups or addresses" aria-label="Search the fleet" bind:value={query} />
    </div>
    <div class="mt-1.5 flex gap-1 rounded-lg border border-line bg-base p-0.5 text-[11px]" role="group" aria-label="Filter by status">
      {#each FILTERS as f (f.value)}
        <button
          type="button"
          class="min-w-0 flex-1 truncate rounded-md px-1.5 py-1 font-medium transition-colors {filter === f.value ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          aria-pressed={filter === f.value}
          onclick={() => (filter = f.value)}>{f.label}</button
        >
      {/each}
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
    {#if vaultStore.hosts.length === 0}
      <EmptyState art="hosts" text="No hosts yet." />
    {:else if total === 0}
      <p class="px-2 py-6 text-center text-xs text-fg-muted">No hosts match.</p>
    {:else}
      {#each sections as s (s.name)}
        {@const folded = !query.trim() && closed.has(s.name)}
        <section class="mb-1">
          <button
            class="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1.5 text-left text-xs font-semibold text-fg-muted hover:bg-panel-hover hover:text-fg"
            aria-expanded={!folded}
            onclick={() => toggle(s.name)}
          >
            {#if folded}<ChevronRight size={13} class="shrink-0" />{:else}<ChevronDown size={13} class="shrink-0" />{/if}
            <span class="min-w-0 flex-1 truncate" title={s.name}>{s.name}</span>
            <span class="shrink-0 font-mono text-[10px] font-normal">
              {#if s.down}<span class="text-danger">{s.down} down</span> · {/if}{s.up}/{s.hosts.length}
            </span>
          </button>
          {#if !folded}
            {#each s.hosts as rec (rec.id)}
              {@const d = rec.data!}
              {@const health = vaultStore.health[rec.id]}
              {@const reading = hostMetrics.readings[rec.id]}
              {@const env = envInfo(vaultStore.effectiveEnv(d))}
              <button
                class="flex w-full items-center gap-2 rounded-md py-1.5 pl-5 pr-2 text-left hover:bg-panel-hover"
                title={health?.state === "down" ? `Unreachable: ${health.reason}` : d.hostname}
                onclick={() => ui.openTerminal(rec.id, d.label)}
              >
                <span class="h-2 w-2 shrink-0 rounded-full {dot(rec.id)}"></span>
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-sm">{d.label}</span>
                  <span class="block truncate text-[11px] text-fg-muted">
                    {d.hostname}{#if hostMetrics.isMonitored(rec.id) && reading?.metrics}{@const m = reading.metrics}
                      {#if m.cpuPct != null} · {m.cpuPct}% cpu{/if}{#if m.memPct != null} · {m.memPct}% mem{/if}
                    {/if}
                  </span>
                </span>
                {#if env.value === "production"}<span class="shrink-0 rounded bg-danger/15 px-1 text-[9px] font-semibold text-danger">{env.short}</span>{/if}
                {#if health?.state === "up"}
                  <span class="shrink-0 font-mono text-[11px] text-success">{health.latency_ms}ms</span>
                {:else if health?.state === "down"}
                  <span class="shrink-0 font-mono text-[11px] text-danger">down</span>
                {/if}
              </button>
            {/each}
          {/if}
        </section>
      {/each}
    {/if}
  </div>
</aside>
