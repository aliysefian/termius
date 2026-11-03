<script lang="ts">
  import { KeyRound } from "lucide-svelte";
  import { formatBytes } from "$lib/dbinspect";
  import type { DbTableMeta } from "$lib/types";

  let { meta }: { meta: DbTableMeta } = $props();
  let open = $state<Record<string, boolean>>({});
</script>

{#if meta.indexes.length === 0}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">This {meta.kind ?? "table"} has no indexes.</p>
{:else}
  <div class="overflow-auto">
    <table class="w-full min-w-[40rem] text-left text-xs" aria-label="Indexes">
      <thead class="sticky top-0 bg-panel text-fg-muted">
        <tr class="border-b border-line">
          <th class="px-3 py-2 font-medium">Name</th>
          <th class="px-3 py-2 font-medium">Columns</th>
          <th class="px-3 py-2 font-medium">Unique</th>
          <th class="px-3 py-2 font-medium">Type</th>
          <th class="px-3 py-2 text-right font-medium">Size</th>
          <th class="px-3 py-2 font-medium">Definition</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-line/60">
        {#each meta.indexes as i (i.name)}
          <tr class="align-top hover:bg-panel-hover/50">
            <td class="px-3 py-1.5 font-mono font-medium">{#if i.primary}<KeyRound size={12} class="mr-1 inline text-warning" aria-label="Primary key" />{/if}{i.name}</td>
            <td class="px-3 py-1.5 font-mono">{i.columns.length ? i.columns.join(", ") : "(expression)"}</td>
            <td class="px-3 py-1.5">{i.unique ? "yes" : "no"}</td>
            <td class="px-3 py-1.5 text-fg-muted">{i.method ?? ""}</td>
            <td class="px-3 py-1.5 text-right font-mono text-fg-muted">{i.size_bytes === null ? "" : formatBytes(i.size_bytes)}</td>
            <td class="max-w-[28rem] px-3 py-1.5 font-mono text-[11px] text-fg-muted">
              {#if i.definition}
                <button class="text-left {open[i.name] ? 'whitespace-pre-wrap break-all' : 'truncate'} max-w-full" aria-expanded={!!open[i.name]} title={open[i.name] ? "Collapse" : "Show all"} onclick={() => (open[i.name] = !open[i.name])}>{i.definition}</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
