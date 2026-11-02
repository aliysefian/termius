<script lang="ts">
  // Download and upload of one interface over a time range, drawn from the readings that were actually taken: each
  // column shows the peak of its readings (so a burst is never averaged away) and a hole in the data is a hole.
  import { bucketPeaks, formatRate, nearest, niceMax, paths, peak, ticks, type NetPoint, type RateUnit } from "$lib/netdata";

  let { points, from, to, unit, empty, short }: {
    points: NetPoint[];
    /** The time range drawn, in ms since the epoch. */
    from: number;
    to: number;
    unit: RateUnit;
    /** What to say when there is nothing to draw. */
    empty: string;
    /** Label times as HH:MM:SS (a short range) rather than HH:MM or a date. */
    short: boolean;
  } = $props();

  const HEIGHT = 190;
  const LEFT = 58;
  let width = $state(640);
  let hover = $state<number | null>(null);

  const plotW = $derived(Math.max(120, width - LEFT - 8));
  const cols = $derived(bucketPeaks(points, from, to, Math.max(20, Math.floor(plotW / 3))));
  const top = $derived(peak(points, from, to));
  const max = $derived(niceMax(Math.max(top.rx ?? 0, top.tx ?? 0) * 1.1));
  const box = $derived({ width: plotW, height: HEIGHT, from, to, max });
  const rx = $derived(paths(cols, (c) => c.rx, box));
  const tx = $derived(paths(cols, (c) => c.tx, box));
  const any = $derived(cols.some((c) => c.rx !== null || c.tx !== null));
  const at = $derived(hover === null ? null : nearest(cols, from + (hover / plotW) * (to - from)));
  const clock = (t: number) => new Date(t).toLocaleTimeString([], short ? { hour: "2-digit", minute: "2-digit", second: "2-digit" } : { hour: "2-digit", minute: "2-digit" });
  const day = (t: number) => new Date(t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  const label = (t: number) => (to - from > 36 * 3_600_000 ? day(t) : clock(t));

  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    hover = Math.min(plotW, Math.max(0, e.clientX - r.left));
  }
</script>

<figure class="space-y-2" bind:clientWidth={width}>
  <figcaption class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-fg-muted">
    <span class="flex items-center gap-1.5"><span class="h-0.5 w-4 rounded" style="background:#38bdf8"></span> Download / RX</span>
    <span class="flex items-center gap-1.5"><span class="h-0.5 w-4 rounded" style="background:#a78bfa"></span> Upload / TX</span>
    <span class="ml-auto">peak per column</span>
  </figcaption>

  <div class="relative" style="height:{HEIGHT + 22}px">
    {#if !any}
      <div class="absolute inset-0 flex items-center justify-center rounded-[10px] border border-line bg-base/40 px-6 text-center text-sm text-fg-muted">{empty}</div>
    {:else}
      <!-- Axis labels -->
      {#each ticks(max, 4) as v, i (i)}
        <div class="pointer-events-none absolute right-0 border-t border-line/50 text-[10px] tabular-nums text-fg-muted" style="top:{HEIGHT - (v / max) * HEIGHT}px; left:{LEFT}px">
          <span class="absolute -top-2 -translate-x-full pr-1.5" style="left:-4px">{formatRate(v, unit).value === "–" ? "" : `${formatRate(v, unit).value} ${formatRate(v, unit).unit}`}</span>
        </div>
      {/each}
      <svg class="absolute top-0 touch-none" style="left:{LEFT}px" width={plotW} height={HEIGHT} role="img" aria-label="Download and upload speed over time" onpointermove={move} onpointerleave={() => (hover = null)}>
        {#if rx.area}<path d={rx.area} fill="#38bdf8" fill-opacity="0.14" />{/if}
        {#if tx.area}<path d={tx.area} fill="#a78bfa" fill-opacity="0.14" />{/if}
        <path d={rx.line} fill="none" stroke="#38bdf8" stroke-width="1.6" stroke-linejoin="round" />
        <path d={tx.line} fill="none" stroke="#a78bfa" stroke-width="1.6" stroke-linejoin="round" />
        {#if at}
          {@const x = ((at.t - from) / (to - from)) * plotW}
          <line x1={x} x2={x} y1="0" y2={HEIGHT} stroke="currentColor" stroke-opacity="0.35" stroke-dasharray="3 3" />
          {#if at.rx !== null}<circle cx={x} cy={HEIGHT - (Math.min(at.rx, max) / max) * HEIGHT} r="3.5" fill="#38bdf8" />{/if}
          {#if at.tx !== null}<circle cx={x} cy={HEIGHT - (Math.min(at.tx, max) / max) * HEIGHT} r="3.5" fill="#a78bfa" />{/if}
        {/if}
      </svg>
      <div class="pointer-events-none absolute flex justify-between text-[10px] tabular-nums text-fg-muted" style="top:{HEIGHT + 4}px; left:{LEFT}px; width:{plotW}px">
        <span>{label(from)}</span><span>{label(from + (to - from) / 2)}</span><span>{label(to)}</span>
      </div>
      {#if at}
        {@const x = ((at.t - from) / (to - from)) * plotW}
        <div class="pointer-events-none absolute z-10 rounded-lg border border-line bg-panel px-2.5 py-1.5 text-xs shadow-lg" style="top:6px; left:{LEFT + Math.min(plotW - 150, Math.max(0, x + 10))}px">
          <div class="mb-0.5 text-[11px] text-fg-muted">{day(at.t)}</div>
          <div class="flex items-center gap-1.5 tabular-nums"><span class="h-2 w-2 rounded-full" style="background:#38bdf8"></span>RX {at.rx === null ? "–" : `${formatRate(at.rx, unit).value} ${formatRate(at.rx, unit).unit}`}</div>
          <div class="flex items-center gap-1.5 tabular-nums"><span class="h-2 w-2 rounded-full" style="background:#a78bfa"></span>TX {at.tx === null ? "–" : `${formatRate(at.tx, unit).value} ${formatRate(at.tx, unit).unit}`}</div>
        </div>
      {/if}
    {/if}
  </div>
</figure>
