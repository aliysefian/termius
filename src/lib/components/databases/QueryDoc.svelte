<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Lock, LockOpen, Play, ListVideo, Save, Square, Wand2, ScanSearch } from "lucide-svelte";
  import Kbd from "../Kbd.svelte";
  import ResultsPanel from "./ResultsPanel.svelte";
  import SqlEditor from "../SqlEditor.svelte";
  import { canFormat, formatSql } from "$lib/dbformat";
  import { engineInfo } from "$lib/dbengines";
  import { explainSql } from "$lib/dbpaging";
  import { isScript } from "$lib/dbstatement";
  import { databases, ROW_LIMITS, type QueryTab } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import type { SavedQuery } from "$lib/savedqueries";

  let { tab, now, ongotosettings }: { tab: QueryTab; now: number; ongotosettings?: () => void } = $props();

  let editor = $state<{ focus(): void; selection(): string | null; currentStatement(): string | null; replaceAll(t: string): void }>();
  let box = $state<HTMLDivElement>();
  let editorH = $state(220);
  let dragging = false;

  const engine = $derived(databases.engineOf(tab.connId));
  const info = $derived(engineInfo(engine));
  const connReadOnly = $derived(databases.isReadOnlyConnection(tab.connId));
  const pgOnly = $derived(engine === "postgres");

  export function focus() {
    editor?.focus();
  }

  function run(mode: "statement" | "script", text: string | null) {
    const sql = text ?? (mode === "script" ? tab.sql : null);
    if (!sql?.trim()) {
      ui.notify("info", "Type a statement to run.");
      return;
    }
    void databases.run(tab.id, sql, mode);
  }

  function runButton() {
    const sel = editor?.selection();
    if (sel) run("statement", sel);
    else run("statement", editor?.currentStatement() ?? tab.sql);
  }

  async function format() {
    const sel = editor?.selection();
    const r = formatSql(engine, tab.sql);
    if ("error" in r) {
      ui.notify("info", r.error);
      return;
    }
    // The whole text, like most editors: a selection can start mid-statement.
    void sel;
    editor?.replaceAll(r.text);
    await tick();
    editor?.focus();
  }

  async function explain() {
    const sql = editor?.selection() ?? editor?.currentStatement();
    if (!sql) {
      ui.notify("info", "Put the cursor in a statement to explain it.");
      return;
    }
    void databases.explain(tab.id, sql);
  }

  async function save() {
    await databases.saveQuery(tab.id);
  }

  function openSql(sql: string) {
    tab.sql = sql;
    tab.resultTab = "results";
    void tick().then(() => editor?.focus());
  }

  function openSaved(q: SavedQuery) {
    databases.openSaved(q, tab.connId);
  }

  // The divider between the editor and the results.
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (!dragging || !box) return;
    editorH = Math.round(Math.min(box.clientHeight - 160, Math.max(96, e.clientY - box.getBoundingClientRect().top - 40)));
  }
  function up() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function keyDivider(e: KeyboardEvent) {
    if (e.key === "ArrowUp") editorH = Math.max(96, editorH - 24);
    else if (e.key === "ArrowDown") editorH = Math.min((box?.clientHeight ?? 600) - 160, editorH + 24);
    else return;
    e.preventDefault();
  }

  onMount(() => void tick().then(() => editor?.focus()));
  const dirty = $derived(databases.isDirty(tab));
  const multi = $derived(isScript(tab.sql, engine));
</script>

