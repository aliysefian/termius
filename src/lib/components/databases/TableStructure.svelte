<script lang="ts">
  import { KeyRound, Link2 } from "lucide-svelte";
  import type { DbTableInfo, DbTableMeta } from "$lib/types";

  let { info, meta }: { info: DbTableInfo | null; meta: DbTableMeta | null } = $props();

  const note = (name: string) => meta?.column_notes.find(([n]) => n === name)?.[1] ?? "";
  const fk = (name: string) => meta?.constraints.find((c) => c.kind === "foreign_key" && c.columns.includes(name));
  const unique = (name: string) => meta?.constraints.some((c) => c.kind === "unique" && c.columns.length === 1 && c.columns[0] === name) ?? false;
</script>

{#if !info}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">The columns couldn't be read.</p>
{:else}
  <div class="overflow-auto">
    <table class="w-full min-w-[40rem] text-left text-xs" aria-label="Columns">
      <thead class="sticky top-0 bg-panel text-fg-muted">
        <tr class="border-b border-line">
          <th class="w-10 px-3 py-2 text-right font-medium">#</th>
          <th class="px-3 py-2 font-medium">Column</th>
          <th class="px-3 py-2 font-medium">Type</th>
          <th class="px-3 py-2 font-medium">Nullable</th>
          <th class="px-3 py-2 font-medium">Default</th>
          <th class="px-3 py-2 font-medium">Keys</th>
          <th class="px-3 py-2 font-medium">Notes</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-line/60">
        {#each info.columns as c, i (c.name)}
          {@const f = fk(c.name)}
          <tr class="hover:bg-panel-hover/50">
            <td class="px-3 py-1.5 text-right text-fg-muted">{i + 1}</td>
            <td class="px-3 py-1.5 font-mono font-medium">{c.name}</td>
            <td class="px-3 py-1.5 font-mono text-sky-400">{c.data_type}</td>
            <td class="px-3 py-1.5">{c.nullable ? "yes" : "no"}</td>
            <td class="max-w-64 truncate px-3 py-1.5 font-mono text-fg-muted" title={c.default ?? ""}>{c.default ?? ""}</td>
            <td class="px-3 py-1.5">
              <span class="inline-flex items-center gap-2">
                {#if c.primary_key}<span class="inline-flex items-center gap-1 text-warning" title="Primary key"><KeyRound size={12} /> PK</span>{/if}
                {#if f}<span class="inline-flex items-center gap-1 text-accent" title="References {f.references?.schema}.{f.references?.table}"><Link2 size={12} /> FK</span>{/if}
                {#if unique(c.name)}<span class="text-fg-muted" title="Unique">unique</span>{/if}
              </span>
            </td>
            <td class="px-3 py-1.5 text-fg-muted">{note(c.name)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
