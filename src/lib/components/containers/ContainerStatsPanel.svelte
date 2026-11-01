<script lang="ts">
  // CPU, memory, network and disk of one container, from the readings the runtime gives with each refresh. Charts
  // appear once there are enough readings to draw; a figure the runtime doesn't give is shown as unavailable.
  import { Activity } from "lucide-svelte";
  import { formatBytes, toSample, type Sample } from "$lib/containerview";
  import { containers } from "$lib/stores/containers.svelte";
  import type { ContainerInfo } from "$lib/types";

  let { sourceKey, c }: { sourceKey: string; c: ContainerInfo } = $props();

  const src = $derived(containers.sources[sourceKey]);
  const live = $derived(c.state === "running");
  const reading = $derived(src ? (src.stats[c.id] ?? src.stats[c.id.slice(0, 12)]) : undefined);
  const now = $derived(reading ? toSample(reading, 0) : null);
  const history = $derived(containers.history(sourceKey, c.id));
  const MIN_POINTS = 4;

  /** An SVG polyline for a series, scaled to its own maximum (or a fixed ceiling). */
  function line(values: (number | null)[], ceiling?: number): string {
    const pts = values.map((v, i) => [i, v] as const).filter((p): p is readonly [number, number] => p[1] !== null);
    if (pts.length < 2) return "";
    const max = ceiling ?? Math.max(1, ...pts.map((p) => p[1]));
    const n = values.length - 1 || 1;
    return pts.map(([i, v]) => `${((i / n) * 100).toFixed(1)},${(30 - (Math.min(v, max) / max) * 28 - 1).toFixed(1)}`).join(" ");
  }
  const cpuSeries = $derived(history.map((s: Sample) => s.cpu));
  const memSeries = $derived(history.map((s: Sample) => s.memPct));
  const peakCpu = $derived(Math.max(100, ...cpuSeries.map((v) => v ?? 0)));
</script>

{#if !src?.wantStats}
  <div class="rounded-[10px] border border-line bg-base/50 p-5 text-center">
    <Activity size={22} class="mx-auto text-accent" aria-hidden="true" />
    <p class="mt-2 text-sm font-medium">CPU and memory monitoring is off</p>
    <p class="mx-auto mt-1 max-w-xs text-xs text-fg-muted">Turning it on reads every running container's usage with each refresh: one more command, about a second, on this host.</p>
    <button class="btn-primary mt-3" onclick={() => containers.setStats(sourceKey, true)}>Turn on monitoring</button>
  </div>
{:else if !live}
  <p class="rounded-md border border-line bg-base/50 px-3 py-4 text-sm text-fg-muted">This container isn't running, so it has no usage to show.</p>
{:else if !now}
  <p class="rounded-md border border-line bg-base/50 px-3 py-4 text-sm text-fg-muted" role="status">{src.statsError ? `The runtime couldn't give stats: ${src.statsError}` : "Waiting for the first reading…"}</p>
{:else}
  <div class="space-y-4">
    <div class="grid grid-cols-2 gap-2">
      {#each [
        ["CPU", now.cpu === null ? "Unavailable" : `${now.cpu.toFixed(1)}%`, "of one core"],
        ["Memory", now.memUsed === null ? "Unavailable" : formatBytes(now.memUsed), now.memLimit ? `of ${formatBytes(now.memLimit)}${now.memPct !== null ? ` · ${now.memPct.toFixed(1)}%` : ""}` : "no limit reported"],
        ["Network in", formatBytes(now.netRx), "received so far"],
        ["Network out", formatBytes(now.netTx), "sent so far"],
        ["Disk read", formatBytes(now.blockRead), "since it started"],
        ["Disk write", formatBytes(now.blockWrite), "since it started"],
        ["Processes", now.pids === null ? "Unavailable" : String(now.pids), ""],
      ] as [string, string, string][] as [k, v, sub] (k)}
        <div class="rounded-[10px] border border-line bg-base/50 px-3 py-2">
          <div class="text-[11px] uppercase tracking-wide text-fg-muted">{k}</div>
          <div class="mt-0.5 font-mono text-[15px] tabular-nums {v === 'Unavailable' ? 'text-fg-muted' : ''}">{v}</div>
          {#if sub}<div class="text-[11px] text-fg-muted">{sub}</div>{/if}
        </div>
      {/each}
    </div>

    {#if history.length >= MIN_POINTS}
      <div class="space-y-3">
        {#each [["CPU %", cpuSeries, peakCpu], ["Memory %", memSeries, 100]] as [title, series, ceiling] (title)}
          {@const path = line(series as (number | null)[], ceiling as number)}
          {#if path}
            <figure>
              <figcaption class="mb-1 flex justify-between text-[11px] text-fg-muted"><span>{title}</span><span>last {history.length} readings</span></figcaption>
              <svg viewBox="0 0 100 30" preserveAspectRatio="none" class="h-16 w-full rounded-md border border-line bg-base/50" role="img" aria-label="{title} over the last {history.length} readings">
                <polyline points={path} fill="none" stroke="var(--color-accent)" stroke-width="1.2" vector-effect="non-scaling-stroke" />
              </svg>
            </figure>
          {/if}
        {/each}
      </div>
    {:else}
      <p class="text-[11px] text-fg-muted">A chart appears after a few readings.</p>
    {/if}
    <p class="text-[11px] text-fg-muted">Read {containers.paused ? "when you refresh" : `every ${containers.intervalSecs} s`}. Network and disk figures are totals since the container started. Up to the last 60 readings are kept.</p>
  </div>
{/if}