<div bind:this={box} class="flex h-full min-h-0 flex-col">
  <div class="shrink-0" style="height: {editorH + 40}px">
    <!-- Query toolbar. Below a certain width the buttons keep their icons and lose their words. -->
    <div class="@container h-10 border-b border-line bg-panel">
      <div class="flex h-full items-center gap-1.5 overflow-x-auto px-3" role="toolbar" aria-label="Query actions">
        {#if tab.running}
          <button class="btn-danger shrink-0 border border-danger/40 py-1 text-xs" onclick={() => databases.cancel(tab.id)} title="Stop the running query on the server"><Square size={12} /> Stop</button>
        {:else}
          <button class="btn-primary shrink-0 py-1 text-xs" disabled={!tab.sql.trim()} onclick={runButton} title="Run the selection, or the statement at the cursor (Ctrl+Enter)" data-testid="run-statement"><Play size={12} /> Run <span class="hidden @[900px]:inline"><Kbd keys="Ctrl+Enter" /></span></button>
          <button class="btn-secondary shrink-0 py-1 text-xs" disabled={!tab.sql.trim() || !info.sql} onclick={() => run("script", editor?.selection() ?? null)} title={multi ? "Run every statement, one after another (Ctrl+Shift+Enter)" : "Run the whole text as a script (Ctrl+Shift+Enter)"} aria-label="Run as a script"><ListVideo size={12} /> <span class="hidden @[820px]:inline">Script</span></button>
        {/if}
        <button class="btn-secondary shrink-0 py-1 text-xs" disabled={tab.running || !explainSql(engine, "x")} onclick={explain} title={explainSql(engine, "x") ? "Show the plan of the statement at the cursor, without running it" : "Explain is not available for this database type"} aria-label="Explain"><ScanSearch size={12} /> <span class="hidden @[820px]:inline">Explain</span></button>
        <button class="btn-secondary shrink-0 py-1 text-xs" disabled={!canFormat(engine) || !tab.sql.trim()} onclick={format} title="Format the SQL (Ctrl+Shift+F)" aria-label="Format"><Wand2 size={12} /> <span class="hidden @[820px]:inline">Format</span></button>
        <button class="btn-secondary shrink-0 py-1 text-xs" disabled={!tab.sql.trim()} onclick={save} title="Save this query on this computer (Ctrl+S)" aria-label="Save"><Save size={12} /> <span class="hidden @[820px]:inline">Save</span>{#if dirty}<span class="h-1.5 w-1.5 rounded-full bg-warning" title="Unsaved changes"></span>{/if}</button>
        <span class="mx-1 h-4 w-px shrink-0 bg-line"></span>
        <label class="flex shrink-0 items-center gap-1.5 text-xs text-fg-muted">
          <span class="hidden @[960px]:inline">Limit</span>
          <select class="input !w-auto h-7 py-0 text-xs" bind:value={tab.limit} aria-label="Row limit">
            {#each ROW_LIMITS as n (n)}<option value={n}>{n.toLocaleString()}</option>{/each}
          </select>
        </label>
        <!-- Read-only: the connection's own setting is enforced by the server; a tab can add a guard on PostgreSQL. -->
        {#if connReadOnly}
          <span class="inline-flex shrink-0 items-center gap-1 rounded-md border border-success/30 bg-success/10 px-2 py-1 text-xs text-success" title="The connection starts every session read-only, so the server refuses changes."><Lock size={12} /> <span class="hidden @[900px]:inline">Read-only (server)</span></span>
        {:else if pgOnly}
          <button
            class="inline-flex shrink-0 items-center gap-1 rounded-md border px-2 py-1 text-xs {tab.readOnly ? 'border-success/30 bg-success/10 text-success' : 'border-line text-fg-muted hover:bg-panel-hover'}"
            aria-pressed={tab.readOnly}
            aria-label="Read-only for this tab"
            onclick={() => (tab.readOnly = !tab.readOnly)}
            title={tab.readOnly ? "Each statement here starts in a read-only transaction, so the server refuses changes. A statement that deliberately switches that off can get around it; a database account that can only read is the hard limit. Column types show as text in this mode." : "Start each statement in a read-only transaction. For a stronger guard, turn on Read-only sessions in the connection."}
          >{#if tab.readOnly}<Lock size={12} /> <span class="hidden @[900px]:inline">Read-only (this tab)</span>{:else}<LockOpen size={12} /> <span class="hidden @[900px]:inline">Can change data</span>{/if}</button>
        {:else}
          <button class="inline-flex shrink-0 items-center gap-1 rounded-md border border-line px-2 py-1 text-xs text-fg-muted hover:bg-panel-hover" onclick={ongotosettings} aria-label="Can change data" title="This connection can change data. Turn on Read-only sessions in the connection to have the server refuse changes."><LockOpen size={12} /> <span class="hidden @[900px]:inline">Can change data</span></button>
        {/if}
        <span class="ml-auto hidden shrink-0 truncate text-xs text-fg-muted @[1040px]:inline" title="{info.label} · {databases.sessions[tab.connId]?.version ?? 'not connected'}">on {databases.connName(tab.connId)}{#if !databases.sessions[tab.connId]} (not connected){/if}</span>
      </div>
    </div>
    <div class="relative" style="height: {editorH}px">
      <SqlEditor
        bind:this={editor}
        tabId={tab.id}
        value={tab.sql}
        {engine}
        label="{info.console} for {tab.title}"
        placeholder={info.consolePlaceholder}
        metadata={databases.completionProvider(tab.connId)}
        onchange={(sql) => (tab.sql = sql)}
        onrun={run}
        onsave={save}
        onformat={format}
      />
    </div>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="h-1.5 shrink-0 cursor-row-resize border-y border-line bg-panel hover:bg-accent/40 focus-visible:bg-accent/60 focus-visible:outline-none"
    role="separator"
    aria-orientation="horizontal"
    aria-label="Resize the editor"
    tabindex="0"
    title="Drag to resize the editor"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onkeydown={keyDivider}
  ></div>

  <div class="min-h-0 flex-1"><ResultsPanel {tab} {now} onopensql={openSql} onopensaved={openSaved} /></div>
</div>
