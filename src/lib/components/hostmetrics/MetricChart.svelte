<script lang="ts">
  import type { Frame } from "$lib/hostframes";
  import { FRAME_GAP_MS, fitWindow, peak } from "$lib/hostframes";
  import { agoLabel, areaOf, chartSegments, clock, nearestIndex, niceMax, pathOf, stepsFor, ticks, type Box } from "$lib/hosthistory";
  import { pctText, rateText, type RateUnit } from "$lib/hostsample";

  interface Series {
    label: string;
    /** 1 is the first categorical colour, 2 the second. */
    slot: 1 | 2;
    pick: (f: Frame) => number | null;
  }

  let {
    title,
    frames,
    series,
    unit,
    now,
    windowMs: fullWindow,
    rateUnit = "bytes",
    height = 150,
    empty = "Collecting readings…",
    peaks = false,
    gapMs = FRAME_GAP_MS,
  }: { title: string; frames: Frame[]; series: Series[]; unit: "percent" | "rate"; now: number; windowMs: number; rateUnit?: RateUnit; height?: number; empty?: string; peaks?: boolean; gapMs?: number } = $props();

  const PAD = { left: 62, right: 10, top: 8, bottom: 22 };
  let width = $state(480);
  let hover = $state<number | null>(null);

  const plotW = $derived(Math.max(40, width - PAD.left - PAD.right));
  const plotH = $derived(height - PAD.top - PAD.bottom);
  // While there is less to show than the range asks for, the chart covers what there is (at least a minute), so a window
  // that has just opened is a chart, not a dot at the edge of 15 empty minutes. It widens as readings come in.
  const windowMs = $derived(fitWindow(frames, now, fullWindow));
  const shown = $derived(frames.filter((f) => f.t >= now - windowMs));
  // Percent charts run 0 to 100. Rates scale to their own tidy maximum, never below 1 kB/s so a quiet line is not noise.
  const max = $derived(unit === "percent" ? 100 : niceMax(Math.max(0, ...shown.flatMap((f) => series.map((s) => s.pick(f) ?? 0))), 1000));
  const box = $derived<Box>({ width: plotW, height: plotH, from: now - windowMs, to: now, max });
  const lines = $derived(series.map((s) => ({ ...s, segments: chartSegments(shown, s.pick, box, gapMs) })));
  const yTicks = $derived(ticks(max, unit === "percent" ? 4 : stepsFor(max)));
  const hasData = $derived(lines.some((l) => l.segments.length > 0));
  const fmt = (v: number | null) => (unit === "percent" ? pctText(v) : rateText(v, rateUnit));
  const axis = (v: number) => (unit === "percent" ? `${Math.round(v)}%` : rateText(v, rateUnit));
  const latest = $derived(shown[shown.length - 1]);

  // The crosshair snaps to the nearest reading; the pointer never has to land on a line.
  const cursor = $derived(hover !== null && shown[hover] ? shown[hover] : null);
  const cursorX = $derived(cursor ? ((cursor.t - box.from) / (box.to - box.from)) * plotW : 0);
  const flip = $derived(cursorX > plotW * 0.6);

  function move(e: PointerEvent & { currentTarget: SVGRectElement }) {
    const r = e.currentTarget.getBoundingClientRect();
    const t = box.from + ((e.clientX - r.left) / r.width) * (box.to - box.from);
    const i = nearestIndex(shown, t);
    hover = i >= 0 && Math.abs(shown[i].t - t) < Math.max(90_000, gapMs * 2) ? i : null;
  }

  function key(e: KeyboardEvent) {
    if (!shown.length) return;
    const last = shown.length - 1;
    const at = hover ?? last;
    if (e.key === "ArrowLeft") hover = Math.max(0, at - 1);
    else if (e.key === "ArrowRight") hover = Math.min(last, at + 1);
    else if (e.key === "Home") hover = 0;
    else if (e.key === "End") hover = last;
    else if (e.key === "Escape") {
      if (hover === null) return;
      hover = null;
      e.stopPropagation();
    } else return;
    e.preventDefault();
  }

  const summary = $derived(`${title}. ${series.map((s) => `${s.label} ${fmt(latest ? s.pick(latest) : null)}`).join(", ")}. Arrow keys move through readings.`);
</script>

