<script lang="ts">
  import { formatRate } from "$lib/hostdetail";
  import { GAP_MS, HISTORY_MS, areaOf, chartSegments, clock, nearestIndex, niceMax, pathOf, stepsFor, ticks, agoLabel, type Box, type Sample } from "$lib/hosthistory";

  type Key = "cpu" | "mem" | "rx" | "tx";
  interface Series {
    key: Key;
    label: string;
    /** 1 is the first categorical colour, 2 the second. */
    slot: 1 | 2;
  }

  let {
    title,
    samples,
    series,
    unit,
    now,
    empty = "Collecting readings…",
    windowMs = HISTORY_MS,
    gapMs = GAP_MS,
  }: { title: string; samples: Sample[]; series: Series[]; unit: "percent" | "rate"; now: number; empty?: string; windowMs?: number; gapMs?: number } = $props();

  const HEIGHT = 132;
  const PAD = { left: 52, right: 10, top: 8, bottom: 22 };
  let width = $state(480);
  let hover = $state<number | null>(null);

  const plotW = $derived(Math.max(40, width - PAD.left - PAD.right));
  const plotH = HEIGHT - PAD.top - PAD.bottom;
  const pick = (k: Key) => (s: Sample) => s[k];

  // Percent charts always run 0 to 100. Throughput scales to its own tidy maximum (never below 1 kB/s,
  // so a quiet line doesn't blow up into noise).
  const max = $derived(unit === "percent" ? 100 : niceMax(Math.max(0, ...samples.flatMap((s) => series.map((x) => s[x.key] ?? 0))), 1000));
  const box = $derived<Box>({ width: plotW, height: plotH, from: now - windowMs, to: now, max });
  const lines = $derived(series.map((s) => ({ ...s, segments: chartSegments(samples, pick(s.key), box, gapMs) })));
  const yTicks = $derived(ticks(max, unit === "percent" ? 4 : stepsFor(max)));
  const hasData = $derived(lines.some((l) => l.segments.length > 0));
  const fmt = (v: number | null) => (v === null ? "—" : unit === "percent" ? `${Math.round(v)}%` : formatRate(v));
  const latest = $derived(samples[samples.length - 1]);

  // The crosshair snaps to the nearest reading; the pointer never has to land on a line.
  const cursor = $derived(hover !== null && samples[hover] ? samples[hover] : null);
  const cursorX = $derived(cursor ? ((cursor.t - box.from) / (box.to - box.from)) * plotW : 0);
  const flip = $derived(cursorX > plotW * 0.6);

  function move(e: PointerEvent & { currentTarget: SVGRectElement }) {
    const r = e.currentTarget.getBoundingClientRect();
    const t = box.from + ((e.clientX - r.left) / r.width) * (box.to - box.from);
    const i = nearestIndex(samples, t);
    hover = i >= 0 && Math.abs(samples[i].t - t) < 90_000 ? i : null;
  }

  function key(e: KeyboardEvent) {
    if (!samples.length) return;
    const last = samples.length - 1;
    const at = hover ?? last;
    if (e.key === "ArrowLeft") hover = Math.max(0, at - 1);
    else if (e.key === "ArrowRight") hover = Math.min(last, at + 1);
    else if (e.key === "Home") hover = 0;
    else if (e.key === "End") hover = last;
    else if (e.key === "Escape") {
      // Escape closes the readout first; only with none showing does it reach the dialog and close that.
      if (hover === null) return;
      hover = null;
      e.stopPropagation();
    } else return;
    e.preventDefault();
  }

  const summary = $derived(`${title}. ${series.map((s) => `${s.label} ${fmt(latest?.[s.key] ?? null)}`).join(", ")}. Last 15 minutes; arrow keys move through readings.`);
</script>

