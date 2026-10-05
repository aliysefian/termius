<script lang="ts">
  import { ArrowDown, ArrowUp } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { cellText, isCut, sortedOrder, visibleRange } from "$lib/dbdata";
  import { databases, type QueryTab } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { tab }: { tab: QueryTab } = $props();

  const ROW_H = 28;
  const COL_W = 180;
  const NUM_W = 56;

  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(400);
  let selected = $state<{ row: number; col: number } | null>(null);
  let editing = $state<{ row: number; col: number; value: string } | null>(null);
  let editInput = $state<HTMLInputElement>();

  const result = $derived(tab.result);
  const order = $derived(
    result ? sortedOrder(result.rows, tab.sort.col, tab.sort.dir, tab.sort.col === null ? undefined : result.columns[tab.sort.col]?.kind) : [],
  );
  const range = $derived(visibleRange(scrollTop, viewport, ROW_H, order.length));
  const shown = $derived(order.slice(range.start, range.end));
  const width = $derived(NUM_W + (result?.columns.length ?? 0) * COL_W);

  // A new result starts at the top.
  $effect(() => {
    void tab.result;
    if (scroller) scroller.scrollTop = 0;
    selected = null;
    editing = null;
  });

  function startEdit(row: number, col: number) {
    if (!result) return;
    const block = databases.editBlock(tab, row, col);
    if (block) {
      ui.notify("info", block);
      return;
    }
    const cell = result.rows[row][col];
    editing = { row, col, value: cell === null ? "" : cellText(cell) };
    queueMicrotask(() => editInput?.select());
  }

  async function commit(value: string | null) {
    const e = editing;
    if (!e) return;
    editing = null;
    const current = result?.rows[e.row][e.col];
    const same = value !== null && current !== null && current !== undefined && !isCut(current) && String(current) === value;
    if (same || (value === null && current === null)) return;
    await databases.editCell(tab.id, e.row, e.col, value);
    scroller?.focus();
  }

  async function onKey(e: KeyboardEvent) {
    if (editing || !selected || !result) return;
    const { row, col } = selected;
    const pos = order.indexOf(row);
    const move = (dr: number, dc: number) => {
      const p = Math.min(order.length - 1, Math.max(0, pos + dr));
      selected = { row: order[p], col: Math.min(result!.columns.length - 1, Math.max(0, col + dc)) };
      e.preventDefault();
      // Keep the cell in view.
      const top = p * ROW_H;
      if (scroller) {
        if (top < scroller.scrollTop) scroller.scrollTop = top;
        else if (top + ROW_H > scroller.scrollTop + viewport - ROW_H) scroller.scrollTop = top + ROW_H * 2 - viewport;
      }
    };
    if (e.key === "ArrowDown") move(1, 0);
    else if (e.key === "ArrowUp") move(-1, 0);
    else if (e.key === "ArrowRight") move(0, 1);
    else if (e.key === "ArrowLeft") move(0, -1);
    else if (e.key === "Enter" || e.key === "F2") {
      e.preventDefault();
      startEdit(row, col);
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
      e.preventDefault();
      const c = result.rows[row][col];
      try {
        await writeText(c === null ? "" : isCut(c) ? c.preview : String(c));
      } catch (err) {
        ui.notify("error", errorMessage(err));
      }
    }
  }

  const align = (kind: string) => (kind === "number" ? "text-right" : "text-left");
</script>

