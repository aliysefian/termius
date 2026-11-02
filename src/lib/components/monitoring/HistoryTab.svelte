<script lang="ts">
  import { BellRing } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { STATUS_TEXT, ago, describeDeliveries, statusClass } from "$lib/monitoring";

  let { now }: { now: number } = $props();
  $effect(() => {
    // Looking at the history reads it.
    monitoring.unread = 0;
  });
</script>

{#if monitoring.history.length === 0}
  <EmptyState icon={BellRing} text="No alert has been raised since the vault was unlocked. This list is not kept when it locks." />
{:else}
  <div class="divide-y divide-line rounded-xl border border-line bg-panel">
    {#each monitoring.history as a, i (a.at + a.target_id + a.rule + i)}
      <div class="px-4 py-2.5">
        <div class="flex flex-wrap items-center gap-2 text-sm">
          <span class="inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium {statusClass(a.status)}">{a.status === "ok" ? "Recovered" : STATUS_TEXT[a.status]}</span>
          <span class="font-medium">{a.target}</span>
          <span class="text-fg-muted">{a.rule}</span>
          <span class="ml-auto text-xs text-fg-muted">{ago(a.at, now)}</span>
        </div>
        <div class="mt-0.5 text-xs">{a.summary}</div>
        <div class="mt-0.5 text-xs text-fg-muted">{describeDeliveries(a.deliveries)}</div>
      </div>
    {/each}
  </div>
{/if}
