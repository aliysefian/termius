<script lang="ts">
  import { Eye, Loader2, Lock, SquareCode, Table2, Upload } from "lucide-svelte";
  import TableConstraints from "./TableConstraints.svelte";
  import TableData from "./TableData.svelte";
  import TableDdl from "./TableDdl.svelte";
  import TableIndexes from "./TableIndexes.svelte";
  import TableRelationships from "./TableRelationships.svelte";
  import TableStructure from "./TableStructure.svelte";
  import { engineInfo } from "$lib/dbengines";
  import { estimateText, formatBytes } from "$lib/dbinspect";
  import { databases, type QueryTab, type TableView } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let { tab }: { tab: QueryTab } = $props();

  const engine = $derived(databases.engineOf(tab.connId));
  const sql = $derived(engineInfo(engine).sql);
  const meta = $derived(tab.meta);
  const supported = $derived(!!meta?.supported);
  const readOnlyConn = $derived(databases.isReadOnlyConnection(tab.connId));
  const transfer = $derived(engine === "postgres" || engine === "mysql");
  const isView = $derived(meta?.kind?.includes("view") ?? false);

  const TABS = $derived<{ id: TableView; label: string; show: boolean }[]>([
    { id: "data", label: "Data", show: true },
    { id: "structure", label: "Structure", show: !!tab.info },
    { id: "indexes", label: "Indexes", show: supported },
    { id: "constraints", label: "Constraints", show: supported },
    { id: "relationships", label: "Relationships", show: supported },
    { id: "ddl", label: "DDL", show: supported && !!meta?.ddl },
  ]);
  const visible = $derived(TABS.filter((t) => t.show));

  // A tab the engine turns out not to support falls back to the data.
  $effect(() => {
    if (!visible.some((t) => t.id === tab.view) && !tab.metaLoading) tab.view = "data";
  });

  function tabKey(e: KeyboardEvent) {
    const i = visible.findIndex((x) => x.id === tab.view);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? visible.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = visible[(to + visible.length) % visible.length];
    tab.view = next.id;
    queueMicrotask(() => document.getElementById(`tbl-tab-${next.id}`)?.focus());
  }

  function exportTable() {
    if (tab.table) ui.modal = { kind: "db-export", connId: tab.connId, database: tab.table.database, table: tab.table.table };
  }
  function importTable() {
    if (tab.table) ui.modal = { kind: "db-import", connId: tab.connId, database: tab.table.database, table: tab.table.table };
  }

  function openRelated(schema: string, table: string) {
    void databases.openTable(tab.connId, schema, table);
  }

  function asQuery(text?: string) {
    databases.newTab(tab.connId, text ?? tab.sql, `${tab.title} query`);
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <header class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line bg-panel px-3 py-2">
    <div class="flex min-w-0 items-center gap-2">
      {#if isView}<Eye size={15} class="shrink-0 text-fg-muted" />{:else}<Table2 size={15} class="shrink-0 text-accent" />{/if}
      <h2 class="truncate font-mono text-sm font-semibold" title="{tab.table?.database}.{tab.table?.table}">{tab.table?.database}.{tab.table?.table}</h2>
      {#if meta?.kind}<span class="rounded border border-line px-1.5 py-0.5 text-[11px] text-fg-muted">{meta.kind}</span>{/if}
      {#if readOnlyConn}<span class="inline-flex items-center gap-1 text-[11px] text-success" title="This connection starts every session read-only."><Lock size={11} /> read-only</span>{/if}
    </div>
    <p class="flex flex-wrap items-center gap-x-3 text-xs text-fg-muted">
      {#if tab.metaLoading && !meta}<span class="inline-flex items-center gap-1"><Loader2 size={11} class="animate-spin" /> Reading details…</span>{/if}
      {#if meta && meta.approx_rows !== null}<span title="What the server believes; it is not a count">{estimateText(meta.approx_rows)}</span>{/if}
      {#if meta && meta.size_bytes !== null}<span>{formatBytes(meta.size_bytes)} on disk</span>{/if}
      {#if meta?.comment}<span class="max-w-96 truncate" title={meta.comment}>{meta.comment}</span>{/if}
      {#if tab.metaError}<span class="text-warning" title={tab.metaError}>Details unavailable</span>{/if}
    </p>
    <div class="ml-auto flex items-center gap-1.5">
      <button class="btn-secondary h-7 px-2 text-xs" onclick={() => asQuery()} title="Open this page's SQL in a query tab"><SquareCode size={12} /> Query</button>
      {#if transfer && !isView}<button class="btn-secondary h-7 px-2 text-xs" disabled={readOnlyConn || !databases.sessions[tab.connId]?.changes} title={readOnlyConn ? "This connection is read-only." : "Import rows from a CSV file"} onclick={importTable}><Upload size={12} /> Import…</button>{/if}
    </div>
  </header>

  <div class="flex shrink-0 gap-1 overflow-x-auto border-b border-line bg-panel px-2" role="tablist" aria-label="Table views">
    {#each visible as t (t.id)}
      <button
        id="tbl-tab-{t.id}"
        role="tab"
        aria-selected={tab.view === t.id}
        aria-controls="tbl-panel"
        tabindex={tab.view === t.id ? 0 : -1}
        class="-mb-px shrink-0 border-b-2 px-3 py-1.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent {tab.view === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
        onclick={() => (tab.view = t.id)}
        onkeydown={tabKey}
      >{t.label}{#if t.id === "indexes" && meta}<span class="ml-1 text-fg-muted">{meta.indexes.length}</span>{/if}{#if t.id === "constraints" && meta}<span class="ml-1 text-fg-muted">{meta.constraints.length}</span>{/if}{#if t.id === "relationships" && meta}<span class="ml-1 text-fg-muted">{meta.constraints.filter((c) => c.kind === "foreign_key").length + meta.referenced_by.length}</span>{/if}</button>
    {/each}
  </div>

  <div id="tbl-panel" role="tabpanel" aria-labelledby="tbl-tab-{tab.view}" class="min-h-0 flex-1 overflow-hidden">
    {#if tab.view === "data" || !sql}
      <TableData {tab} onexport={exportTable} onimport={importTable} />
    {:else if tab.view === "structure"}
      <div class="h-full overflow-auto"><TableStructure info={tab.info} {meta} /></div>
    {:else if !meta}
      <div class="flex h-full items-center justify-center gap-2 text-sm text-fg-muted">{#if tab.metaLoading}<Loader2 size={16} class="animate-spin" /> Reading…{:else}{tab.metaError ?? "Nothing to show."}{/if}</div>
    {:else if tab.view === "indexes"}
      <div class="h-full overflow-auto"><TableIndexes {meta} /></div>
    {:else if tab.view === "constraints"}
      <div class="h-full overflow-auto"><TableConstraints {meta} /></div>
    {:else if tab.view === "relationships"}
      <div class="h-full overflow-auto"><TableRelationships {meta} schema={tab.table?.database ?? ""} table={tab.table?.table ?? ""} onopen={openRelated} /></div>
    {:else}
      <TableDdl {meta} onquery={(t) => asQuery(t)} />
    {/if}
  </div>
</div>
