<script lang="ts">
  import { Boxes, CircleAlert, CircleCheck, CircleStop } from "lucide-svelte";
  import type { Metrics } from "$lib/containerview";

  let { metrics, onpick }: { metrics: Metrics; /** Show the containers with this status. */ onpick: (status: "all" | "running" | "stopped" | "unhealthy") => void } = $props();

  const cards = $derived([
    { key: "all", label: "Total", value: metrics.total, icon: Boxes, tone: "text-accent bg-accent/10" },
    { key: "running", label: "Running", value: metrics.running, icon: CircleCheck, tone: "text-success bg-success/10" },
    { key: "stopped", label: "Stopped", value: metrics.stopped, icon: CircleStop, tone: "text-fg-muted bg-fg-muted/10" },
    { key: "unhealthy", label: "Unhealthy", value: metrics.unhealthy, icon: CircleAlert, tone: metrics.unhealthy ? "text-danger bg-danger/10" : "text-fg-muted bg-fg-muted/10" },
  ] as const);
</script>

<!-- Counts of the containers' real states; each card shows the containers behind it. -->
<div class="grid grid-cols-4 gap-2" role="group" aria-label="Container overview">
  {#each cards as c (c.key)}
    <button class="flex items-center gap-2.5 rounded-[10px] border border-line bg-panel px-3 py-2 text-left transition-colors hover:border-accent/40 hover:bg-panel-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" title="Show {c.key === 'all' ? 'all containers' : c.label.toLowerCase() + ' containers'}" onclick={() => onpick(c.key)}>
      <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md max-sm:hidden {c.tone}"><c.icon size={15} aria-hidden="true" /></span>
      <span class="min-w-0">
        <span class="block truncate text-[11px] text-fg-muted">{c.label}</span>
        <span class="block text-lg font-semibold leading-6 tabular-nums">{c.value}</span>
      </span>
    </button>
  {/each}
</div>