<figure class="metricchart rounded-lg border border-line bg-panel p-3">
  <figcaption class="mb-1 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
    <span class="text-sm font-semibold">{title}</span>
    <span class="flex flex-wrap items-center gap-x-4 gap-y-0.5 text-xs">
      {#each series as s (s.label)}
        <span class="flex items-center gap-1.5">
          {#if series.length > 1}<span class="key" style="background: var(--series-{s.slot})" aria-hidden="true"></span><span class="text-fg-muted">{s.label}</span>{/if}
          <span class="font-semibold tabular-nums">{fmt(latest ? s.pick(latest) : null)}</span>
          {#if peaks}<span class="text-fg-muted">peak <span class="tabular-nums">{fmt(peak(shown, s.pick))}</span></span>{/if}
        </span>
      {/each}
    </span>
  </figcaption>

  <div class="relative" style="height: {height}px" bind:clientWidth={width}>
    {#if !hasData}
      <p class="absolute inset-0 flex items-center justify-center px-6 text-center text-xs text-fg-muted">{empty}</p>
    {/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <svg {width} {height} role="img" aria-label={summary} tabindex="0" onkeydown={key} onblur={() => (hover = null)} class="block overflow-visible outline-none focus-visible:ring-1 focus-visible:ring-accent/50">
      <g transform="translate({PAD.left} {PAD.top})">
        {#each yTicks as t, i (i)}
          {@const y = plotH - (t / max) * plotH}
          <line x1="0" x2={plotW} y1={y} y2={y} class="grid" />
          <text x="-8" {y} dy="0.32em" text-anchor="end" class="axis">{axis(t)}</text>
        {/each}
        {#each [1, 2 / 3, 1 / 3, 0] as f (f)}
          <text x={plotW - f * plotW} y={plotH + 16} text-anchor={f === 0 ? "end" : f === 1 ? "start" : "middle"} class="axis">{agoLabel(f * windowMs)}</text>
        {/each}

        {#if lines.length === 1 && lines[0].segments.length}
          {#each lines[0].segments as seg, i (i)}<path d={areaOf(seg, plotH)} fill="var(--series-{lines[0].slot})" fill-opacity="0.1" />{/each}
        {/if}
        {#each lines as l (l.label)}
          {#each l.segments as seg, i (i)}
            <path d={pathOf(seg)} fill="none" stroke="var(--series-{l.slot})" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
          {/each}
          {#if l.segments.length}
            {@const end = l.segments[l.segments.length - 1][l.segments[l.segments.length - 1].length - 1]}
            <circle cx={end.x} cy={end.y} r="4" fill="var(--series-{l.slot})" stroke="var(--surface)" stroke-width="2" />
          {/if}
        {/each}

        {#if cursor}
          <line x1={cursorX} x2={cursorX} y1="0" y2={plotH} class="cross" />
          {#each lines as l (l.label)}
            {@const v = l.pick(cursor)}
            {#if v !== null}<circle cx={cursorX} cy={plotH - Math.min(1, v / max) * plotH} r="4" fill="var(--series-{l.slot})" stroke="var(--surface)" stroke-width="2" />{/if}
          {/each}
        {/if}

        <rect role="presentation" x="0" y="0" width={plotW} height={plotH} fill="transparent" onpointermove={move} onpointerleave={() => (hover = null)} />
      </g>
    </svg>

    {#if cursor}
      <div class="tip" style="top: {PAD.top}px; {flip ? `right: ${width - PAD.left - cursorX + 10}px` : `left: ${PAD.left + cursorX + 10}px`}" role="status">
        <div class="mb-1 text-[11px] text-fg-muted">{windowMs > 3_600_000 ? new Date(cursor.t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : clock(cursor.t)}</div>
        {#each series as s (s.label)}
          <div class="flex items-center gap-2">
            <span class="key" style="background: var(--series-{s.slot})" aria-hidden="true"></span>
            <span class="font-semibold tabular-nums">{fmt(s.pick(cursor))}</span>
            {#if series.length > 1}<span class="text-fg-muted">{s.label}</span>{/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</figure>

<style>
  .metricchart {
    --surface: #20222b;
    --series-1: #3987e5;
    --series-2: #d95926;
    --grid: #2c2f3a;
    --text-muted: #8b8d98;
  }
  :global(:root[data-theme="light"]) .metricchart {
    --surface: #ffffff;
    --series-1: #2a78d6;
    --series-2: #eb6834;
    --grid: #dcdfe6;
    --text-muted: #5c6170;
  }
  .grid {
    stroke: var(--grid);
    stroke-width: 1;
  }
  .cross {
    stroke: var(--text-muted);
    stroke-width: 1;
  }
  .axis {
    fill: var(--text-muted);
    font-size: 10px;
  }
  .key {
    display: inline-block;
    width: 14px;
    height: 2px;
    border-radius: 1px;
    flex: none;
  }
  .tip {
    position: absolute;
    pointer-events: none;
    z-index: 5;
    min-width: 7rem;
    padding: 6px 8px;
    font-size: 12px;
    border: 1px solid var(--grid);
    border-radius: 6px;
    background: var(--surface);
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.25);
  }
</style>
