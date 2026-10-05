<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    ChevronDown,
    ChevronRight,
    Columns3,
    Database,
    Download,
    Eye,
    History,
    KeyRound,
    Loader2,
    Pencil,
    Play,
    Plus,
    RefreshCw,
    Square,
    Table2,
    TriangleAlert,
    Trash2,
    Unplug,
    X,
  } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Badge from "./Badge.svelte";
  import EmptyState from "./EmptyState.svelte";
  import Kbd from "./Kbd.svelte";
  import ResultGrid from "./ResultGrid.svelte";
  import * as api from "$lib/api";
  import { toCsv, toJson, toTsv, quoteName } from "$lib/dbdata";
  import { ask } from "$lib/dialogs.svelte";
  import { databases, pathKey, ROW_LIMITS, type QueryTab } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type DbTreeNode, type Uuid } from "$lib/types";

  const connections = $derived(
    vaultStore.dbConnections
      .filter((c) => c.data)
      .sort((a, b) => (a.data!.group || "￿").localeCompare(b.data!.group || "￿") || a.data!.name.localeCompare(b.data!.name)),
  );

  let collapsed = $state<Record<Uuid, boolean>>({});
  let editor = $state<HTMLTextAreaElement>();
  let showHistory = $state(false);
  let showExport = $state(false);
  let now = $state(Date.now());

  // Elapsed time of a running query.
  const timer = setInterval(() => (now = Date.now()), 250);
  onDestroy(() => clearInterval(timer));

  const tab = $derived(databases.active);

  function kindIcon(kind: DbTreeNode["kind"]) {
    return { database: Database, table: Table2, view: Eye, column: Columns3, index: KeyRound }[kind];
  }

  async function connectAndShow(id: Uuid) {
    collapsed[id] = false;
    await databases.connect(id);
  }

  async function removeConnection(id: Uuid) {
    const c = vaultStore.dbConnections.find((x) => x.id === id);
    if (!(await ask(`Delete the connection "${c?.data?.name}"? Its saved password goes with it, on every device.`))) return;
    await databases.disconnect(id);
    for (const t of databases.tabs.filter((t) => t.connId === id)) await databases.closeTab(t.id);
    try {
      await vaultStore.deleteDbConnection(id);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function selectStarOf(db: string, table: string) {
    return `SELECT * FROM ${quoteName(db, table)}`;
  }

  function newQueryFor(connId: Uuid, sql = "") {
    databases.newTab(connId, sql);
    queueMicrotask(() => editor?.focus());
  }

  function runFromEditor() {
    if (!tab || !editor) return;
    // Run the selection if there is one, else the whole editor.
    const sel = editor.value.slice(editor.selectionStart, editor.selectionEnd);
    void databases.run(tab.id, sel.trim() ? sel : undefined);
  }

  function onEditorKey(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      runFromEditor();
    }
  }

  const elapsed = (t: QueryTab) => (t.running ? ((now - t.startedAt) / 1000).toFixed(1) + " s" : "");

  const summary = (t: QueryTab) => {
    const r = t.result;
    if (!r) return "";
    if (r.columns.length === 0) {
      const n = r.affected_rows ?? 0;
      return `OK, ${n} row${n === 1 ? "" : "s"} affected${r.last_insert_id ? `, last insert id ${r.last_insert_id}` : ""} · ${r.elapsed_ms} ms`;
    }
    return `${r.rows.length.toLocaleString()} row${r.rows.length === 1 ? "" : "s"} · ${r.elapsed_ms} ms`;
  };

  async function copyAs(fmt: "csv" | "tsv" | "json") {
    showExport = false;
    const r = tab?.result;
    if (!r) return;
    try {
      await writeText(fmt === "csv" ? toCsv(r) : fmt === "tsv" ? toTsv(r) : toJson(r));
      ui.notify("info", `Copied ${r.rows.length.toLocaleString()} rows as ${fmt.toUpperCase()}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function saveAs(fmt: "csv" | "json") {
    showExport = false;
    const r = tab?.result;
    if (!r || !tab) return;
    const path = await save({
      title: "Export the result",
      defaultPath: `${tab.title.replace(/[^\w.-]+/g, "_")}.${fmt}`,
      filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }],
    });
    if (!path) return;
    try {
      await api.db.saveExport(path, fmt === "csv" ? toCsv(r) : toJson(r));
      ui.notify("info", `Saved ${r.rows.length.toLocaleString()} rows to ${path}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function loadFromHistory(sql: string) {
    showHistory = false;
    if (tab) tab.sql = sql;
    else if (connections[0]) newQueryFor(connections[0].id, sql);
    queueMicrotask(() => editor?.focus());
  }

  function onWindowClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest("[data-menu]")) {
      showHistory = false;
      showExport = false;
    }
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="flex min-h-0 flex-1 bg-base">
  <!-- Connections and their trees -->
  <aside class="flex w-72 shrink-0 flex-col border-r border-line bg-panel" aria-label="Database connections">
    <div class="flex items-center justify-between gap-2 border-b border-line px-3 py-2.5">
      <h1 class="flex items-center gap-2 text-sm font-semibold"><Database size={16} class="text-accent" /> Databases</h1>
      <button class="icon-btn" title="New database connection" aria-label="New database connection" onclick={() => (ui.modal = { kind: "db-connection", id: null })}>
        <Plus size={16} />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto py-1">
      {#if connections.length === 0}
        <EmptyState icon={Database} text="No database connections yet.">
          <button class="btn-primary" onclick={() => (ui.modal = { kind: "db-connection", id: null })}>New connection</button>
        </EmptyState>
      {/if}

      {#each connections as rec, i (rec.id)}
        {@const c = rec.data!}
        {@const s = databases.sessions[rec.id]}
        {@const env = envInfo(c.environment)}
        {#if i === 0 || (connections[i - 1].data!.group || "") !== (c.group || "")}
          {#if c.group}<div class="px-3 pb-0.5 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">{c.group}</div>{/if}
        {/if}
        <div class="group">
          <div class="flex items-center gap-1 px-2 py-1 hover:bg-panel-hover">
            <button
              class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
              onclick={() => (s ? (collapsed[rec.id] = !collapsed[rec.id]) : connectAndShow(rec.id))}
              ondblclick={() => !s && connectAndShow(rec.id)}
              aria-expanded={s ? !collapsed[rec.id] : undefined}
              title={s ? `Connected · ${s.version}` : "Connect"}
            >
              {#if databases.connecting[rec.id]}
                <Loader2 size={13} class="shrink-0 animate-spin text-fg-muted" />
              {:else if s}
                {#if collapsed[rec.id]}<ChevronRight size={13} class="shrink-0" />{:else}<ChevronDown size={13} class="shrink-0" />{/if}
              {:else}
                <span class="ml-0.5 mr-0.5 h-2 w-2 shrink-0 rounded-full bg-fg-muted/40"></span>
              {/if}
              <span class="truncate text-sm {s ? 'font-medium' : ''}">{c.name}</span>
              {#if env.short}<span class="shrink-0"><Badge tone={env.tone ?? "neutral"}>{env.short}</Badge></span>{/if}
              {#if c.tls === "disable" && !c.ssh_host_id}<span class="shrink-0 text-warning" title="Not encrypted: the password and rows cross the network in clear text"><TriangleAlert size={12} aria-label="Not encrypted" /></span>{/if}
            </button>
            <div class="flex shrink-0 items-center opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
              {#if s}
                <button class="icon-btn h-6 w-6" title="New query" aria-label="New query on {c.name}" onclick={() => newQueryFor(rec.id)}><Plus size={13} /></button>
                <button class="icon-btn h-6 w-6" title="Refresh the tree" aria-label="Refresh {c.name}" onclick={() => databases.refresh(rec.id)}><RefreshCw size={13} /></button>
                <button class="icon-btn h-6 w-6" title="Disconnect" aria-label="Disconnect {c.name}" onclick={() => databases.disconnect(rec.id)}><Unplug size={13} /></button>
              {/if}
              <button class="icon-btn h-6 w-6" title="Edit" aria-label="Edit {c.name}" onclick={() => (ui.modal = { kind: "db-connection", id: rec.id })}><Pencil size={13} /></button>
              <button class="icon-btn h-6 w-6" title="Delete" aria-label="Delete {c.name}" onclick={() => removeConnection(rec.id)}><Trash2 size={13} /></button>
            </div>
          </div>

          {#if s && !collapsed[rec.id]}
            {@render level(rec.id, [], 1)}
          {/if}
        </div>
      {/each}
    </div>
  </aside>

  <!-- Tabs, editor, result -->
  <section class="flex min-w-0 flex-1 flex-col" aria-label="Query workspace">
    {#if databases.tabs.length === 0}
      <div class="flex flex-1 items-center justify-center">
        <EmptyState icon={Database} text={connections.length ? "Connect to a database, then open a table or start a query." : "Add a connection to get started."}>
          {#if connections.length}
            <button class="btn-primary" onclick={() => connectAndShow(connections[0].id).then(() => newQueryFor(connections[0].id))}>New query on {connections[0].data!.name}</button>
          {/if}
        </EmptyState>
      </div>
    {:else}
      <div class="flex shrink-0 overflow-x-auto border-b border-line bg-panel" role="tablist" aria-label="Queries">
        {#each databases.tabs as t (t.id)}
          {@const prod = databases.isProduction(t.connId)}
          <div class="flex shrink-0 items-center border-r border-line {t.id === databases.activeTabId ? 'bg-base' : 'hover:bg-panel-hover'} {prod ? 'border-t-2 border-t-danger' : 'border-t-2 border-t-transparent'}">
            <button
              role="tab"
              aria-selected={t.id === databases.activeTabId}
              class="flex max-w-56 items-center gap-1.5 px-3 py-1.5 text-xs"
              title="{t.title} on {databases.connName(t.connId)}"
              onclick={() => (databases.activeTabId = t.id)}
            >
              {#if t.running}<Loader2 size={12} class="shrink-0 animate-spin" />{:else if t.table}<Table2 size={12} class="shrink-0 text-fg-muted" />{/if}
              <span class="truncate">{t.title}</span>
            </button>
            <button class="icon-btn mr-1 h-5 w-5" title="Close" aria-label="Close {t.title}" onclick={() => databases.closeTab(t.id)}><X size={12} /></button>
          </div>
        {/each}
      </div>

      {#if tab}
        <div class="flex shrink-0 flex-col border-b border-line">
          <textarea
            bind:this={editor}
            class="h-32 min-h-16 resize-y border-0 bg-base p-3 font-mono text-sm outline-none"
            bind:value={tab.sql}
            onkeydown={onEditorKey}
            spellcheck="false"
            autocomplete="off"
            placeholder="SELECT * FROM … (Ctrl+Enter to run; select text to run only that)"
            aria-label="SQL for {tab.title}"
          ></textarea>
          <div class="flex flex-wrap items-center gap-2 border-t border-line bg-panel px-3 py-1.5">
            {#if tab.running}
              <button class="btn-danger border border-danger/40 py-1 text-xs" onclick={() => databases.cancel(tab!.id)}><Square size={12} /> Cancel</button>
              <span class="font-mono text-xs text-fg-muted" aria-live="off">{elapsed(tab)}</span>
            {:else}
              <button class="btn-primary py-1 text-xs" disabled={!tab.sql.trim()} onclick={runFromEditor}><Play size={12} /> Run <Kbd keys="Ctrl+Enter" /></button>
            {/if}
            <label class="flex items-center gap-1.5 text-xs text-fg-muted">
              Rows
              <select class="input h-7 w-24 py-0 text-xs" bind:value={tab.limit} aria-label="Row limit">
                {#each ROW_LIMITS as n (n)}<option value={n}>{n.toLocaleString()}</option>{/each}
              </select>
            </label>
            <span class="text-xs text-fg-muted">on {databases.connName(tab.connId)}</span>

            <div class="ml-auto flex items-center gap-1" data-menu>
              <div class="relative">
                <button class="btn-secondary py-1 text-xs" onclick={() => ((showHistory = !showHistory), (showExport = false))} aria-expanded={showHistory}><History size={12} /> History</button>
                {#if showHistory}
                  <div class="absolute right-0 top-full z-20 mt-1 max-h-80 w-96 overflow-y-auto rounded-lg border border-line bg-panel py-1 shadow-xl" role="menu">
                    {#each databases.history as h (h.at + h.sql)}
                      <button class="block w-full px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => loadFromHistory(h.sql)}>
                        <span class="block truncate font-mono text-xs">{h.sql.replace(/\s+/g, " ")}</span>
                        <span class="block text-[11px] text-fg-muted">{h.connection} · {new Date(h.at).toLocaleString()}</span>
                      </button>
                    {:else}
                      <p class="px-3 py-3 text-xs text-fg-muted">Nothing yet. History stays on this computer and is never synced.</p>
                    {/each}
                  </div>
                {/if}
              </div>
              <div class="relative">
                <button class="btn-secondary py-1 text-xs" disabled={!tab.result?.columns.length} onclick={() => ((showExport = !showExport), (showHistory = false))} aria-expanded={showExport}><Download size={12} /> Export</button>
                {#if showExport}
                  <div class="absolute right-0 top-full z-20 mt-1 w-48 rounded-lg border border-line bg-panel py-1 shadow-xl" role="menu">
                    {#each [["Copy as CSV", () => copyAs("csv")], ["Copy as TSV", () => copyAs("tsv")], ["Copy as JSON", () => copyAs("json")], ["Save as CSV…", () => saveAs("csv")], ["Save as JSON…", () => saveAs("json")]] as [label, run] (label)}
                      <button class="block w-full px-3 py-1.5 text-left text-xs hover:bg-panel-hover" role="menuitem" onclick={run as () => void}>{label}</button>
                    {/each}
                  </div>
                {/if}
              </div>
            </div>
          </div>
        </div>

        <div class="flex min-h-0 flex-1 flex-col">
          {#if tab.error}
            <p class="m-3 whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{tab.error}</p>
          {:else if tab.running && !tab.result}
            <div class="flex flex-1 items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Running…</div>
          {:else if tab.result}
            <div class="flex shrink-0 items-center gap-3 border-b border-line px-3 py-1 text-xs text-fg-muted">
              <span>{summary(tab)}</span>
              {#if tab.result.truncated}
                <span class="text-warning">Stopped at {tab.limit.toLocaleString()} rows. Raise the row limit or narrow the query to see more.</span>
              {/if}
              {#if tab.table && tab.result.columns.length}
                <span class="ml-auto">{tab.info?.primary_key.length ? "Double-click a cell to edit it" : tab.info ? "No primary key: read-only" : ""}</span>
              {/if}
            </div>
            <div class="min-h-0 flex-1">
              <ResultGrid {tab} />
            </div>
          {:else}
            <div class="flex flex-1 items-center justify-center text-sm text-fg-muted">Run a statement to see its result here.</div>
          {/if}
        </div>
      {/if}
    {/if}
  </section>
</div>

{#snippet level(connId: Uuid, path: string[], depth: number)}
  {@const s = databases.sessions[connId]}
  {@const key = pathKey(path)}
  {#if s?.failed[key]}
    <p class="px-3 py-1 text-xs text-danger" style="padding-left: {depth * 14 + 8}px" role="alert">{s.failed[key]}</p>
  {:else if s?.loading[key] && !s.nodes[key]}
    <p class="flex items-center gap-1.5 px-3 py-1 text-xs text-fg-muted" style="padding-left: {depth * 14 + 8}px"><Loader2 size={11} class="animate-spin" /> Loading…</p>
  {:else}
    {#each s?.nodes[key] ?? [] as node (node.kind + node.name)}
      {@const childPath = [...path, node.name]}
      {@const ck = pathKey(childPath)}
      {@const Icon = kindIcon(node.kind)}
      {@const isTable = node.kind === "table" || node.kind === "view"}
      <div class="group/node flex items-center gap-1 pr-2 hover:bg-panel-hover" style="padding-left: {depth * 14}px">
        <button
          class="flex min-w-0 flex-1 items-center gap-1.5 py-0.5 text-left"
          onclick={() => node.expandable && databases.toggle(connId, childPath)}
          ondblclick={() => isTable && databases.openTable(connId, path[0], node.name)}
          aria-expanded={node.expandable ? !!s?.open[ck] : undefined}
          title={isTable ? "Double-click to open its rows" : node.detail}
        >
          {#if node.expandable}
            {#if s?.open[ck]}<ChevronDown size={12} class="shrink-0" />{:else}<ChevronRight size={12} class="shrink-0" />{/if}
          {:else}
            <span class="w-3 shrink-0"></span>
          {/if}
          <Icon size={13} class="shrink-0 {node.kind === 'database' ? 'text-accent' : 'text-fg-muted'}" />
          <span class="truncate text-xs {node.detail === 'system' ? 'text-fg-muted' : ''}">{node.name}</span>
          {#if node.detail}<span class="ml-1 shrink-0 truncate text-[11px] text-fg-muted/70">{node.detail}</span>{/if}
        </button>
        {#if isTable}
          <button class="icon-btn h-5 w-5 opacity-0 focus:opacity-100 group-hover/node:opacity-100" title="Open rows" aria-label="Open rows of {node.name}" onclick={() => databases.openTable(connId, path[0], node.name)}><Table2 size={12} /></button>
          <button class="icon-btn h-5 w-5 opacity-0 focus:opacity-100 group-hover/node:opacity-100" title="New query" aria-label="New query on {node.name}" onclick={() => newQueryFor(connId, selectStarOf(path[0], node.name))}><Plus size={12} /></button>
        {/if}
      </div>
      {#if node.expandable && s?.open[ck]}
        {@render level(connId, childPath, depth + 1)}
      {/if}
    {/each}
  {/if}
{/snippet}
