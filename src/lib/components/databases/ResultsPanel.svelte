<script lang="ts">
  import { ChevronDown, Download, Loader2, Search, X } from "lucide-svelte";
  import CellViewer from "./CellViewer.svelte";
  import DataGrid from "./DataGrid.svelte";
  import HistoryPanel from "./HistoryPanel.svelte";
  import MessagesList from "./MessagesList.svelte";
  import SavedPanel from "./SavedPanel.svelte";
  import { copyResult, saveResult } from "$lib/dbexport";
  import { databases, type QueryTab, type ResultTab } from "$lib/stores/databases.svelte";
  import type { SavedQuery } from "$lib/savedqueries";

  let { tab, now, onopensql, onopensaved }: { tab: QueryTab; now: number; onopensql: (sql: string) => void; onopensaved: (q: SavedQuery) => void } = $props();

  let find = $state("");
  let exportOpen = $state(false);

  const TABS: { id: ResultTab; label: string }[] = [
    { id: "results", label: "Results" },
    { id: "messages", label: "Messages" },
    { id: "history", label: "Query history" },
    { id: "saved", label: "Saved queries" },
  ];

  const r = $derived(tab.result);
  const elapsed = $derived(tab.running ? ((now - tab.startedAt) / 1000).toFixed(1) + " s" : "");
  const errors = $derived(tab.messages.filter((m) => m.kind === "error").length);

  const summary = $derived.by(() => {
    if (!r) return "";
    if (r.columns.length === 0) {
      const n = r.affected_rows ?? 0;
      return `${n.toLocaleString()} row${n === 1 ? "" : "s"} affected${r.last_insert_id ? ` · last insert id ${r.last_insert_id}` : ""} · ${r.elapsed_ms} ms`;
    }
    return `${r.rows.length.toLocaleString()} row${r.rows.length === 1 ? "" : "s"} returned · ${r.elapsed_ms} ms`;
  });

  function onWindowClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest("[data-export-menu]")) exportOpen = false;
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="flex h-full min-h-0 flex-col">
  <div class="flex shrink-0 items-center gap-0.5 border-b border-line bg-panel px-2" role="tablist" aria-label="Result sections">
    {#each TABS as t (t.id)}
      <button
        role="tab"
        aria-selected={tab.resultTab === t.id}
        class="relative -mb-px flex items-center gap-1.5 border-b-2 px-2.5 py-1.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent {tab.resultTab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
        onclick={() => (tab.resultTab = t.id)}
      >
        {t.label}
        {#if t.id === "results" && r && r.columns.length}<span class="rounded bg-fg-muted/15 px-1 font-mono text-[10px]">{r.rows.length.toLocaleString()}</span>{/if}
        {#if t.id === "messages" && errors}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{errors}</span>{/if}
      </button>
    {/each}
    {#if tab.running}
      <span class="ml-auto flex items-center gap-1.5 pr-2 font-mono text-xs text-fg-muted" role="status"><Loader2 size={12} class="animate-spin" /> Running {elapsed}</span>
    {/if}
  </div>

  <div class="min-h-0 flex-1 overflow-hidden">
    {#if tab.resultTab === "results"}
      <div class="flex h-full min-h-0 flex-col">
        {#if tab.error && !tab.running}
          <p class="m-3 whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{tab.error}</p>
        {:else if tab.running && !r}
          <div class="flex flex-1 items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Running…</div>
        {:else if r}
          <div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-3 py-1 text-xs text-fg-muted">
            {#if tab.sets.length > 1}
              <div class="flex gap-1" role="group" aria-label="Result sets">
                {#each tab.sets as set, i (i)}
                  <button class="rounded px-2 py-0.5 {tab.activeSet === i ? 'bg-accent/20 text-accent' : 'hover:bg-panel-hover'}" aria-pressed={tab.activeSet === i} title={set.sql} onclick={() => databases.selectSet(tab.id, i)}>{set.title}</button>
                {/each}
              </div>
            {/if}
            <span data-testid="result-summary">{summary}</span>
            {#if r.truncated}<span class="text-warning">Stopped at {tab.limit.toLocaleString()} rows. Raise the row limit or narrow the query to see more.</span>{/if}
            {#if tab.table && r.columns.length}<span>{tab.info?.primary_key.length ? "Double-click a cell to edit it" : tab.info ? "No primary key: read-only" : ""}</span>{/if}
            {#if r.columns.length}
              <div class="relative ml-auto w-44">
                <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2" />
                <input class="input h-6 w-full pl-6 pr-6 text-xs" type="search" placeholder="Search loaded rows" aria-label="Search loaded rows" bind:value={find} />
                {#if find}<button class="icon-btn absolute right-0.5 top-1/2 h-5 w-5 -translate-y-1/2" aria-label="Clear the search" onclick={() => (find = "")}><X size={11} /></button>{/if}
              </div>
              <div class="relative" data-export-menu>
                <button class="btn-secondary h-6 px-2 text-xs" aria-expanded={exportOpen} onclick={() => (exportOpen = !exportOpen)}><Download size={12} /> Export <ChevronDown size={11} /></button>
                {#if exportOpen}
                  <div class="absolute right-0 top-full z-20 mt-1 w-52 rounded-lg border border-line bg-panel py-1 shadow-xl" role="menu">
                    {#each [["Copy as CSV", () => copyResult(r, "csv")], ["Copy as TSV", () => copyResult(r, "tsv")], ["Copy as JSON", () => copyResult(r, "json")], ["Save as CSV…", () => saveResult(r, "csv", tab.title)], ["Save as JSON…", () => saveResult(r, "json", tab.title)]] as [label, run] (label)}
                      <button class="block w-full px-3 py-1.5 text-left text-xs hover:bg-panel-hover" role="menuitem" onclick={() => { exportOpen = false; void (run as () => Promise<void>)(); }}>{label}</button>
                    {/each}
                    <p class="border-t border-line px-3 py-1.5 text-[11px] text-fg-muted">The {r.rows.length.toLocaleString()} loaded rows. To export a whole table, use Export rows… on it in the explorer.</p>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
          <div class="flex min-h-0 flex-1">
            <div class="min-h-0 min-w-0 flex-1"><DataGrid {tab} query={find} /></div>
            {#if tab.viewer}
              <div class="w-80 shrink-0"><CellViewer column={tab.viewer.column} type={tab.viewer.type} text={tab.viewer.text} onclose={() => (tab.viewer = null)} /></div>
            {/if}
          </div>
        {:else}
          <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-fg-muted">Run a statement to see its result here. Ctrl+Enter runs the statement at the cursor.</div>
        {/if}
      </div>
    {:else if tab.resultTab === "messages"}
      <div class="h-full overflow-y-auto"><MessagesList messages={tab.messages} /></div>
    {:else if tab.resultTab === "history"}
      <HistoryPanel onopen={onopensql} />
    {:else}
      <SavedPanel connId={tab.connId} onopen={onopensaved} />
    {/if}
  </div>
</div>
