<script lang="ts">
  import { CircleAlert, CircleCheck, CircleDot, Loader2, PauseCircle } from "lucide-svelte";
  import { STATUS_TEXT, type TunnelStatus } from "$lib/tunnels";

  let { status, title = undefined }: { status: TunnelStatus; title?: string } = $props();

  // Status is said in words and an icon as well as colour.
  const tone: Record<TunnelStatus, string> = {
    active: "border-success/30 bg-success/10 text-success",
    connecting: "border-warning/30 bg-warning/10 text-warning",
    stopping: "border-warning/30 bg-warning/10 text-warning",
    failed: "border-danger/30 bg-danger/10 text-danger",
    stopped: "border-line bg-fg-muted/10 text-fg-muted",
  };
</script>

<span class="inline-flex h-6 items-center gap-1.5 rounded-full border px-2 text-xs font-medium {tone[status]}" {title}>
  {#if status === "active"}<CircleCheck size={13} aria-hidden="true" />
  {:else if status === "connecting" || status === "stopping"}<Loader2 size={13} class="animate-spin motion-reduce:animate-none" aria-hidden="true" />
  {:else if status === "failed"}<CircleAlert size={13} aria-hidden="true" />
  {:else}<PauseCircle size={13} aria-hidden="true" />{/if}
  {STATUS_TEXT[status]}
</span>
