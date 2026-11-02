<script lang="ts">
  import { HeartPulse } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import { ago } from "$lib/monitoring";
  import { checkLines, type HostRow } from "$lib/monitoringview";

  let { rows, now, onpick }: { rows: HostRow[]; now: number; onpick: (id: string) => void } = $props();
  const lines = $derived(rows.flatMap((r) => checkLines(r.status).map((c) => ({ row: r, c }))));
</script>

{#if lines.length === 0}
  <EmptyState icon={HeartPulse} text="No check has run yet." />
{:else}
  <div class="divide-y divide-line rounded-xl border border-line bg-panel">
    {#each lines as { row, c } (row.id + c.kind + c.name)}
      <button class="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-panel-hover" onclick={() => onpick(row.id)}>
        <span class="h-2 w-2 shrink-0 rounded-full {c.ok === null ? 'bg-fg-muted/50' : c.ok ? 'bg-success' : 'bg-danger'}"></span>
        <span class="w-40 shrink-0 truncate font-medium">{row.target.label}</span>
        <span class="w-40 shrink-0 truncate">{c.name}</span>
        <span class="min-w-0 flex-1 truncate text-xs {c.ok === false ? 'text-danger' : 'text-fg-muted'}">{c.ok ? "OK" : "Failing"} · {c.detail}</span>
        <span class="shrink-0 text-xs text-fg-muted">{row.status ? ago(row.status.checked_at, now) : ""}</span>
      </button>
    {/each}
  </div>
{/if}
