<script lang="ts">
  import { ChevronDown, ChevronRight, Loader2 } from "lucide-svelte";
  import { infra } from "$lib/stores/infra.svelte";
  import { offsets, windowOf } from "$lib/virtuallist";
  import { FRESH_LABEL, FRESH_TONE, STATUS_TONE, TYPE_ICON } from "./style";

  const ROW = 28;
  let scroller = $state<HTMLDivElement | null>(null);
  let top = $state(0);
  let height = $state(480);
  const starts = $derived(offsets(infra.rows.map(() => ROW)));
  const win = $derived(windowOf(starts, top, height));
  const shown = $derived(infra.rows.slice(win.start, win.end));
</script>

<div
  bind:this={scroller}
  class="min-h-0 flex-1 overflow-auto"
  bind:clientHeight={height}
  onscroll={() => (top = scroller?.scrollTop ?? 0)}
  role="tree"
  aria-label="Resources"
  data-testid="infra-tree"
>
  <div style="height: {win.total}px; position: relative;">
    <div style="position: absolute; top: {win.top}px; left: 0; right: 0;">
      {#each shown as row (row.key)}
        {@const R = row.resource ? TYPE_ICON[row.resource.type] : null}
        {@const selected = row.resource && infra.selectedId === row.resource.id}
        <div
          class="flex items-center gap-1 pr-2 text-xs {selected ? 'bg-accent/15 text-fg' : 'hover:bg-panel-hover/60'} {row.kind === 'group' ? 'text-fg-muted' : ''}"
          style="height: {ROW}px; padding-left: {0.4 + row.depth * 0.9}rem"
          role="treeitem"
          aria-level={row.depth + 1}
          aria-expanded={row.expandable ? row.open : undefined}
          aria-selected={selected ? true : undefined}
        >
          {#if row.expandable}
            <button class="icon-btn h-5 w-5 shrink-0" aria-label="{row.open ? 'Collapse' : 'Expand'} {row.label}" onclick={() => infra.toggle(row.key, row.kind === "host" ? row.hostId : null)}>
              {#if infra.busy[row.hostId ?? ""] && row.kind === "host"}<Loader2 size={12} class="animate-spin" />{:else if row.open}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
            </button>
          {:else}<span class="w-5 shrink-0"></span>{/if}

          {#if row.kind === "note"}
            <span class="italic text-fg-muted">{row.text}</span>
          {:else if row.kind === "group"}
            <span class="font-medium">{row.label}</span>
            {#if row.count !== undefined && row.count > 0}<span class="text-fg-muted">{row.count}</span>{/if}
            {#if row.text}<span class="truncate text-[11px] italic text-fg-muted" title={row.text}>{row.text}</span>{/if}
            {#if row.freshness === "stale"}<span class="ml-auto text-[10px] {FRESH_TONE.stale}" title="Last read failed or is old; this is the last known list">{FRESH_LABEL.stale}</span>{/if}
          {:else if row.resource && R}
            <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" onclick={() => { if (row.resource) infra.select(row.resource.id); }}>
              <span class="h-2 w-2 shrink-0 rounded-full {STATUS_TONE[row.resource.status]} {row.resource.freshness === 'stale' ? 'opacity-40' : ''}" title="{row.resource.statusText}{row.resource.freshness === 'stale' ? ' (last known)' : ''}"></span>
              <R size={13} class="shrink-0 text-fg-muted" />
              <span class="truncate">{row.label}</span>
              {#if row.resource.freshness === "stale"}<span class="shrink-0 text-[10px] {FRESH_TONE.stale}">stale</span>{/if}
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </div>
  {#if infra.rows.length === 0}
    <p class="px-3 py-6 text-center text-xs text-fg-muted">{infra.filters.query || infra.filters.types.length || infra.filters.statuses.length || infra.filters.environment ? `Nothing matches in what has been read so far (${infra.coverage.read} of ${infra.coverage.total} hosts).` : "No saved hosts."}</p>
  {/if}
</div>
