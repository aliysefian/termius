<script lang="ts">
  import { ArrowDown, ArrowUp, Trash2, Undo2, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { keepInView } from "$lib/actions";
  import { copyText, isCut, searchRows, sortedOrder, startWidths, subResult, toCsv, toJson, toTsv, typeBadge, visibleRange } from "$lib/dbdata";
  import { cellMark, rowBlock, shownValue } from "$lib/dbstaging";
  import { databases, type QueryTab } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type DbCell } from "$lib/types";

  let { tab, query = "" }: { tab: QueryTab; query?: string } = $props();

  const ROW_H = 28;
  const NUM_W = 64;
  const MIN_COL = 56;

  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(400);
  /** Widths and column order the person changed, valid only for the columns they were changed on. */
  let custom = $state<{ sig: string; widths: number[]; order: number[] } | null>(null);
  let anchor = $state<{ row: number; col: number } | null>(null);
  let focus = $state<{ row: number; col: number } | null>(null);
  let editing = $state<{ row: number; col: number; value: string } | null>(null);
  let editInput = $state<HTMLInputElement>();
  let menu = $state<{ x: number; y: number; row: number; col: number } | null>(null);
  let dragCol = $state<number | null>(null);
  let resizing: { col: number; startX: number; startW: number } | null = null;

  const result = $derived(tab.result);
  const paged = $derived(databases.isPaged(tab));
  const staged = $derived(databases.stagedEditing(tab));
  const readOnlyConn = $derived(databases.isReadOnlyConnection(tab.connId));
  const offset = $derived(paged ? tab.page.offset : 0);
  const sig = $derived(result ? result.columns.map((c) => c.name).join("\u0000") : "");
  // Always consistent with the columns of the result being drawn, so a new result never meets a stale order.
  const order = $derived(custom && custom.sig === sig ? custom.order : (result?.columns.map((_, i) => i) ?? []));
  const widths = $derived(custom && custom.sig === sig ? custom.widths : result ? startWidths(result) : []);

  function edit(mut: (c: { sig: string; widths: number[]; order: number[] }) => void) {
    if (!result) return;
    const c = custom && custom.sig === sig ? custom : { sig, widths: [...widths], order: [...order] };
    mut(c);
    custom = { ...c, widths: [...c.widths], order: [...c.order] };
  }

  /** Row positions in the order shown: the server's order for a paged table, else sorted here; then searched. */
  const rows = $derived.by(() => {
    if (!result) return [];
    const base = paged ? result.rows.map((_, i) => i) : sortedOrder(result.rows, tab.sort.col, tab.sort.dir, tab.sort.col === null ? undefined : result.columns[tab.sort.col]?.kind);
    if (!query.trim()) return base;
    const keep = new Set(searchRows(result.rows, query));
    return base.filter((i) => keep.has(i));
  });
  const range = $derived(visibleRange(scrollTop, viewport, ROW_H, rows.length));
  const shown = $derived(rows.slice(range.start, range.end));
  const total = $derived(NUM_W + order.reduce((n, c) => n + (widths[c] ?? 160), 0));

  // A new result starts at the top with nothing selected.
  $effect(() => {
    void tab.result;
    if (scroller) scroller.scrollTop = 0;
    anchor = focus = null;
    editing = null;
  });

  // -- what a cell shows ---------------------------------------------------------------------------------------

  function show(cell: DbCell, kind: string, type: string) {
    if (cell === null) return { text: "NULL", cls: "italic text-fg-muted" };
    if (isCut(cell)) return { text: `${cell.preview.slice(0, 200)}…`, cls: "text-warning" };
    const t = typeBadge({ kind: kind as "text", data_type: type }).key;
    const text = typeof cell === "boolean" ? String(cell) : String(cell);
    if (t === "boolean") return { text, cls: /^(t|true|1)$/i.test(text) ? "text-success" : "text-fg-muted" };
    if (t === "json") return { text, cls: "text-accent" };
    if (t === "number") return { text, cls: "" };
    return { text, cls: "" };
  }

  const align = (kind: string) => (kind === "number" ? "text-right" : "text-left");
  const badgeCls = (k: string) => ({ number: "text-sky-400", boolean: "text-emerald-400", datetime: "text-amber-400", json: "text-accent", binary: "text-rose-400", text: "text-fg-muted", other: "text-fg-muted" })[k] ?? "text-fg-muted";

  // -- selection and copying ------------------------------------------------------------------------------------

  /** The cells selected, as original row and column positions, in the order shown. */
  function selection(): { rows: number[]; cols: number[] } | null {
    if (!anchor || !focus || !result) return null;
    const r1 = Math.min(anchor.row, focus.row);
    const r2 = Math.max(anchor.row, focus.row);
    const c1 = Math.min(anchor.col, focus.col);
    const c2 = Math.max(anchor.col, focus.col);
    return { rows: rows.slice(r1, r2 + 1), cols: order.slice(c1, c2 + 1) };
  }

  const inSelection = (rp: number, cp: number) => {
    if (!anchor || !focus) return false;
    return rp >= Math.min(anchor.row, focus.row) && rp <= Math.max(anchor.row, focus.row) && cp >= Math.min(anchor.col, focus.col) && cp <= Math.max(anchor.col, focus.col);
  };

  function pick(e: MouseEvent, rp: number, cp: number) {
    if (e.shiftKey && anchor) focus = { row: rp, col: cp };
    else anchor = focus = { row: rp, col: cp };
    scroller?.focus();
  }

  function pickRow(e: MouseEvent, rp: number) {
    const last = order.length - 1;
    if (e.shiftKey && anchor) focus = { row: rp, col: last };
    else {
      anchor = { row: rp, col: 0 };
      focus = { row: rp, col: last };
    }
    scroller?.focus();
  }

  async function copy(text: string, what: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied ${what}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function copySelection(kind: "values" | "tsv" | "csv" | "json") {
    const s = selection();
    if (!s || !result) return;
    const sub = subResult(result, s.rows, s.cols);
    const n = s.rows.length * s.cols.length;
    if (kind === "values") {
      const text = sub.rows.map((r) => r.map(copyText).join("\t")).join("\n");
      await copy(text, n === 1 ? "the value" : `${n.toLocaleString()} values`);
    } else if (kind === "tsv") await copy(toTsv(sub), `${s.rows.length.toLocaleString()} row${s.rows.length === 1 ? "" : "s"} with headers`);
    else if (kind === "csv") await copy(toCsv(sub), `${s.rows.length.toLocaleString()} row${s.rows.length === 1 ? "" : "s"} as CSV`);
    else await copy(toJson(sub), `${s.rows.length.toLocaleString()} row${s.rows.length === 1 ? "" : "s"} as JSON`);
  }

  function viewValue(row: number, col: number) {
    if (!result) return;
    const c = result.columns[col];
    const cell = shownValue(tab.staging, result, row, col);
    tab.viewer = { column: c.name, type: c.data_type, text: cell === null ? "NULL" : copyText(cell) };
  }

  // -- editing ---------------------------------------------------------------------------------------------------

  function editProblem(row: number, col: number): string | null {
    if (!result) return "No rows";
    if (readOnlyConn) return "This connection is read-only.";
    if (!tab.table) return "Open a table to edit its rows.";
    return databases.editBlock(tab, row, col);
  }

  function startEdit(row: number, col: number) {
    if (!result) return;
    const block = editProblem(row, col);
    if (block) {
      ui.notify("info", block);
      return;
    }
    if (tab.staging.deletes.includes(row)) {
      ui.notify("info", "This row is marked for deletion. Undo that first.");
      return;
    }
    const cell = shownValue(tab.staging, result, row, col);
    editing = { row, col, value: cell === null ? "" : copyText(cell) };
    queueMicrotask(() => editInput?.select());
  }

  async function commit(value: string | null) {
    const e = editing;
    if (!e || !result) return;
    editing = null;
    if (staged) {
      databases.stageCell(tab.id, e.row, e.col, value);
    } else {
      const current = result.rows[e.row][e.col];
      const same = value !== null && current !== null && current !== undefined && !isCut(current) && String(current) === value;
      if (!(same || (value === null && current === null))) await databases.editCell(tab.id, e.row, e.col, value);
    }
    scroller?.focus();
  }

  // -- keyboard --------------------------------------------------------------------------------------------------

  async function onKey(e: KeyboardEvent) {
    if (editing || !result) return;
    if (!focus) {
      if (rows.length && ["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(e.key)) anchor = focus = { row: 0, col: 0 };
      return;
    }
    const meta = e.ctrlKey || e.metaKey;
    const move = (dr: number, dc: number) => {
      e.preventDefault();
      const next = { row: Math.min(rows.length - 1, Math.max(0, focus!.row + dr)), col: Math.min(order.length - 1, Math.max(0, focus!.col + dc)) };
      focus = next;
      if (!e.shiftKey) anchor = next;
      const top = next.row * ROW_H;
      if (scroller) {
        if (top < scroller.scrollTop) scroller.scrollTop = top;
        else if (top + ROW_H > scroller.scrollTop + viewport - ROW_H) scroller.scrollTop = top + ROW_H * 2 - viewport;
      }
    };
    if (e.key === "ArrowDown") move(1, 0);
    else if (e.key === "ArrowUp") move(-1, 0);
    else if (e.key === "ArrowRight") move(0, 1);
    else if (e.key === "ArrowLeft") move(0, -1);
    else if (e.key === "PageDown") move(Math.max(1, Math.floor(viewport / ROW_H) - 1), 0);
    else if (e.key === "PageUp") move(-Math.max(1, Math.floor(viewport / ROW_H) - 1), 0);
    else if (e.key === "Home") move(meta ? -rows.length : 0, -order.length);
    else if (e.key === "End") move(meta ? rows.length : 0, order.length);
    else if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault();
      viewValue(rows[focus.row], order[focus.col]);
    } else if (e.key === "Enter" || e.key === "F2") {
      e.preventDefault();
      startEdit(rows[focus.row], order[focus.col]);
    } else if (meta && e.key.toLowerCase() === "a") {
      e.preventDefault();
      anchor = { row: 0, col: 0 };
      focus = { row: rows.length - 1, col: order.length - 1 };
    } else if (meta && e.key.toLowerCase() === "c") {
      e.preventDefault();
      await copySelection(e.shiftKey ? "tsv" : "values");
    } else if (e.key === "Delete" && staged) {
      e.preventDefault();
      databases.stageDelete(tab.id, rows[focus.row]);
    } else if (e.key === "Escape") {
      anchor = focus = null;
      menu = null;
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = scroller?.getBoundingClientRect();
      menu = { x: (r?.left ?? 0) + 120, y: (r?.top ?? 0) + 80, row: rows[focus.row], col: order[focus.col] };
    }
  }

  // -- columns: resize and reorder ----------------------------------------------------------------------------------

  function startResize(e: PointerEvent, col: number) {
    e.preventDefault();
    e.stopPropagation();
    resizing = { col, startX: e.clientX, startW: widths[col] ?? 160 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function moveResize(e: PointerEvent) {
    if (resizing) {
      const { col, startW, startX } = resizing;
      edit((c) => (c.widths[col] = Math.max(MIN_COL, Math.round(startW + e.clientX - startX))));
    }
  }
  function stopResize() {
    resizing = null;
  }

  function dropOn(target: number) {
    if (dragCol === null || dragCol === target) return;
    const from = dragCol;
    edit((c) => {
      const next = c.order.filter((x) => x !== from);
      next.splice(next.indexOf(target) + (c.order.indexOf(from) < c.order.indexOf(target) ? 1 : 0), 0, from);
      c.order = next;
    });
    dragCol = null;
  }

  function headerClick(col: number) {
    if (!result) return;
    if (paged) void databases.sortPage(tab.id, result.columns[col].name);
    else databases.sortBy(tab.id, col);
  }

  const sortDir = (col: number): "asc" | "desc" | null => {
    if (!result) return null;
    if (paged) {
      const o = tab.page.order[0];
      return o && o.column === result.columns[col].name && tab.page.order.length === 1 ? (o.desc ? "desc" : "asc") : null;
    }
    return tab.sort.col === col ? tab.sort.dir : null;
  };

</script>

{#if result && result.columns.length > 0}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={scroller}
    bind:clientHeight={viewport}
    class="relative h-full overflow-auto outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent/50"
    role="grid"
    tabindex="0"
    aria-label="Query result"
    aria-rowcount={rows.length + 1}
    aria-colcount={order.length}
    data-testid="data-grid"
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    onkeydown={onKey}
  >
    <div style="width: {total}px">
      <div class="sticky top-0 z-10 flex border-b border-line bg-panel text-xs font-medium" role="row" style="height: {ROW_H}px">
        <div class="sticky left-0 z-20 shrink-0 border-r border-line bg-panel px-2 py-1.5 text-right text-fg-muted" style="width: {NUM_W}px" role="columnheader">#</div>
        {#each order as ci (ci)}
          {@const c = result.columns[ci]}
          {@const b = typeBadge(c)}
          {@const dir = sortDir(ci)}
          <div
            class="relative flex shrink-0 items-center border-r border-line {dragCol === ci ? 'opacity-50' : ''}"
            style="width: {widths[ci] ?? 160}px"
            role="columnheader"
            tabindex="-1"
            aria-sort={dir === "asc" ? "ascending" : dir === "desc" ? "descending" : "none"}
            draggable="true"
            ondragstart={() => (dragCol = ci)}
            ondragover={(e) => dragCol !== null && e.preventDefault()}
            ondrop={() => dropOn(ci)}
            ondragend={() => (dragCol = null)}
          >
            <button
              class="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 hover:bg-panel-hover {c.kind === 'number' ? 'flex-row-reverse' : ''}"
              title="{c.name} · {c.data_type}. {paged ? 'Click to sort the whole table' : 'Click to sort the loaded rows'}. Drag to move."
              onclick={() => headerClick(ci)}
            >
              <span class="truncate">{c.name}</span>
              <span class="shrink-0 font-mono text-[10px] font-normal {badgeCls(b.key)}" title={c.data_type}>{b.label}</span>
              {#if dir}{#if dir === "asc"}<ArrowUp size={12} class="shrink-0" />{:else}<ArrowDown size={12} class="shrink-0" />{/if}{/if}
            </button>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="absolute right-0 top-0 z-10 h-full w-1.5 cursor-col-resize hover:bg-accent/50"
              title="Drag to resize · double-click to fit"
              onpointerdown={(e) => startResize(e, ci)}
              onpointermove={moveResize}
              onpointerup={stopResize}
              onpointercancel={stopResize}
              ondblclick={() => edit((c) => (c.widths[ci] = startWidths(result)[ci]))}
            ></div>
          </div>
        {/each}
      </div>

      <div style="height: {rows.length * ROW_H}px; position: relative">
        <div style="transform: translateY({range.start * ROW_H}px)">
          {#each shown as ri, k (ri)}
            {@const rp = range.start + k}
            {@const deleted = tab.staging.deletes.includes(ri)}
            <div class="group/row flex border-b border-line/50 text-xs {deleted ? 'bg-danger/10 line-through decoration-danger/60' : rp % 2 ? 'bg-panel/40' : ''}" role="row" aria-rowindex={rp + 2} style="height: {ROW_H}px">
              <div class="sticky left-0 z-[5] flex shrink-0 items-center justify-end gap-0.5 border-r border-line/50 pr-1 text-fg-muted {deleted ? 'bg-danger/10' : rp % 2 ? 'bg-panel' : 'bg-base'}" style="width: {NUM_W}px">
                {#if staged}
                  <button
                    class="icon-btn h-5 w-5 opacity-0 focus:opacity-100 group-hover/row:opacity-100 {deleted ? '!opacity-100 text-danger' : ''}"
                    title={deleted ? "Keep this row" : rowBlock(tab.info, result, ri) ?? "Mark this row for deletion"}
                    aria-label={deleted ? "Keep this row" : "Mark this row for deletion"}
                    onclick={() => databases.stageDelete(tab.id, ri)}
                  >{#if deleted}<Undo2 size={12} />{:else}<Trash2 size={12} />{/if}</button>
                {/if}
                <button class="min-w-6 px-1 text-right hover:text-fg" title="Select this row" onclick={(e) => pickRow(e, rp)}>{(offset + ri + 1).toLocaleString()}</button>
              </div>
              {#each order as ci, cp (ci)}
                {@const c = result.columns[ci]}
                {@const cell = shownValue(tab.staging, result, ri, ci)}
                {@const mark = cellMark(tab.staging, ri, c.name)}
                {@const sel = inSelection(rp, cp)}
                {@const v = show(cell, c.kind, c.data_type)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="relative shrink-0 border-r border-line/50 px-2 py-1.5 font-mono {align(c.kind)} {mark === 'edited' ? 'bg-warning/15' : ''} {sel ? 'bg-accent/15 outline outline-1 -outline-offset-1 outline-accent/60' : ''}"
                  style="width: {widths[ci] ?? 160}px"
                  role="gridcell"
                  tabindex="-1"
                  aria-selected={sel}
                  onclick={(e) => pick(e, rp, cp)}
                  ondblclick={() => (editProblem(ri, ci) ? viewValue(ri, ci) : startEdit(ri, ci))}
                  oncontextmenu={(e) => {
                    e.preventDefault();
                    if (!inSelection(rp, cp)) anchor = focus = { row: rp, col: cp };
                    menu = { x: e.clientX, y: e.clientY, row: ri, col: ci };
                  }}
                >
                  {#if mark === "edited"}<span class="absolute left-0 top-0 h-full w-0.5 bg-warning" title="Edited, not saved yet"></span>{/if}
                  {#if editing?.row === ri && editing.col === ci}
                    <div class="-mx-1 -my-1 flex items-center gap-1">
                      <input
                        bind:this={editInput}
                        class="input h-6 min-w-0 flex-1 px-1 py-0 font-mono text-xs"
                        bind:value={editing.value}
                        aria-label="Edit {c.name}"
                        onkeydown={(e) => {
                          e.stopPropagation();
                          // Without this the same Enter lands on the confirmation dialog this may open.
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
                  {:else}
                    <span class="block truncate {v.cls}" title={cell !== null && !isCut(cell) && String(cell).length > 30 ? String(cell).slice(0, 500) : isCut(cell) ? `Too long to show in full (${cell.bytes} bytes). Open it with Shift+Enter.` : undefined}>{v.text}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        </div>
      </div>

      {#if staged && tab.staging.inserts.length}
        <div class="border-t-2 border-success/40" aria-label="New rows">
          {#each tab.staging.inserts as values, i (i)}
            <div class="flex border-b border-line/50 bg-success/10 text-xs" role="row" style="height: {ROW_H}px">
              <div class="sticky left-0 z-[5] flex shrink-0 items-center justify-end border-r border-line/50 bg-success/10 pr-1" style="width: {NUM_W}px">
                <button class="icon-btn h-5 w-5" title="Remove this new row" aria-label="Remove this new row" onclick={() => databases.dropNewRow(tab.id, i)}><X size={12} /></button>
              </div>
              {#each order as ci (ci)}
                {@const c = result.columns[ci]}
                {@const typed = values[c.name]}
                <div class="flex shrink-0 items-center gap-0.5 border-r border-line/50 px-1" style="width: {widths[ci] ?? 160}px">
                  <input
                    class="input h-6 min-w-0 flex-1 px-1 py-0 font-mono text-xs {typed === null ? 'italic text-fg-muted' : ''}"
                    placeholder="DEFAULT"
                    aria-label="New row, {c.name}"
                    value={typed === null ? "NULL" : (typed ?? "")}
                    oninput={(e) => databases.setNewValue(tab.id, i, c.name, e.currentTarget.value === "" ? undefined : e.currentTarget.value)}
                  />
                  <button class="btn-secondary h-6 shrink-0 px-1 py-0 text-[10px]" title="Insert NULL here" onclick={() => databases.setNewValue(tab.id, i, c.name, null)}>∅</button>
                </div>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    </div>
    {#if rows.length === 0}
      <p class="sticky left-0 px-4 py-6 text-center text-xs text-fg-muted">{query.trim() ? `No loaded row contains "${query}".` : "No rows."}</p>
    {/if}
  </div>
{/if}

{#if menu}
  {@const m = menu}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
  <div class="fixed z-50 w-60 rounded-xl border border-line bg-panel p-1 text-sm shadow-xl" use:keepInView={{ x: m.x, y: m.y }} role="menu" aria-label="Cell actions">
    {#snippet item(label: string, run: () => void, opts: { disabled?: boolean; why?: string; danger?: boolean } = {})}
      <button class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 disabled:hover:bg-transparent {opts.danger ? 'text-danger' : ''}" role="menuitem" disabled={opts.disabled} title={opts.why} onclick={() => { run(); menu = null; }}>{label}</button>
    {/snippet}
    {@render item("Copy", () => void copySelection("values"))}
    {@render item("Copy with headers", () => void copySelection("tsv"))}
    {@render item("Copy as CSV", () => void copySelection("csv"))}
    {@render item("Copy as JSON", () => void copySelection("json"))}
    {@render item("Copy column name", () => void copy(result?.columns[m.col].name ?? "", "the column name"))}
    <div class="my-1 h-px bg-line" role="separator"></div>
    {@render item("View value…", () => viewValue(m.row, m.col))}
    {#if tab.table}
      {@const why = editProblem(m.row, m.col)}
      {@render item("Edit…", () => startEdit(m.row, m.col), { disabled: !!why, why: why ?? undefined })}
      {@render item("Set to NULL", () => (staged ? databases.stageCell(tab.id, m.row, m.col, null) : void databases.editCell(tab.id, m.row, m.col, null)), { disabled: !!why, why: why ?? undefined })}
      {#if staged}
        {@const rwhy = rowBlock(tab.info, result!, m.row)}
        {@render item(tab.staging.deletes.includes(m.row) ? "Keep this row" : "Delete this row", () => databases.stageDelete(tab.id, m.row), { disabled: !!rwhy || readOnlyConn, why: readOnlyConn ? "This connection is read-only." : (rwhy ?? undefined), danger: true })}
      {/if}
    {/if}
  </div>
{/if}
