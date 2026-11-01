<script lang="ts">
  // A container's lifecycle state and, separately, what its own health check says (if it has one).
  import { CircleAlert, CircleCheck, CirclePause, CircleStop, HeartPulse, Loader2, RotateCw } from "lucide-svelte";
  import { stateTone } from "$lib/containerdata";
  import { exitCode, health, isFailed } from "$lib/containerview";
  import type { ContainerInfo } from "$lib/types";

  let { c, detail = false }: { c: ContainerInfo; /** Also say how long ago, from the runtime's own status text. */ detail?: boolean } = $props();

  const tone = $derived(stateTone(c.state));
  const h = $derived(health(c));
  const failed = $derived(isFailed(c));
  const toneClass = $derived(
    c.state === "running" ? "text-success" : c.state === "paused" || c.state === "restarting" ? "text-warning" : failed ? "text-danger" : "text-fg-muted",
  );
</script>

<span class="inline-flex min-w-0 flex-col gap-0.5">
  <span class="inline-flex items-center gap-1.5 text-xs font-medium {toneClass}" data-tone={tone}>
    {#if c.state === "running"}<CircleCheck size={13} aria-hidden="true" />
    {:else if c.state === "paused"}<CirclePause size={13} aria-hidden="true" />
    {:else if c.state === "restarting"}<RotateCw size={13} class="animate-spin motion-reduce:animate-none" aria-hidden="true" />
    {:else if c.state === "removing"}<Loader2 size={13} class="animate-spin motion-reduce:animate-none" aria-hidden="true" />
    {:else if failed}<CircleAlert size={13} aria-hidden="true" />
    {:else}<CircleStop size={13} aria-hidden="true" />{/if}
    <span class="capitalize">{c.state}</span>{#if failed && exitCode(c.status) !== null}<span class="font-mono text-[11px] font-normal opacity-80" title="Exit code">({exitCode(c.status)})</span>{/if}
    {#if h}
      <span class="inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[10px] font-medium {h === 'healthy' ? 'border-success/30 text-success' : h === 'unhealthy' ? 'border-danger/30 text-danger' : 'border-warning/30 text-warning'}" title="The container's own health check">
        <HeartPulse size={10} aria-hidden="true" />{h}
      </span>
    {/if}
  </span>
  {#if detail}<span class="truncate text-[11px] text-fg-muted">{c.status}</span>{/if}
</span>
