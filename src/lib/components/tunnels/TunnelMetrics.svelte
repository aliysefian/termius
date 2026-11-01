<script lang="ts">
  import { ArrowLeftRight, CircleAlert, CircleCheck, PauseCircle } from "lucide-svelte";
  import type { Counts } from "$lib/tunnels";

  let { counts, onpick, compact = false }: { counts: Counts; /** Filter the list to this status. */ onpick: (status: "all" | "active" | "stopped" | "failed") => void; /** One dense row, for a short or narrow window. */ compact?: boolean } = $props();

  const cards = $derived([
    { key: "all", label: "Total tunnels", value: counts.total, icon: ArrowLeftRight, tone: "text-accent bg-accent/10" },
    { key: "active", label: "Active", value: counts.active, icon: CircleCheck, tone: "text-success bg-success/10" },
    { key: "stopped", label: "Stopped", value: counts.stopped, icon: PauseCircle, tone: "text-fg-muted bg-fg-muted/10" },
    { key: "failed", label: "Failed", value: counts.failed, icon: CircleAlert, tone: counts.failed ? "text-danger bg-danger/10" : "text-fg-muted bg-fg-muted/10" },
  ] as const);
</script>

<!-- The numbers are the real tunnels' states; each card filters the list to its state. -->
<div class="grid gap-3 {compact ? 'grid-cols-4 gap-2' : 'grid-cols-2 lg:grid-cols-4'}" role="group" aria-label="Tunnel overview">
  {#each cards as c (c.key)}
    <button
      class="flex items-center gap-3 rounded-[10px] border border-line bg-panel text-left {compact ? 'px-3 py-2' : 'px-4 py-3'} transition-colors hover:border-accent/40 hover:bg-panel-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      title="Show {c.key === 'all' ? 'all tunnels' : c.label.toLowerCase() + ' tunnels'}"
      onclick={() => onpick(c.key)}
    >
      <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg {c.tone} {compact ? 'max-sm:hidden h-7 w-7' : ''}"><c.icon size={compact ? 15 : 18} aria-hidden="true" /></span>
      <span class="min-w-0">
        <span class="block truncate text-xs text-fg-muted">{c.label}</span>
        <span class="block font-semibold tabular-nums {compact ? 'text-lg leading-6' : 'text-2xl leading-7'}">{c.value}</span>
      </span>
    </button>
  {/each}
</div>
