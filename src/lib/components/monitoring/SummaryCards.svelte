<script lang="ts">
  import { Boxes, CircleAlert, CircleCheck, PlugZap, TriangleAlert } from "lucide-svelte";
  import type { HealthFilter, Summary } from "$lib/monitoringview";

  let { summary, onpick }: { summary: Summary; /** Show the hosts with this health. */ onpick: (h: HealthFilter) => void } = $props();

  const cards = $derived([
    { key: "all", label: "Total hosts", value: summary.total, icon: Boxes, tone: "text-accent bg-accent/10" },
    { key: "healthy", label: "Healthy", value: summary.healthy, icon: CircleCheck, tone: "text-success bg-success/10" },
    { key: "warning", label: "Warning", value: summary.warning, icon: TriangleAlert, tone: summary.warning ? "text-warning bg-warning/10" : "text-fg-muted bg-fg-muted/10" },
    { key: "critical", label: "Critical", value: summary.critical, icon: CircleAlert, tone: summary.critical ? "text-danger bg-danger/10" : "text-fg-muted bg-fg-muted/10" },
    { key: "offline", label: "Offline", value: summary.offline, icon: PlugZap, tone: summary.offline ? "text-danger bg-danger/10" : "text-fg-muted bg-fg-muted/10" },
  ] as const);
</script>

<!-- Counted from what the checks actually found; each card shows the hosts behind it. -->
<div class="grid grid-cols-5 gap-2 max-md:grid-cols-3" role="group" aria-label="Monitoring summary">
  {#each cards as c (c.key)}
    <button class="flex items-center gap-2.5 rounded-[10px] border border-line bg-panel px-3 py-2 text-left transition-colors hover:border-accent/40 hover:bg-panel-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" title="Show {c.key === 'all' ? 'all hosts' : c.label.toLowerCase() + ' hosts'}" onclick={() => onpick(c.key)}>
      <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md max-sm:hidden {c.tone}"><c.icon size={15} aria-hidden="true" /></span>
      <span class="min-w-0">
        <span class="block truncate text-[11px] text-fg-muted">{c.label}</span>
        <span class="block text-lg font-semibold leading-6 tabular-nums">{c.value}</span>
      </span>
    </button>
  {/each}
</div>
