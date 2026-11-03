<script lang="ts">
  import {
    CalendarClock,
    ChevronDown,
    ChevronRight,
    CircleAlert,
    Cog,
    Columns3,
    Database,
    Eye,
    FolderTree,
    Hash,
    KeyRound,
    Layers,
    Loader2,
    PanelLeftClose,
    Plus,
    Search,
    Sigma,
    Table2,
    TriangleAlert,
    X,
    Zap,
  } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Badge from "../Badge.svelte";
  import ObjectMenu from "./ObjectMenu.svelte";
  import { menuFor, type ActionId } from "$lib/dbactions";
  import { visibleRange } from "$lib/dbdata";
  import { browseText, engineInfo, quoteName } from "$lib/dbengines";
  import { buildRows, groupKey, highlightParts, moveFocus, parentIndex, TYPE_FILTERS, type ConnectionView, type TreeRow } from "$lib/dbtree";
  import { ask } from "$lib/dialogs.svelte";
  import { databases } from "$lib/stores/databases.svelte";
  import { dbWorkspace } from "$lib/stores/dbworkspace.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type DbNodeKind, type Uuid } from "$lib/types";

  let { oncollapse, onnewquery }: { oncollapse: () => void; onnewquery: (connId: Uuid, sql?: string) => void } = $props();

  const ROW_H = 26;
  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(400);
  let focusKey = $state<string | null>(null);
  let menu = $state<{ row: TreeRow; x: number; y: number } | null>(null);

  const connections = $derived(
    vaultStore.dbConnections
      .filter((c) => c.data)
      .sort((a, b) => (a.data!.group || "￿").localeCompare(b.data!.group || "￿") || a.data!.name.localeCompare(b.data!.name)),
  );
  const views = $derived<ConnectionView[]>(
    connections.map((rec) => ({
      id: rec.id,
      name: rec.data!.name,
      group: rec.data!.group ?? "",
      engine: rec.data!.engine,
      session: databases.sessions[rec.id],
      connecting: databases.connecting[rec.id],
    })),
  );
  const rows = $derived(
    buildRows(views, { query: dbWorkspace.query, type: dbWorkspace.type, groups: settings.prefs.dbGroups, collapsed: settings.prefs.dbCollapsed, hits: dbWorkspace.hits, searching: dbWorkspace.searching }),
  );
  const range = $derived(visibleRange(scrollTop, viewport, ROW_H, rows.length));
  const shown = $derived(rows.slice(range.start, range.end));
  const searching = $derived(dbWorkspace.query.trim() !== "" || dbWorkspace.type !== "all");
  const focusIndex = $derived(Math.max(0, rows.findIndex((r) => r.key === focusKey)));

  const kindIcon = { database: Database, schema: Layers, folder: FolderTree, table: Table2, view: Eye, column: Columns3, index: KeyRound, function: Sigma, procedure: Cog, sequence: Hash, trigger: Zap, event: CalendarClock } as const;

  const selected = $derived(dbWorkspace.focus);
  function isSelected(r: TreeRow): boolean {
    const f = selected;
    if (!f) return false;
    if (f.kind === "connection") return r.kind === "connection" && r.connId === f.connId;
    if (f.kind === "node") return r.kind === "node" && r.connId === f.connId && r.path.join("\u0000") === f.path.join("\u0000") && r.node?.kind === f.node.kind;
    return false;
  }

  const isTable = (k: DbNodeKind | undefined) => k === "table" || k === "view";
  const kindOf = (r: TreeRow): DbNodeKind | undefined => r.node?.kind ?? r.hit?.kind;
  const dbOf = (r: TreeRow) => r.path[0] ?? "";

  async function connectAndShow(id: Uuid) {
    settings.prefs.dbCollapsed[id] = false;
    await databases.connect(id);
  }

  function activate(r: TreeRow) {
    focusKey = r.key;
    if (r.kind === "connection") {
      dbWorkspace.selectConnection(r.connId);
      if (databases.sessions[r.connId]) settings.prefs.dbCollapsed[r.connId] = !settings.prefs.dbCollapsed[r.connId];
      else void connectAndShow(r.connId);
    } else if (r.kind === "group" && r.group) {
      const k = groupKey(r.connId, r.path, r.group);
      settings.prefs.dbGroups[k] = !r.open;
    } else if (r.kind === "node" && r.node) {
      dbWorkspace.selectNode(r.connId, r.path, r.node);
      if (r.expandable) void databases.toggle(r.connId, r.path);
    } else if (r.kind === "hit" && r.hit) {
      void revealHit(r);
    }
  }

  async function revealHit(r: TreeRow) {
    if (!r.hit) return;
    dbWorkspace.clearSearch();
    await databases.reveal(r.connId, r.hit.parent);
    const node = databases.sessions[r.connId]?.nodes[r.hit.parent.join("\u0000")]?.find((n) => n.name === r.hit!.name && n.kind === r.hit!.kind);
    if (node) {
      dbWorkspace.selectNode(r.connId, [...r.hit.parent, node.name], node);
      focusKey = `${r.connId}\u0000${[...r.hit.parent, node.name].join("\u0000")}\u0000${node.kind}`;
      const i = rows.findIndex((x) => x.key === focusKey);
      if (i >= 0 && scroller) scroller.scrollTop = Math.max(0, i * ROW_H - viewport / 2);
    }
  }

  function open(r: TreeRow) {
    const k = kindOf(r);
    if (r.kind === "node" && isTable(k)) void databases.openTable(r.connId, dbOf(r), r.node!.name);
    else if (r.kind === "hit" && r.hit && isTable(r.hit.kind)) void databases.openTable(r.connId, r.hit.parent[0], r.hit.name);
    else activate(r);
  }

  function key(e: KeyboardEvent) {
    const r = rows[focusIndex];
    if (!r) return;
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      e.preventDefault();
      const next = moveFocus(rows, focusIndex, e.key as "ArrowDown");
      focusKey = rows[next]?.key ?? null;
      reveal(next);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      if (r.expandable && !r.open) activate(r);
      else if (r.expandable && r.open) {
        focusKey = rows[focusIndex + 1]?.key ?? focusKey;
        reveal(focusIndex + 1);
      }
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      if (r.expandable && r.open) activate(r);
      else {
        const p = parentIndex(rows, focusIndex);
        if (p >= 0) {
          focusKey = rows[p].key;
          reveal(p);
        }
      }
    } else if (e.key === "Enter") {
      e.preventDefault();
      open(r);
    } else if (e.key === "F5") {
      e.preventDefault();
      void refreshRow(r);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const el = scroller?.querySelector<HTMLElement>(`[data-key="${CSS.escape(r.key)}"]`)?.getBoundingClientRect();
      if (el) menu = { row: r, x: el.left + 24, y: el.bottom };
    } else if (e.key === "Escape" && searching) {
      e.preventDefault();
      dbWorkspace.clearSearch();
    }
  }

  function reveal(i: number) {
    if (!scroller || i < 0) return;
    const top = i * ROW_H;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW_H > scroller.scrollTop + viewport) scroller.scrollTop = top + ROW_H - viewport;
  }

  async function refreshRow(r: TreeRow) {
    if (r.kind === "connection") await databases.refresh(r.connId);
    else if (r.kind === "node" || r.kind === "group") await databases.refresh(r.connId, r.kind === "group" ? r.path : r.path);
  }

  function menuItems(r: TreeRow) {
    const engine = databases.engineOf(r.connId);
    const s = databases.sessions[r.connId];
    return menuFor({
      target: r.kind === "connection" ? "connection" : (kindOf(r) ?? "folder"),
      connected: !!s,
      sql: engineInfo(engine).sql,
      transfer: engine === "postgres" || engine === "mysql",
      changes: !!s?.changes,
      readOnly: databases.isReadOnlyConnection(r.connId),
    });
  }

  const qualified = (r: TreeRow) => {
    const engine = databases.engineOf(r.connId);
    const parts = r.kind === "hit" && r.hit ? [...r.hit.parent, r.hit.name] : r.path;
    return quoteName(engine, ...parts);
  };

  async function pick(id: ActionId, r: TreeRow) {
    const name = r.kind === "hit" ? r.hit!.name : r.label;
    const db = r.kind === "hit" ? r.hit!.parent[0] : dbOf(r);
    switch (id) {
      case "connect":
        await connectAndShow(r.connId);
        break;
      case "disconnect":
        await databases.disconnect(r.connId);
        break;
      case "new-query":
        onnewquery(r.connId);
        break;
      case "refresh":
        await refreshRow(r);
        break;
      case "edit":
        ui.modal = { kind: "db-connection", id: r.connId };
        break;
      case "delete":
        await removeConnection(r.connId);
        break;
      case "copy-name":
        await copy(name);
        break;
      case "copy-qualified":
        await copy(qualified(r));
        break;
      case "open-data":
        await databases.openTable(r.connId, db, name);
        break;
      case "open-structure":
        await databases.openTable(r.connId, db, name, "structure");
        break;
      case "open-ddl":
        await databases.openTable(r.connId, db, name, "ddl");
        break;
      case "select-query":
        onnewquery(r.connId, browseText(databases.engineOf(r.connId), db, name));
        break;
      case "export":
        ui.modal = { kind: "db-export", connId: r.connId, database: db, table: name };
        break;
      case "import":
        ui.modal = { kind: "db-import", connId: r.connId, database: db, table: name };
        break;
    }
  }

  async function copy(text: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied ${text}`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function removeConnection(id: Uuid) {
    const c = vaultStore.dbConnections.find((x) => x.id === id);
    if (!(await ask(`Delete the connection "${c?.data?.name}"? Its saved password goes with it, on every device.`))) return;
    await databases.disconnect(id);
    for (const t of databases.tabs.filter((t) => t.connId === id)) await databases.closeTab(t.id, true);
    try {
      await vaultStore.deleteDbConnection(id);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function context(e: MouseEvent, r: TreeRow) {
    e.preventDefault();
    focusKey = r.key;
    if (r.kind === "connection") dbWorkspace.selectConnection(r.connId);
    else if (r.kind === "node" && r.node) dbWorkspace.selectNode(r.connId, r.path, r.node);
    if (r.kind === "connection" || r.kind === "node" || r.kind === "hit") menu = { row: r, x: e.clientX, y: e.clientY };
  }

  const pad = (r: TreeRow) => `padding-left: ${r.depth * 14 + 6}px`;
  const countLabel = $derived(`${connections.length} connection${connections.length === 1 ? "" : "s"}`);
</script>

<div class="flex h-full min-h-0 flex-col bg-panel">
  <header class="space-y-2.5 border-b border-line px-3 pb-2.5 pt-3">
    <div class="flex items-start gap-2">
      <div class="min-w-0 flex-1">
        <h1 class="flex items-center gap-2 text-sm font-semibold"><Database size={16} class="text-accent" /> Databases</h1>
        <p class="mt-0.5 text-[11px] leading-4 text-fg-muted">Manage and explore your databases.</p>
      </div>
      <button class="icon-btn h-7 w-7 shrink-0" title="Hide the explorer" aria-label="Hide the explorer" onclick={oncollapse}><PanelLeftClose size={15} /></button>
    </div>
    <button class="btn-primary w-full justify-center py-1.5 text-xs" data-testid="db-new-connection" onclick={() => (ui.modal = { kind: "db-connection", id: null })}><Plus size={13} /> Connection</button>
    <div class="relative">
      <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input
        class="input h-8 w-full pl-8 pr-7 text-xs"
        type="search"
        placeholder="Search connections, schemas, tables…"
        aria-label="Search database objects"
        value={dbWorkspace.query}
        oninput={(e) => dbWorkspace.setQuery(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            scroller?.focus();
          }
        }}
      />
      {#if dbWorkspace.query}
        <button class="icon-btn absolute right-1 top-1/2 h-6 w-6 -translate-y-1/2" title="Clear the search" aria-label="Clear the search" onclick={() => dbWorkspace.clearSearch()}><X size={12} /></button>
      {/if}
    </div>
    <div class="flex flex-wrap gap-1" role="group" aria-label="Filter by object type">
      {#each TYPE_FILTERS as f (f.id)}
        <button
          class="rounded-md border px-2 py-0.5 text-[11px] transition-colors {dbWorkspace.type === f.id ? 'border-accent/50 bg-accent/15 text-accent' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          aria-pressed={dbWorkspace.type === f.id}
          onclick={() => (dbWorkspace.type = f.id)}
        >{f.label}</button>
      {/each}
    </div>
  </header>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={scroller}
    bind:clientHeight={viewport}
    class="relative min-h-0 flex-1 overflow-y-auto outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent/50"
    role="tree"
    aria-label="Database objects"
    tabindex="0"
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    onkeydown={key}
    onfocus={() => !focusKey && (focusKey = rows[0]?.key ?? null)}
  >
    {#if connections.length === 0}
      <div class="px-4 py-8 text-center text-xs text-fg-muted">
        <p>No database connections yet.</p>
        <button class="btn-secondary mt-3 text-xs" onclick={() => (ui.modal = { kind: "db-connection", id: null })}><Plus size={13} /> New connection</button>
      </div>
    {:else if rows.length === 0}
      <p class="px-4 py-6 text-center text-xs text-fg-muted">Nothing matches "{dbWorkspace.query}".{#if connections.some((c) => !databases.sessions[c.id])} Connect to search a server's objects.{/if}</p>
    {:else}
      <div style="height: {rows.length * ROW_H}px; position: relative">
        <div style="transform: translateY({range.start * ROW_H}px)">
          {#each shown as r (r.key)}
            {@const focused = r.key === focusKey}
            {#if r.kind === "section"}
              <div class="flex items-end px-3 pb-0.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted" style="height: {ROW_H}px" data-key={r.key}>{r.label}</div>
            {:else if r.kind === "connection"}
              {@const rec = connections.find((c) => c.id === r.connId)}
              {@const c = rec?.data}
              {@const s = databases.sessions[r.connId]}
              {@const env = envInfo(c?.environment)}
              <div
                class="group flex items-center gap-1 pr-1.5 text-sm {isSelected(r) ? 'bg-accent/15' : 'hover:bg-panel-hover'} {focused ? 'outline outline-1 -outline-offset-1 outline-accent/50' : ''}"
                style="height: {ROW_H}px; {pad(r)}"
                role="treeitem"
                aria-level={1}
                aria-selected={isSelected(r)}
                aria-expanded={s ? r.open : undefined}
                data-key={r.key}
                data-testid="db-conn-row"
                tabindex="-1"
                oncontextmenu={(e) => context(e, r)}
              >
                <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" onclick={() => activate(r)} title={s ? `${r.label} · connected · ${s.version}` : `${r.label} · click to connect`}>
                  {#if r.connecting}
                    <Loader2 size={13} class="shrink-0 animate-spin text-fg-muted" />
                  {:else if s}
                    {#if r.open}<ChevronDown size={13} class="shrink-0" />{:else}<ChevronRight size={13} class="shrink-0" />{/if}
                  {:else}
                    <span class="mx-[3px] h-2 w-2 shrink-0 rounded-full bg-fg-muted/40"></span>
                  {/if}
                  <span class="h-1.5 w-1.5 shrink-0 rounded-full {s ? 'bg-success' : 'bg-transparent'}" aria-hidden="true"></span>
                  <span class="truncate font-medium {s ? '' : 'text-fg-muted'}">{#each highlightParts(r.label, dbWorkspace.query) as p}{#if p.match}<mark class="rounded-sm bg-accent/30 text-fg">{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
                  {#if env.short}<span class="shrink-0"><Badge tone={env.tone ?? "neutral"}>{env.short}</Badge></span>{/if}
                  {#if c?.tls === "disable" && !c?.ssh_host_id}<span class="shrink-0 text-warning" title="Not encrypted: the password and rows cross the network in clear text"><TriangleAlert size={12} aria-label="Not encrypted" /></span>{/if}
                </button>
                <span class="shrink-0 text-[10px] text-fg-muted/70">{engineInfo(r.engine ?? "").label.split(" ")[0]}</span>
                {#if s}<button class="icon-btn h-6 w-6 opacity-0 focus:opacity-100 group-hover:opacity-100" title="New query" aria-label="New query on {r.label}" onclick={() => onnewquery(r.connId)}><Plus size={13} /></button>{/if}
              </div>
            {:else if r.kind === "group"}
              <div class="flex items-center {focused ? 'outline outline-1 -outline-offset-1 outline-accent/50' : ''}" style="height: {ROW_H}px; {pad(r)}" role="treeitem" aria-level={r.depth + 1} aria-expanded={r.open} aria-selected="false" data-key={r.key} tabindex="-1">
                <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left text-[11px] font-medium uppercase tracking-wide text-fg-muted hover:text-fg" onclick={() => activate(r)}>
                  {#if r.open}<ChevronDown size={12} class="shrink-0" />{:else}<ChevronRight size={12} class="shrink-0" />{/if}
                  {r.label}<span class="font-normal normal-case tracking-normal text-fg-muted/70">({r.count})</span>
                </button>
              </div>
            {:else if r.kind === "node" && r.node}
              {@const Icon = kindIcon[r.node.kind]}
              {@const tableish = isTable(r.node.kind)}
              <div
                class="group/node flex items-center gap-1 pr-1.5 {isSelected(r) ? 'bg-accent/15' : 'hover:bg-panel-hover'} {focused ? 'outline outline-1 -outline-offset-1 outline-accent/50' : ''}"
                style="height: {ROW_H}px; {pad(r)}"
                role="treeitem"
                aria-level={r.depth + 1}
                aria-selected={isSelected(r)}
                aria-expanded={r.expandable ? r.open : undefined}
                data-key={r.key}
                tabindex="-1"
                oncontextmenu={(e) => context(e, r)}
              >
                <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" onclick={() => activate(r)} ondblclick={() => tableish && open(r)} title="{r.node.name}{r.node.detail ? ` · ${r.node.detail}${r.node.detail.startsWith('~') ? ' (the server\'s estimate)' : ''}` : ''}{tableish ? ' · double-click to open its rows' : ''}">
                  {#if r.expandable}
                    {#if r.open}<ChevronDown size={12} class="shrink-0" />{:else}<ChevronRight size={12} class="shrink-0" />{/if}
                  {:else}
                    <span class="w-3 shrink-0"></span>
                  {/if}
                  <Icon size={13} class="shrink-0 {r.node.kind === 'database' || r.node.kind === 'schema' ? 'text-accent' : 'text-fg-muted'}" />
                  <span class="truncate text-xs {r.node.detail === 'system' ? 'text-fg-muted' : ''}">{#each highlightParts(r.node.name, dbWorkspace.query) as p}{#if p.match}<mark class="rounded-sm bg-accent/30 text-fg">{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
                  {#if r.node.detail}<span class="ml-1 min-w-0 shrink truncate text-[11px] text-fg-muted/70">{r.node.detail.startsWith("~") ? `≈ ${r.node.detail.slice(1)}` : r.node.detail}</span>{/if}
                </button>
                {#if tableish}
                  <button class="icon-btn h-5 w-5 opacity-0 focus:opacity-100 group-hover/node:opacity-100" title="Open data" aria-label="Open data of {r.node.name}" onclick={() => open(r)}><Table2 size={12} /></button>
                {/if}
              </div>
            {:else if r.kind === "hit" && r.hit}
              {@const Icon = kindIcon[r.hit.kind]}
              <div class="group/node flex items-center gap-1 pr-1.5 hover:bg-panel-hover {focused ? 'outline outline-1 -outline-offset-1 outline-accent/50' : ''}" style="height: {ROW_H}px; {pad(r)}" role="treeitem" aria-level={2} aria-selected="false" data-key={r.key} tabindex="-1" oncontextmenu={(e) => context(e, r)}>
                <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" onclick={() => activate(r)} ondblclick={() => open(r)} title="{r.hit.parent.join('.')}.{r.hit.name} · click to find it in the tree">
                  <Icon size={13} class="shrink-0 text-fg-muted" />
                  <span class="truncate text-xs">{#each highlightParts(r.hit.name, dbWorkspace.query) as p}{#if p.match}<mark class="rounded-sm bg-accent/30 text-fg">{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
                  <span class="ml-1 min-w-0 shrink truncate text-[11px] text-fg-muted/70">{r.hit.parent.join(".")}</span>
                </button>
              </div>
            {:else if r.kind === "hits-header"}
              <div class="flex items-center gap-1.5 px-3 pt-1 text-[11px] font-medium text-fg-muted" style="height: {ROW_H}px" data-key={r.key}>
                {#if r.label.startsWith("Searching")}<Loader2 size={11} class="animate-spin" />{/if}{r.label}
              </div>
            {:else if r.kind === "loading"}
              <p class="flex items-center gap-1.5 text-xs text-fg-muted" style="height: {ROW_H}px; {pad(r)}"><Loader2 size={11} class="animate-spin" /> Loading…</p>
            {:else if r.kind === "error"}
              <p class="flex items-center gap-1.5 truncate text-xs text-danger" style="height: {ROW_H}px; {pad(r)}" role="alert" title={r.label}><CircleAlert size={12} class="shrink-0" /> <span class="truncate">{r.label}</span></p>
            {:else if r.kind === "empty"}
              <p class="flex items-center text-xs text-fg-muted" style="height: {ROW_H}px; {pad(r)}">{r.label}</p>
            {/if}
          {/each}
        </div>
      </div>
    {/if}
  </div>
  <footer class="flex items-center justify-between border-t border-line px-3 py-1.5 text-[11px] text-fg-muted">
    <span>{countLabel}</span>
    <span>{Object.keys(databases.sessions).length} connected</span>
  </footer>
</div>

{#if menu}
  {@const r = menu.row}
  <ObjectMenu items={menuItems(r)} x={menu.x} y={menu.y} label={r.kind === "hit" ? r.hit!.name : r.label} onpick={(id) => void pick(id, r)} onclose={() => (menu = null)} />
{/if}
