<script lang="ts">
  import { ArrowRight, Table2 } from "lucide-svelte";
  import type { DbTableMeta } from "$lib/types";

  let { meta, schema, table, onopen }: { meta: DbTableMeta; schema: string; table: string; onopen: (schema: string, table: string) => void } = $props();

  const outgoing = $derived(meta.constraints.filter((c) => c.kind === "foreign_key" && c.references));
  const incoming = $derived(meta.referenced_by);

  // A small diagram: tables that point here on the left, this table in the middle, tables it points at on the right.
  const BOX_W = 168;
  const BOX_H = 34;
  const GAP = 14;
  const COL_GAP = 120;
  const rows = $derived(Math.max(1, incoming.length, outgoing.length));
  const height = $derived(rows * (BOX_H + GAP) + GAP);
  const width = BOX_W * 3 + COL_GAP * 2 + 16;
  const y = (i: number, n: number) => (height - n * (BOX_H + GAP) + GAP) / 2 + i * (BOX_H + GAP);
  const midY = $derived(height / 2);
</script>

{#if incoming.length === 0 && outgoing.length === 0}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">{schema}.{table} has no foreign keys, and no other table points at it.</p>
{:else}
  <div class="space-y-5 overflow-auto p-4">
    <div class="overflow-x-auto rounded-lg border border-line bg-base p-2">
      <svg {width} {height} viewBox="0 0 {width} {height}" role="img" aria-label="Foreign keys of {table}" class="mx-auto">
        <defs><marker id="arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" fill="var(--color-fg-muted)" /></marker></defs>
        {#each incoming as r, i (r.name + r.table)}
          <path d="M{8 + BOX_W},{y(i, incoming.length) + BOX_H / 2} C{8 + BOX_W + COL_GAP / 2},{y(i, incoming.length) + BOX_H / 2} {8 + BOX_W + COL_GAP / 2},{midY} {8 + BOX_W + COL_GAP - 2},{midY}" fill="none" stroke="var(--color-fg-muted)" stroke-width="1.2" marker-end="url(#arrow)" />
        {/each}
        {#each outgoing as c, i (c.name)}
          <path d="M{8 + BOX_W * 2 + COL_GAP},{midY} C{8 + BOX_W * 2 + COL_GAP * 1.5},{midY} {8 + BOX_W * 2 + COL_GAP * 1.5},{y(i, outgoing.length) + BOX_H / 2} {8 + BOX_W * 2 + COL_GAP * 2 - 2},{y(i, outgoing.length) + BOX_H / 2}" fill="none" stroke="var(--color-fg-muted)" stroke-width="1.2" marker-end="url(#arrow)" />
        {/each}
        {#each incoming as r, i (r.name + r.table)}
          <g role="button" tabindex="0" class="cursor-pointer outline-none" aria-label="Open {r.schema}.{r.table}" onclick={() => onopen(r.schema, r.table)} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onopen(r.schema, r.table)}>
            <rect x="8" y={y(i, incoming.length)} width={BOX_W} height={BOX_H} rx="7" fill="var(--color-panel)" stroke="var(--color-line)" />
            <text x={8 + BOX_W / 2} y={y(i, incoming.length) + 14} text-anchor="middle" font-size="11" fill="var(--color-fg)">{r.table.length > 22 ? r.table.slice(0, 21) + "…" : r.table}</text>
            <text x={8 + BOX_W / 2} y={y(i, incoming.length) + 27} text-anchor="middle" font-size="9" fill="var(--color-fg-muted)">{r.columns.join(", ").slice(0, 26)}</text>
          </g>
        {/each}
        <g>
          <rect x={8 + BOX_W + COL_GAP} y={midY - BOX_H / 2} width={BOX_W} height={BOX_H} rx="7" fill="color-mix(in srgb, var(--color-accent) 18%, var(--color-panel))" stroke="var(--color-accent)" />
          <text x={8 + BOX_W * 1.5 + COL_GAP} y={midY + 4} text-anchor="middle" font-size="12" font-weight="600" fill="var(--color-fg)">{table.length > 22 ? table.slice(0, 21) + "…" : table}</text>
        </g>
        {#each outgoing as c, i (c.name)}
          {@const ref = c.references!}
          <g role="button" tabindex="0" class="cursor-pointer outline-none" aria-label="Open {ref.schema}.{ref.table}" onclick={() => onopen(ref.schema, ref.table)} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onopen(ref.schema, ref.table)}>
            <rect x={8 + BOX_W * 2 + COL_GAP * 2} y={y(i, outgoing.length)} width={BOX_W} height={BOX_H} rx="7" fill="var(--color-panel)" stroke="var(--color-line)" />
            <text x={8 + BOX_W * 2.5 + COL_GAP * 2} y={y(i, outgoing.length) + 14} text-anchor="middle" font-size="11" fill="var(--color-fg)">{ref.table.length > 22 ? ref.table.slice(0, 21) + "…" : ref.table}</text>
            <text x={8 + BOX_W * 2.5 + COL_GAP * 2} y={y(i, outgoing.length) + 27} text-anchor="middle" font-size="9" fill="var(--color-fg-muted)">{ref.columns.join(", ").slice(0, 26)}</text>
          </g>
        {/each}
      </svg>
    </div>

    <div class="grid gap-4 lg:grid-cols-2">
      <section aria-labelledby="rel-out">
        <h3 id="rel-out" class="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">References ({outgoing.length})</h3>
        {#if outgoing.length === 0}<p class="text-xs text-fg-muted">No foreign keys.</p>{/if}
        <ul class="space-y-1.5">
          {#each outgoing as c (c.name)}
            {@const ref = c.references!}
            <li class="rounded-lg border border-line bg-panel px-3 py-2 text-xs">
              <p class="font-mono"><span class="text-fg-muted">{c.columns.join(", ")}</span> <ArrowRight size={11} class="inline" /> <button class="text-accent hover:underline" onclick={() => onopen(ref.schema, ref.table)}><Table2 size={11} class="mr-0.5 inline" />{ref.schema}.{ref.table}</button> <span class="text-fg-muted">({ref.columns.join(", ")})</span></p>
              <p class="mt-0.5 text-[11px] text-fg-muted">{c.name}{#if ref.on_delete} · ON DELETE {ref.on_delete}{/if}{#if ref.on_update} · ON UPDATE {ref.on_update}{/if}</p>
            </li>
          {/each}
        </ul>
      </section>
      <section aria-labelledby="rel-in">
        <h3 id="rel-in" class="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Referenced by ({incoming.length})</h3>
        {#if incoming.length === 0}<p class="text-xs text-fg-muted">No other table points here.</p>{/if}
        <ul class="space-y-1.5">
          {#each incoming as r (r.name + r.table)}
            <li class="rounded-lg border border-line bg-panel px-3 py-2 text-xs">
              <p class="font-mono"><button class="text-accent hover:underline" onclick={() => onopen(r.schema, r.table)}><Table2 size={11} class="mr-0.5 inline" />{r.schema}.{r.table}</button> <span class="text-fg-muted">({r.columns.join(", ")})</span> <ArrowRight size={11} class="inline" /> <span class="text-fg-muted">{r.referenced_columns.join(", ")}</span></p>
              <p class="mt-0.5 text-[11px] text-fg-muted">{r.name}</p>
            </li>
          {/each}
        </ul>
      </section>
    </div>
  </div>
{/if}
