<script lang="ts">
  import type { Frame } from "$lib/hostframes";
  import { memText, pctText, type HostSample } from "$lib/hostsample";
  import { memoryBar } from "$lib/hostview";
  import MetricChart from "./MetricChart.svelte";
  import UsageBar from "./UsageBar.svelte";

  let { s, frames, now, windowMs }: { s: HostSample; frames: Frame[]; now: number; windowMs: number } = $props();
  const m = $derived(s.memory);
  const bar = $derived(m ? memoryBar(m) : null);
  const swapPct = $derived(m && m.swap_total > 0 ? (m.swap_used * 100) / m.swap_total : null);
  const rows = $derived<[string, number | null, string?][]>(
    m
      ? [
          ["Total", m.total],
          ["Used", m.used, "Total minus available."],
          ["Available", m.available, m.available_exact ? "What a new program could get without swapping (the kernel's own figure)." : "An estimate: this kernel does not report it."],
          ["Free", m.free, "Not used for anything at all."],
          ["Cache", m.cached, "Page cache and reclaimable kernel memory, not counting shared memory."],
          ["Buffers", m.buffers],
          ["Shared", m.shared, "tmpfs and shared memory: it counts as used, not as cache."],
          ["Kernel slab", m.slab],
          ["Committed", m.committed, "Memory promised to programs, which can be more than exists."],
        ]
      : [],
  );
</script>

{#if !m || !bar}
  <p class="rounded-lg border border-line bg-panel p-4 text-sm text-fg-muted">This host does not report memory (<code>/proc/meminfo</code>).</p>
{:else}
  <div class="grid gap-3 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
    <div class="space-y-3">
      <div class="rounded-lg border border-line bg-panel p-4">
        <div class="flex flex-wrap items-baseline gap-x-4">
          <span class="text-4xl font-semibold tabular-nums">{pctText(m.used_pct)}</span>
          <span class="text-sm text-fg-muted">{memText(m.used)} of {memText(m.total)} used · {memText(m.available)} available</span>
        </div>
        <div class="mt-3 flex h-3 overflow-hidden rounded-full bg-line/60" role="img" aria-label="Memory: {pctText(m.used_pct)} in use, {memText(m.available)} available">
          <div class="bg-accent" style="width: {bar.used * 100}%"></div>
          <div style="width: {bar.reclaimable * 100}%; background: var(--series-1, #3987e5)"></div>
          <div class="bg-success/70" style="width: {bar.free * 100}%"></div>
        </div>
        <ul class="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs">
          <li class="flex items-center gap-1.5"><span class="h-2 w-2 rounded-full bg-accent"></span><span class="text-fg-muted">In use</span> <span class="tabular-nums">{memText(m.used)}</span></li>
          <li class="flex items-center gap-1.5"><span class="h-2 w-2 rounded-full" style="background: #3987e5"></span><span class="text-fg-muted">Cache and buffers (reclaimable)</span> <span class="tabular-nums">{memText(m.available - Math.min(m.free, m.available))}</span></li>
          <li class="flex items-center gap-1.5"><span class="h-2 w-2 rounded-full bg-success/70"></span><span class="text-fg-muted">Free</span> <span class="tabular-nums">{memText(Math.min(m.free, m.available))}</span></li>
        </ul>
        <div class="mt-4">
          <div class="mb-1 flex justify-between text-xs"><span class="font-medium">Swap</span><span class="tabular-nums text-fg-muted">{m.swap_total > 0 ? `${memText(m.swap_used)} / ${memText(m.swap_total)} (${pctText(swapPct)})` : "No swap configured"}</span></div>
          <UsageBar pct={swapPct} label="Swap" />
        </div>
      </div>
      <MetricChart title="Memory usage" {frames} {now} {windowMs} unit="percent" series={[{ label: "Memory", slot: 1, pick: (f) => f.mem }]} peaks />
    </div>
    <div class="self-start rounded-lg border border-line bg-panel p-3">
      <h3 class="mb-2 text-sm font-semibold">Details</h3>
      <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5 text-xs">
        {#each rows as [k, v, why] (k)}
          <dt class="text-fg-muted" title={why}>{k}</dt><dd class="tabular-nums" title={why}>{v === null ? "Unavailable" : memText(v)}</dd>
        {/each}
      </dl>
    </div>
  </div>
{/if}
