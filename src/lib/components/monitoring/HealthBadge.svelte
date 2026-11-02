<script lang="ts">
  import { CircleAlert, CircleCheck, CircleHelp, PauseCircle, PlugZap, TriangleAlert } from "lucide-svelte";
  import { HEALTH_TEXT, type Health } from "$lib/monitoringview";

  let { health }: { health: Health } = $props();

  // Health is said in words and an icon as well as colour.
  const tone: Record<Health, string> = {
    healthy: "border-success/30 bg-success/10 text-success",
    warning: "border-warning/30 bg-warning/10 text-warning",
    critical: "border-danger/30 bg-danger/10 text-danger",
    offline: "border-danger/30 bg-danger/10 text-danger",
    paused: "border-line bg-fg-muted/10 text-fg-muted",
    waiting: "border-line bg-fg-muted/10 text-fg-muted",
  };
</script>

<span class="inline-flex h-6 items-center gap-1.5 rounded-full border px-2 text-xs font-medium {tone[health]}">
  {#if health === "healthy"}<CircleCheck size={13} aria-hidden="true" />
  {:else if health === "warning"}<TriangleAlert size={13} aria-hidden="true" />
  {:else if health === "critical"}<CircleAlert size={13} aria-hidden="true" />
  {:else if health === "offline"}<PlugZap size={13} aria-hidden="true" />
  {:else if health === "paused"}<PauseCircle size={13} aria-hidden="true" />
  {:else}<CircleHelp size={13} aria-hidden="true" />{/if}
  {HEALTH_TEXT[health]}
</span>
