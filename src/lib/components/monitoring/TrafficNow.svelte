<script lang="ts">
  // Who is moving the most data right now: each host's main interface, as bars on one shared scale. Hosts whose
  // counters couldn't be read are left out rather than drawn as empty.
  import { ArrowDown, ArrowUp } from "lucide-svelte";
  import { formatRate, type RateUnit } from "$lib/netdata";
  import type { HostRow } from "$lib/monitoringview";

  let { rows, unit, onpick }: { rows: HostRow[]; unit: RateUnit; onpick: (id: string) => void } = $props();

  const talkers = $derived(
    rows
      .filter((r) => r.net && (r.net.rx !== null || r.net.tx !== null))
      .sort((a, b) => (b.net!.rx ?? 0) + (b.net!.tx ?? 0) - ((a.net!.rx ?? 0) + (a.net!.tx ?? 0)))
      .slice(0, 5),
  );
  const top = $derived(Math.max(1, ...talkers.flatMap((r) => [r.net!.rx ?? 0, r.net!.tx ?? 0])));
</script>

{#if talkers.length}
  <section class="rounded-xl border border-line bg-panel p-3" aria-label="Traffic now">
    <h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Traffic now · busiest hosts</h2>
    <ul class="space-y-1.5">
      {#each talkers as r (r.id)}
        <li>
          <button class="grid w-full grid-cols-[8rem_1fr_1fr] items-center gap-3 rounded-md px-1.5 py-1 text-left hover:bg-panel-hover max-md:grid-cols-[6rem_1fr_1fr]" onclick={() => onpick(r.id)} title="Open {r.target.label}">
            <span class="truncate text-sm">{r.target.label}</span>
            {#each [["rx", r.net!.rx, "#38bdf8", ArrowDown], ["tx", r.net!.tx, "#a78bfa", ArrowUp]] as [k, v, color, Icon] (k)}
              <span class="flex items-center gap-2">
                <span class="h-1.5 flex-1 overflow-hidden rounded-full bg-fg-muted/10"><span class="block h-full rounded-full" style="width:{v === null ? 0 : Math.max(1.5, ((v as number) / top) * 100)}%; background:{color}"></span></span>
                <span class="w-20 text-right font-mono text-[11px] tabular-nums">{formatRate(v as number | null, unit).value} <span class="text-fg-muted">{formatRate(v as number | null, unit).unit}</span></span>
              </span>
            {/each}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}
