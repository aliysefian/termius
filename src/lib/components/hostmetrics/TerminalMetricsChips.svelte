<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowDown, ArrowUp, Gauge } from "lucide-svelte";
  import MiniBar from "./MiniBar.svelte";
  import { isStale, rateText } from "$lib/hostsample";
  import { mainFilesystem } from "$lib/hostview";
  import { loadPrefs as loadView } from "$lib/hostviewprefs";
  import { hostSamples } from "$lib/stores/hostsamples.svelte";
  import { terminalMetrics as tm } from "$lib/stores/terminalmetrics.svelte";

  const unit = loadView().unit;
  let now = $state(Date.now());
  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 2000);
    return () => clearInterval(t);
  });

  const id = $derived(tm.hostId);
  const s = $derived(id ? (hostSamples.latest[id] ?? null) : null);
  const error = $derived(id ? (hostSamples.errors[id] ?? null) : null);
  const at = $derived(id ? hostSamples.receivedAt[id] : undefined);
  const stale = $derived(at !== undefined && isStale(now - at, tm.every ?? 5000));
  const linux = $derived(s?.platform.kind === "linux");
  const fs = $derived(s ? mainFilesystem(s.filesystems) : null);
  const total = $derived(s?.network?.total ?? null);
  const chip = "flex h-6 items-center gap-1.5 rounded-md px-1.5 hover:bg-panel-hover hover:text-fg";
</script>

{#if id}
  <button class="{chip} ml-1 {tm.choice?.panel ? 'bg-panel-hover text-accent' : ''}" aria-pressed={!!tm.choice?.panel} aria-label="Host metrics panel" title={tm.choice?.panel ? "Close the host metrics panel" : "Show host metrics beside the terminal (reads this host over a second connection)"} onclick={() => tm.togglePanel()} data-testid="term-metrics-toggle">
    <Gauge size={14} />
  </button>
  {#if tm.toolbarOn}
    <span class="flex items-center gap-3 text-[11px]" role="group" aria-label="Host resources{stale ? ' (not current)' : ''}" data-testid="term-metrics-chips" title={stale ? "The last reading is old" : "Read over a second connection; nothing is installed on the host"}>
      {#if s && linux}
        <MiniBar label="CPU" pct={s.cpu?.total_pct ?? null} {stale} />
        <MiniBar label="RAM" pct={s.memory?.used_pct ?? null} {stale} />
        <MiniBar label="Disk" pct={fs?.used_pct ?? null} {stale} />
        {#if total}
          <span class="hidden items-center gap-1.5 tabular-nums md:flex {stale ? 'opacity-50' : ''}"><span class="flex items-center"><ArrowDown size={11} />{rateText(total.rx_bps, unit)}</span><span class="flex items-center"><ArrowUp size={11} />{rateText(total.tx_bps, unit)}</span></span>
        {/if}
      {:else if s}
        <span class="text-fg-muted">Metrics are not available for this system</span>
      {:else if error}
        <span class="max-w-[40ch] truncate text-warning" title={error}>Metrics unavailable: {error}</span>
      {:else}
        <span class="text-fg-muted">Reading…</span>
      {/if}
    </span>
  {/if}
{/if}