<figure class="hostchart m-0 rounded-lg border border-line bg-panel p-3">
  <figcaption class="mb-1 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
    <span class="text-sm font-semibold">{title}</span>
    <span class="flex flex-wrap items-center gap-x-4 gap-y-0.5 text-xs">
      {#each series as s (s.key)}
        <span class="flex items-center gap-1.5">
          {#if series.length > 1}<span class="key" style="background: var(--series-{s.slot})" aria-hidden="true"></span>{/if}
          <span class="text-fg-muted">{series.length > 1 ? s.label : ""}</span>
          <span class="font-semibold tabular-nums">{fmt(latest?.[s.key] ?? null)}</span>
        </span>
      {/each}
    </span>
  </figcaption>

  <!-- The drawing is sized to this box (inside the card's padding), so it can never run past the edge. -->
  <div class="relative" style="height: {HEIGHT}px" bind:clientWidth={width}>
    {#if !hasData}
      <p class="absolute inset-0 flex items-center justify-center text-xs text-fg-muted">{empty}</p>
    {/if}
    <!-- The chart takes focus so the arrow keys can step through readings, the keyboard's version of the
         pointer's crosshair. Its label gives the latest values, and the table view lists every reading. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <svg {width} height={HEIGHT} role="img" aria-label={summary} tabindex="0" onkeydown={key} onblur={() => (hover = null)} class="block overflow-visible outline-none focus-visible:ring-1 focus-visible:ring-accent/50">
      <g transform="translate({PAD.left} {PAD.top})">
        {#each yTicks as t, i (i)}
          {@const y = plotH - (t / max) * plotH}
          <line x1="0" x2={plotW} y1={y} y2={y} class="grid" />
          <text x="-8" {y} dy="0.32em" text-anchor="end" class="axis">{fmt(t)}</text>
        {/each}
        {#each [1, 2 / 3, 1 / 3, 0] as f (f)}
          {@const x = plotW - f * plotW}
          <text {x} y={plotH + 16} text-anchor={f === 0 ? "end" : f === 1 ? "start" : "middle"} class="axis">{agoLabel(f * windowMs)}</text>
        {/each}

        {#if lines.length === 1 && lines[0].segments.length}
          {#each lines[0].segments as seg, i (i)}<path d={areaOf(seg, plotH)} fill="var(--series-{lines[0].slot})" fill-opacity="0.1" />{/each}
        {/if}
        {#each lines as l (l.key)}
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
          {#each lines as l (l.key)}
            {@const v = cursor[l.key]}
            {#if v !== null}<circle cx={cursorX} cy={plotH - Math.min(1, v / max) * plotH} r="4" fill="var(--series-{l.slot})" stroke="var(--surface)" stroke-width="2" />{/if}
          {/each}
        {/if}

        <!-- A wide, invisible target: the pointer only has to be near a reading, not on the line. -->
        <rect role="presentation" x="0" y="0" width={plotW} height={plotH} fill="transparent" onpointermove={move} onpointerleave={() => (hover = null)} />
      </g>
    </svg>

    {#if cursor}
      <div class="tip" style="top: {PAD.top}px; {flip ? `right: ${width - PAD.left - cursorX + 10}px` : `left: ${PAD.left + cursorX + 10}px`}" role="status">
        <div class="mb-1 text-[11px] text-fg-muted">{windowMs > 3_600_000 ? new Date(cursor.t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : clock(cursor.t)}</div>
        {#each series as s (s.key)}
          <div class="flex items-center gap-2">
            <span class="key" style="background: var(--series-{s.slot})" aria-hidden="true"></span>
            <span class="font-semibold tabular-nums">{fmt(cursor[s.key])}</span>
            {#if series.length > 1}<span class="text-fg-muted">{s.label}</span>{/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</figure>

<style>
  /* Colours are roles, defined once and swapped per theme: the two categorical slots, stepped for each surface. */
  .hostchart {
    --surface: #20222b;
    --series-1: #3987e5;
    --series-2: #d95926;
    --grid: #2c2f3a;
    --text-muted: #8b8d98;
  }
  :global(:root[data-theme="light"]) .hostchart {
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
  /* A short stroke of the series colour: the key, never a box. */
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
