<script lang="ts">
  // A small line of the last readings. Draws nothing until there are two, so no reading is ever invented.
  import { sparkPoints } from "$lib/tmux/monitor";

  let { series, tone = "accent", label }: { series: readonly number[]; tone?: "accent" | "success"; label: string } = $props();
  const points = $derived(sparkPoints(series, 100, 24));
</script>

<svg viewBox="0 0 100 24" preserveAspectRatio="none" class="h-6 w-full {tone === 'success' ? 'text-success' : 'text-accent'}" role="img" aria-label={points ? `${label}: last ${series.length} readings` : `${label}: not enough readings yet`}>
  {#if points}<polyline {points} fill="none" stroke="currentColor" stroke-width="1.5" vector-effect="non-scaling-stroke" stroke-linejoin="round" />{:else}<line x1="0" y1="23" x2="100" y2="23" stroke="currentColor" stroke-opacity="0.25" stroke-dasharray="2 3" vector-effect="non-scaling-stroke" />{/if}
</svg>
