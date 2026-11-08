<script lang="ts">
  import { usageTone } from "$lib/hostview";

  let { pct, label, tone = true }: { pct: number | null; label: string; /** Colour by how full; false keeps one colour. */ tone?: boolean } = $props();
  const t = $derived(tone ? usageTone(pct) : "none");
  const colour = $derived(t === "bad" ? "bg-danger" : t === "warn" ? "bg-warning" : "bg-accent");
</script>

<div class="h-1.5 w-full overflow-hidden rounded-full bg-line/60" role="progressbar" aria-label={label} aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct === null ? undefined : Math.round(pct)} aria-valuetext={pct === null ? "Unavailable" : `${pct.toFixed(1)}%`}>
  {#if pct !== null}<div class="h-full rounded-full {colour}" style="width: {Math.max(0, Math.min(100, pct))}%"></div>{/if}
</div>