{#if result && result.columns.length > 0}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={scroller}
    bind:clientHeight={viewport}
    class="relative h-full overflow-auto outline-none focus-visible:ring-1 focus-visible:ring-accent/50"
    role="grid"
    tabindex="0"
    aria-label="Query result"
    aria-rowcount={order.length + 1}
    aria-colcount={result.columns.length}
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    onkeydown={onKey}
  >
    <div style="width: {width}px">
      <div class="sticky top-0 z-10 flex border-b border-line bg-panel text-xs font-medium" role="row" style="height: {ROW_H}px">
        <div class="shrink-0 border-r border-line px-2 py-1.5 text-right text-fg-muted" style="width: {NUM_W}px" role="columnheader">#</div>
        {#each result.columns as c, i (i)}
          <button
            class="flex shrink-0 items-center gap-1 truncate border-r border-line px-2 py-1.5 hover:bg-panel-hover {align(c.kind)} {c.kind === 'number' ? 'flex-row-reverse' : ''}"
            style="width: {COL_W}px"
            role="columnheader"
            aria-sort={tab.sort.col === i ? (tab.sort.dir === "asc" ? "ascending" : "descending") : "none"}
            title="{c.name} · {c.data_type}. Click to sort the loaded rows"
            onclick={() => databases.sortBy(tab.id, i)}
          >
            <span class="truncate">{c.name}</span>
            <span class="shrink-0 font-normal text-fg-muted/70">{c.data_type}</span>
            {#if tab.sort.col === i}
              {#if tab.sort.dir === "asc"}<ArrowUp size={12} class="shrink-0" />{:else}<ArrowDown size={12} class="shrink-0" />{/if}
            {/if}
          </button>
        {/each}
      </div>

      <div style="height: {order.length * ROW_H}px; position: relative">
        <div style="transform: translateY({range.start * ROW_H}px)">
          {#each shown as ri, k (ri)}
            {@const row = result.rows[ri]}
            <div class="flex border-b border-line/50 text-xs {(range.start + k) % 2 ? 'bg-panel/40' : ''}" role="row" aria-rowindex={range.start + k + 2} style="height: {ROW_H}px">
              <div class="shrink-0 border-r border-line/50 px-2 py-1.5 text-right text-fg-muted/70" style="width: {NUM_W}px">{ri + 1}</div>
              {#each result.columns as c, ci (ci)}
                {@const cell = row[ci]}
                {@const isSel = selected?.row === ri && selected.col === ci}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="shrink-0 border-r border-line/50 px-2 py-1.5 font-mono {align(c.kind)} {isSel ? 'bg-accent/15 outline outline-1 -outline-offset-1 outline-accent/60' : ''}"
                  style="width: {COL_W}px"
                  role="gridcell"
                  tabindex="-1"
                  aria-selected={isSel}
                  onclick={() => (selected = { row: ri, col: ci })}
                  ondblclick={() => startEdit(ri, ci)}
                >
                  {#if editing?.row === ri && editing.col === ci}
                    <div class="-mx-1 -my-1 flex items-center gap-1">
                      <input
                        bind:this={editInput}
                        class="input h-6 min-w-0 flex-1 px-1 py-0 font-mono text-xs"
                        bind:value={editing.value}
                        aria-label="Edit {c.name}"
                        onkeydown={(e) => {
                          e.stopPropagation();
                          // Without this the same Enter lands on the confirmation dialog this opens.
                          if (e.key === "Enter") {
                            e.preventDefault();
                            void commit(editing!.value);
                          }
                          if (e.key === "Escape") {
                            e.preventDefault();
                            editing = null;
                            scroller?.focus();
                          }
                        }}
                        onblur={() => (editing = null)}
                      />
                      <button class="btn-secondary h-6 shrink-0 px-1.5 py-0 text-[10px]" title="Set this value to NULL" onmousedown={(e) => e.preventDefault()} onclick={() => commit(null)}>NULL</button>
                    </div>
                  {:else if cell === null}
                    <span class="italic text-fg-muted/60">NULL</span>
                  {:else if isCut(cell)}
                    <span class="truncate text-warning" title="Too long to show in full ({cell.bytes} bytes)">{cell.preview.slice(0, 200)}…</span>
                  {:else}
                    <span class="block truncate" title={String(cell).length > 30 ? String(cell).slice(0, 500) : undefined}>{cell}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    </div>
  </div>
{/if}
