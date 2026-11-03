<script lang="ts">
  import type { DbConstraintKind, DbTableMeta } from "$lib/types";

  let { meta }: { meta: DbTableMeta } = $props();

  const LABEL: Record<DbConstraintKind, string> = { primary_key: "Primary key", foreign_key: "Foreign key", unique: "Unique", check: "Check", exclusion: "Exclusion" };
  const TONE: Record<DbConstraintKind, string> = {
    primary_key: "border-warning/40 text-warning",
    foreign_key: "border-accent/40 text-accent",
    unique: "border-sky-400/40 text-sky-400",
    check: "border-emerald-400/40 text-emerald-400",
    exclusion: "border-line text-fg-muted",
  };
</script>

{#if meta.constraints.length === 0}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">This {meta.kind ?? "table"} has no constraints.</p>
{:else}
  <div class="overflow-auto">
    <table class="w-full min-w-[40rem] text-left text-xs" aria-label="Constraints">
      <thead class="sticky top-0 bg-panel text-fg-muted">
        <tr class="border-b border-line">
          <th class="px-3 py-2 font-medium">Name</th>
          <th class="px-3 py-2 font-medium">Kind</th>
          <th class="px-3 py-2 font-medium">Columns</th>
          <th class="px-3 py-2 font-medium">Definition</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-line/60">
        {#each meta.constraints as c (c.name + c.kind)}
          <tr class="align-top hover:bg-panel-hover/50">
            <td class="px-3 py-1.5 font-mono font-medium">{c.name}</td>
            <td class="px-3 py-1.5"><span class="rounded border px-1.5 py-0.5 text-[11px] {TONE[c.kind]}">{LABEL[c.kind]}</span></td>
            <td class="px-3 py-1.5 font-mono">{c.columns.join(", ")}</td>
            <td class="px-3 py-1.5 font-mono text-[11px] text-fg-muted">
              {#if c.references}
                → {c.references.schema}.{c.references.table} ({c.references.columns.join(", ")}){#if c.references.on_update} · ON UPDATE {c.references.on_update}{/if}{#if c.references.on_delete} · ON DELETE {c.references.on_delete}{/if}
              {:else}
                {c.definition ?? ""}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
