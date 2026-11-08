<script lang="ts">
  import { fitWindow, type Frame } from "$lib/hostframes";
  import { areaOf, chartSegments, pathOf, type Box } from "$lib/hosthistory";

  let {
    frames,
    pick,
    max = 100,
    now,
    windowMs,
    slot = 1,
    class: cls = "mt-1 h-8 w-full",
  }: { frames: Frame[]; pick: (f: Frame) => number | null; max?: number; now: number; windowMs: number; slot?: 1 | 2 | 3 | 4; class?: string } = $props();

  const W = 160;
  const H = 34;
  const box = $derived<Box>({ width: W, height: H, from: now - fitWindow(frames, now, windowMs), to: now, max: max > 0 ? max : 1 });
  const segs = $derived(chartSegments(frames, pick, box, 30_000));
</script>

<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="spark {cls}" style="--c: var(--spark-{slot})" aria-hidden="true">
  {#each segs as seg, i (i)}
    <path d={areaOf(seg, H)} fill="var(--c)" fill-opacity="0.12" />
    <path d={pathOf(seg)} fill="none" stroke="var(--c)" stroke-width="1.5" vector-effect="non-scaling-stroke" stroke-linejoin="round" stroke-linecap="round" />
  {/each}
</svg>

<style>
  .spark {
    --spark-1: #8b5cf6;
    --spark-2: #3987e5;
    --spark-3: #e5a023;
    --spark-4: #3fb27f;
  }
  :global(:root[data-theme="light"]) .spark {
    --spark-1: #6d3fe0;
    --spark-2: #2a78d6;
    --spark-3: #b87500;
    --spark-4: #1f8a5b;
  }
</style>
