<script lang="ts">
  import { CircleCheck } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import { STATUS_TEXT, ago, statusClass } from "$lib/monitoring";
  import type { HostRow } from "$lib/monitoringview";

  let { rows, now, onpick }: { rows: HostRow[]; now: number; onpick: (id: string) => void } = $props();
  const raised = $derived(rows.flatMap((r) => (r.status?.states ?? []).filter((s) => s.status !== "ok").map((s) => ({ row: r, s }))));
</script>

{#if raised.length === 0}
  <EmptyState icon={CircleCheck} text="No rule is raised right now." />
{:else}
  <div class="divide-y divide-line rounded-xl border border-line bg-panel">
    {#each raised as { row, s } (row.id + s.rule_id + s.subject)}
      <button class="flex w-full items-center gap-3 px-4 py-2.5 text-left text-sm hover:bg-panel-hover" onclick={() => onpick(row.id)}>
        <span class="inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium {statusClass(s.status)}">{STATUS_TEXT[s.status]}</span>
        <span class="w-44 shrink-0 truncate font-medium">{row.target.label}</span>
        <span class="min-w-0 flex-1 truncate">{s.rule}</span>
        <span class="shrink-0 text-xs text-fg-muted">since {ago(s.since, now)}</span>
      </button>
    {/each}
  </div>
{/if}
