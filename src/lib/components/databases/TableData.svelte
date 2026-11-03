<script lang="ts">
  import { ChevronLeft, ChevronRight, Download, Eye, Filter, Hash, Loader2, Plus, RefreshCw, Save, Search, Undo2, X } from "lucide-svelte";
  import CellViewer from "./CellViewer.svelte";
  import DataGrid from "./DataGrid.svelte";
  import { copyResult, saveResult } from "$lib/dbexport";
  import { pageOf, rangeText } from "$lib/dbpaging";
  import { hasPending, pendingSummary } from "$lib/dbstaging";
  import { databases, PAGE_SIZES, type QueryTab } from "$lib/stores/databases.svelte";

  let { tab, onexport, onimport }: { tab: QueryTab; onexport: () => void; onimport: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let filterText = $state(tab.page.filter);
  let find = $state("");
  const r = $derived(tab.result);
  const p = $derived(tab.page);
  const staged = $derived(databases.stagedEditing(tab));
  const readOnlyConn = $derived(databases.isReadOnlyConnection(tab.connId));
  const isView = $derived(tab.meta?.kind?.includes("view") ?? false);
  const canAdd = $derived(staged && !readOnlyConn && !isView && !!tab.info);
  const pending = $derived(hasPending(tab.staging));
  const pageNo = $derived(pageOf(p.offset, p.size));
  const total = $derived(p.count);
  const lastOffset = $derived(total === null ? null : Math.max(0, Math.floor((total - 1) / p.size) * p.size));
  const hasPrev = $derived(p.offset > 0);

  function applyFilter() {
    if (filterText.trim() === p.filter.trim()) {
      void databases.refreshPage(tab.id);
      return;
    }
    void databases.setFilter(tab.id, filterText);
  }

  function clearFilter() {
    filterText = "";
    void databases.setFilter(tab.id, "");
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <!-- Filter, paging -->
  <div class="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-3 py-1.5">
    <form class="relative min-w-56 flex-1" onsubmit={(e) => { e.preventDefault(); applyFilter(); }}>
      <Filter size={12} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input
        class="input h-7 w-full pl-7 pr-7 font-mono text-xs {p.filterError ? 'border-danger' : ''}"
        placeholder="Filter rows: a WHERE condition, e.g. price > 10 AND name LIKE 'a%'"
        aria-label="Filter rows (a WHERE condition)"
        aria-invalid={!!p.filterError}
        aria-describedby={p.filterError ? "filter-error" : undefined}
        bind:value={filterText}
      />
      {#if filterText}<button type="button" class="icon-btn absolute right-0.5 top-1/2 h-6 w-6 -translate-y-1/2" aria-label="Clear the filter" onclick={clearFilter}><X size={12} /></button>{/if}
    </form>
    <button class="btn-secondary h-7 px-2 text-xs" disabled={tab.running} onclick={applyFilter}><Search size={12} /> Apply</button>
    <button class="icon-btn h-7 w-7 border border-line" title="Reload this page (F5)" aria-label="Reload this page" disabled={tab.running} onclick={() => void databases.refreshPage(tab.id)}>{#if tab.running}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}</button>
    <span class="mx-0.5 h-4 w-px bg-line"></span>
    <div class="flex items-center gap-1" role="group" aria-label="Pages">
      <button class="icon-btn h-7 w-7 border border-line" title="First page" aria-label="First page" disabled={!hasPrev || tab.running} onclick={() => void databases.goToPage(tab.id, 0)}><ChevronLeft size={13} /><ChevronLeft size={13} class="-ml-2.5" /></button>
      <button class="icon-btn h-7 w-7 border border-line" title="Previous page" aria-label="Previous page" disabled={!hasPrev || tab.running} onclick={() => void databases.goToPage(tab.id, p.offset - p.size)}><ChevronLeft size={13} /></button>
      <span class="min-w-28 text-center text-xs text-fg-muted" aria-live="polite" data-testid="page-range">{r ? rangeText(p.offset, r.rows.length) : "…"}{#if total !== null} of {total.toLocaleString()}{/if}</span>
      <button class="icon-btn h-7 w-7 border border-line" title="Next page" aria-label="Next page" disabled={!p.hasNext || tab.running} onclick={() => void databases.goToPage(tab.id, p.offset + p.size)}><ChevronRight size={13} /></button>
      <button class="icon-btn h-7 w-7 border border-line" title={lastOffset === null ? "Count the rows first to jump to the last page" : "Last page"} aria-label="Last page" disabled={lastOffset === null || p.offset >= lastOffset || tab.running} onclick={() => lastOffset !== null && void databases.goToPage(tab.id, lastOffset)}><ChevronRight size={13} /><ChevronRight size={13} class="-ml-2.5" /></button>
    </div>
    <span class="text-xs text-fg-muted">page {pageNo}</span>
    <label class="flex items-center gap-1 text-xs text-fg-muted">
      Rows
      <select class="input !w-auto h-7 py-0 text-xs" value={p.size} aria-label="Rows per page" onchange={(e) => void databases.setPageSize(tab.id, Number(e.currentTarget.value))}>
        {#each PAGE_SIZES as n (n)}<option value={n}>{n}</option>{/each}
      </select>
    </label>
    <button class="btn-secondary h-7 px-2 text-xs" disabled={p.countLoading || tab.running} title="Count every row{p.filter.trim() ? ' that matches the filter' : ''}. It reads the whole table, so it runs only when you ask." onclick={() => void databases.countRows(tab.id)}>
      {#if p.countLoading}<Loader2 size={12} class="animate-spin" />{:else}<Hash size={12} />{/if} {total === null ? "Count rows" : "Recount"}
    </button>
    <div class="ml-auto flex items-center gap-1.5">
      {#if canAdd}<button class="btn-secondary h-7 px-2 text-xs" onclick={() => databases.stageNewRow(tab.id)} title="Add a row (saved when you review and save)"><Plus size={12} /> Row</button>{/if}
      <div class="relative w-40">
        <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input h-7 w-full pl-6 text-xs" type="search" placeholder="Search this page" aria-label="Search loaded rows" bind:value={find} />
      </div>
      <button class="btn-secondary h-7 px-2 text-xs" disabled={!r} title="Copy this page as CSV" onclick={() => r && void copyResult(r, "csv")}>Copy CSV</button>
      <button class="btn-secondary h-7 px-2 text-xs" disabled={!r} title="Save this page as a CSV file" onclick={() => r && void saveResult(r, "csv", tab.title)}><Download size={12} /> CSV</button>
      <button class="btn-secondary h-7 px-2 text-xs" title="Write the whole table to a file" onclick={onexport}><Download size={12} /> Export…</button>
    </div>
  </div>
  {#if p.filterError}<p id="filter-error" class="border-b border-line bg-danger/10 px-3 py-1 text-xs text-danger" role="alert">{p.filterError}</p>{/if}

  <!-- Pending changes -->
  {#if pending}
    <div class="flex flex-wrap items-center gap-2 border-b border-warning/30 bg-warning/10 px-3 py-1.5 text-xs" role="status" data-testid="pending-bar">
      <span class="font-medium text-warning">Not saved yet</span>
      <span>{pendingSummary(tab.staging)}</span>
      <span class="text-fg-muted">These are only in this window until you save them.</span>
      <div class="ml-auto flex gap-1.5">
        <button class="btn-secondary h-7 px-2 text-xs" onclick={() => databases.discardChanges(tab.id)}><Undo2 size={12} /> Discard</button>
        <button class="btn-primary h-7 px-2 text-xs" onclick={() => void databases.reviewChanges(tab.id)} data-testid="review-changes"><Eye size={12} /> Review and save…</button>
      </div>
    </div>
  {/if}

  <div class="relative flex min-h-0 flex-1">
    <div class="min-h-0 min-w-0 flex-1">
      {#if tab.error && !tab.running && !r}
        <p class="m-3 whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{tab.error}</p>
      {:else if !r}
        <div class="flex h-full items-center justify-center gap-2 text-sm text-fg-muted">{#if tab.running}<Loader2 size={16} class="animate-spin" /> Loading…{/if}</div>
      {:else}
        {#if tab.error}<p class="whitespace-pre-wrap border-b border-danger/30 bg-danger/10 px-3 py-1.5 font-mono text-xs text-danger" role="alert">{tab.error}</p>{/if}
        <DataGrid {tab} query={find} />
      {/if}
    </div>
    {#if tab.viewer}
      <div class="w-80 shrink-0"><CellViewer column={tab.viewer.column} type={tab.viewer.type} text={tab.viewer.text} onclose={() => (tab.viewer = null)} /></div>
    {/if}

    <!-- Review: the statements that will run, before any of them does -->
    {#if tab.review}
      <div class="absolute inset-0 z-30 flex items-center justify-center bg-black/50 p-6" role="dialog" aria-modal="true" aria-label="Review changes">
        <div class="flex max-h-full w-full max-w-3xl flex-col rounded-xl border border-line bg-panel shadow-2xl">
          <header class="border-b border-line px-4 py-3">
            <h2 class="text-sm font-semibold">Review changes to {tab.table?.table}</h2>
            <p class="mt-0.5 text-xs text-fg-muted">These statements run together in one transaction. If any of them fails, none of them is saved.{#if databases.isProduction(tab.connId)} <strong class="text-danger">This is a production database.</strong>{/if}</p>
          </header>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre class="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all px-4 py-3 font-mono text-xs leading-5" tabindex="0" aria-label="Statements">{tab.review.sql.join("\n")}</pre>
          {#if tab.review.error}<p class="mx-4 mb-3 whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{tab.review.error}</p>{/if}
          <footer class="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
            <button class="btn-secondary text-xs" disabled={tab.review.busy} onclick={() => (tab.review = null)}>Back</button>
            <button class="btn-primary text-xs" disabled={tab.review.busy || tab.review.sql.length === 0} onclick={() => void databases.commitChanges(tab.id)} data-testid="commit-changes">
              {#if tab.review.busy}<Loader2 size={12} class="animate-spin" />{:else}<Save size={12} />{/if} Save {tab.review.sql.length} change{tab.review.sql.length === 1 ? "" : "s"}
            </button>
          </footer>
        </div>
      </div>
    {/if}
  </div>
</div>
