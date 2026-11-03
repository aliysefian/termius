<script lang="ts">
  import { PanelRightClose, Plug, Plus, Unplug } from "lucide-svelte";
  import Badge from "../Badge.svelte";
  import { engineInfo } from "$lib/dbengines";
  import { connectionFacts, tableFacts, type Fact } from "$lib/dbinspect";
  import { pathKey } from "$lib/dbtree";
  import { databases } from "$lib/stores/databases.svelte";
  import { dbWorkspace, type InspectorTab } from "$lib/stores/dbworkspace.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, type DbTableInfo, type DbTableMeta, type Uuid } from "$lib/types";

  let { now, oncollapse }: { now: number; oncollapse: () => void } = $props();

  const TABS: { id: InspectorTab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "structure", label: "Structure" },
    { id: "activity", label: "Activity" },
    { id: "properties", label: "Properties" },
  ];

  // What the inspector is about: the last thing clicked in the explorer, or the open document.
  const focus = $derived(dbWorkspace.focus ?? (databases.active ? ({ kind: "doc", tabId: databases.active.id } as const) : null));
  const doc = $derived(focus?.kind === "doc" ? databases.tab(focus.tabId) : undefined);
  const connId = $derived<Uuid | null>(focus?.kind === "doc" ? (doc?.connId ?? null) : (focus?.connId ?? null));
  const rec = $derived(connId ? vaultStore.dbConnections.find((c) => c.id === connId) : undefined);
  const conn = $derived(rec?.data);
  const session = $derived(connId ? databases.sessions[connId] : undefined);

  /** The table the inspector is about, when it is one. */
  const table = $derived.by<{ schema: string; name: string } | null>(() => {
    if (focus?.kind === "node" && (focus.node.kind === "table" || focus.node.kind === "view") && focus.path.length >= 2) return { schema: focus.path[0], name: focus.path[1] };
    if (doc?.table) return { schema: doc.table.database, name: doc.table.table };
    return null;
  });

  let tableInfo = $state<{ key: string; info: DbTableInfo | null } | null>(null);
  $effect(() => {
    const t = table;
    const c = connId;
    if (!t || !c || !session) {
      tableInfo = null;
      return;
    }
    const key = `${c}\u0000${t.schema}\u0000${t.name}`;
    if (doc?.table && doc.info) {
      tableInfo = { key, info: doc.info };
      return;
    }
    void databases.completionTableInfo(c, t.schema, t.name).then((info) => {
      tableInfo = { key, info };
    });
  });

  const meta = $derived.by<DbTableMeta | null>(() => {
    if (!table || !connId) return null;
    if (doc?.meta) return doc.meta;
    const c = databases.cachedMeta(connId, table.schema, table.name);
    return c && c !== "loading" && !("error" in c) ? c : null;
  });
  const info = $derived(table && tableInfo?.key === `${connId}\u0000${table.schema}\u0000${table.name}` ? tableInfo.info : null);

  const facts = $derived.by<Fact[]>(() => {
    if (table) return tableFacts(table.schema, table.name, meta, info);
    if (conn) {
      const ssh = conn.ssh_host_id ? (vaultStore.hostById.get(conn.ssh_host_id)?.data?.label ?? "a saved host") : null;
      return connectionFacts({
        engineLabel: engineInfo(conn.engine).label,
        host: conn.host,
        port: conn.port,
        user: conn.username,
        database: conn.database,
        tls: conn.tls,
        sshHost: ssh,
        environment: conn.environment ? envInfo(conn.environment).label : null,
        readOnly: conn.options?.read_only === "true",
        version: session?.version ?? null,
        connectedAt: session?.connectedAt ?? null,
        now,
      });
    }
    return [];
  });

  const nodeKind = $derived(focus?.kind === "node" ? focus.node.kind : null);
  const children = $derived(focus?.kind === "node" ? (session?.nodes[pathKey(focus.path)] ?? null) : connId && session ? (session.nodes[""] ?? null) : null);
  const counts = $derived.by(() => {
    const m = new Map<string, number>();
    for (const n of children ?? []) m.set(n.kind, (m.get(n.kind) ?? 0) + 1);
    return [...m.entries()];
  });
  const recent = $derived(
    databases.history
      .filter((h) => (conn ? h.connection === conn.name : true) && (table ? h.sql.toLowerCase().includes(table.name.toLowerCase()) : true))
      .slice(0, 8),
  );
  const title = $derived(table ? `${table.schema}.${table.name}` : focus?.kind === "node" ? focus.node.name : (conn?.name ?? "Nothing selected"));
  const subtitle = $derived(table ? (meta?.kind ?? "table") : focus?.kind === "node" ? focus.node.kind : doc ? (doc.table ? "table document" : "query document") : conn ? "connection" : "");
  const env = $derived(envInfo(conn?.environment));

  function connect() {
    if (connId) void databases.connect(connId);
  }
</script>

<aside class="flex h-full min-h-0 flex-col bg-panel" aria-label="Inspector">
  <header class="flex items-start gap-2 border-b border-line px-3 py-2.5">
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-sm font-semibold" title={title}>{title}</h2>
      <p class="flex items-center gap-1.5 text-[11px] text-fg-muted">{subtitle}{#if conn && env.short}<Badge tone={env.tone ?? "neutral"}>{env.short}</Badge>{/if}</p>
    </div>
    <button class="icon-btn h-7 w-7 shrink-0" title="Hide the inspector" aria-label="Hide the inspector" onclick={oncollapse}><PanelRightClose size={15} /></button>
  </header>

  <div class="flex border-b border-line px-1" role="tablist" aria-label="Inspector sections">
    {#each TABS as t (t.id)}
      <button role="tab" aria-selected={dbWorkspace.inspectorTab === t.id} class="-mb-px border-b-2 px-2.5 py-1.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent {dbWorkspace.inspectorTab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}" onclick={() => (dbWorkspace.inspectorTab = t.id)}>{t.label}</button>
    {/each}
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto p-3 text-xs">
    {#if !focus || (!conn && !doc)}
      <p class="py-6 text-center text-fg-muted">Select a connection, a table or a document to see its details here.</p>
    {:else if dbWorkspace.inspectorTab === "overview"}
      {#if doc && !doc.table}
        <!-- A query: what the last run did -->
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          <dt class="text-fg-muted">Status</dt>
          <dd>{doc.running ? "Running…" : doc.error ? "Failed" : doc.result ? "Completed" : "Not run yet"}</dd>
          <dt class="text-fg-muted">Connection</dt><dd class="truncate">{conn?.name}{#if !session} <span class="text-fg-muted">(not connected)</span>{/if}</dd>
          {#if doc.result}
            <dt class="text-fg-muted">Duration</dt><dd class="font-mono">{doc.result.elapsed_ms} ms</dd>
            <dt class="text-fg-muted">{doc.result.columns.length ? "Rows returned" : "Rows affected"}</dt>
            <dd class="font-mono">{(doc.result.columns.length ? doc.result.rows.length : (doc.result.affected_rows ?? 0)).toLocaleString()}{doc.result.truncated ? " (stopped at the limit)" : ""}</dd>
            {#if doc.result.columns.length}<dt class="text-fg-muted">Columns</dt><dd class="font-mono">{doc.result.columns.length}</dd>{/if}
            {#if doc.sets.length > 1}<dt class="text-fg-muted">Result sets</dt><dd class="font-mono">{doc.sets.length}</dd>{/if}
          {/if}
          {#if doc.error}<dt class="text-fg-muted">Error</dt><dd class="whitespace-pre-wrap break-words font-mono text-danger">{doc.error}</dd>{/if}
        </dl>
        <div class="mt-3 flex flex-wrap gap-1.5">
          <button class="btn-secondary h-7 px-2 text-xs" disabled={doc.running || !doc.sql.trim()} onclick={() => void databases.explain(doc.id, doc.sql)}>Explain the plan</button>
        </div>
      {:else}
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          {#each facts as f (f.label)}
            <dt class="text-fg-muted">{f.label}</dt>
            <dd class="break-words {f.mono ? 'font-mono' : ''} {f.warn ? 'text-warning' : ''}">{f.value}</dd>
          {/each}
        </dl>
        {#if focus.kind !== "node" || nodeKind === null}
          <div class="mt-3 flex flex-wrap gap-1.5">
            {#if session}
              <button class="btn-secondary h-7 px-2 text-xs" onclick={() => connId && databases.newTab(connId)}><Plus size={12} /> New query</button>
              <button class="btn-secondary h-7 px-2 text-xs" onclick={() => connId && void databases.disconnect(connId)}><Unplug size={12} /> Disconnect</button>
            {:else}
              <button class="btn-primary h-7 px-2 text-xs" onclick={connect}><Plug size={12} /> Connect</button>
            {/if}
            <button class="btn-secondary h-7 px-2 text-xs" onclick={() => (ui.modal = { kind: "db-connection", id: connId })}>Edit</button>
          </div>
        {/if}
      {/if}
    {:else if dbWorkspace.inspectorTab === "structure"}
      {#if table}
        {#if !info}
          <p class="text-fg-muted">{session ? "Reading the columns…" : "Connect to see the columns."}</p>
        {:else}
          <ul class="divide-y divide-line/60" aria-label="Columns">
            {#each info.columns as c (c.name)}
              <li class="flex items-baseline gap-2 py-1">
                <span class="min-w-0 flex-1 truncate font-mono">{c.name}{#if c.primary_key} <span class="text-warning">PK</span>{/if}</span>
                <span class="shrink-0 font-mono text-[11px] text-sky-400">{c.data_type}{c.nullable ? "" : " not null"}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {:else if doc && !doc.table}
        {#if doc.result?.columns.length}
          <ul class="divide-y divide-line/60" aria-label="Result columns">
            {#each doc.result.columns as c, i (i)}
              <li class="flex items-baseline gap-2 py-1"><span class="min-w-0 flex-1 truncate font-mono">{c.name}</span><span class="shrink-0 font-mono text-[11px] text-sky-400">{c.data_type}</span></li>
            {/each}
          </ul>
        {:else}<p class="text-fg-muted">Run a query that returns rows to see its columns.</p>{/if}
      {:else if !session}
        <p class="text-fg-muted">Connect to see what is inside.</p>
      {:else if !children}
        <p class="text-fg-muted">Not loaded yet. Open it in the explorer.</p>
      {:else}
        <ul class="divide-y divide-line/60" aria-label="Contents">
          {#each counts as [kind, n] (kind)}<li class="flex justify-between py-1"><span class="capitalize">{kind}s</span><span class="font-mono text-fg-muted">{n}</span></li>{/each}
        </ul>
        <p class="mt-2 text-[11px] text-fg-muted">Counts of what has been loaded at this level.</p>
      {/if}
    {:else if dbWorkspace.inspectorTab === "activity"}
      {#if doc}
        <h3 class="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">This document</h3>
        {#if doc.messages.length === 0}<p class="text-fg-muted">Nothing has run here yet.</p>{/if}
        <ul class="space-y-1.5">
          {#each doc.messages.slice(0, 6) as m, i (m.at + ":" + i)}
            <li class="flex gap-2"><span class="shrink-0 text-fg-muted">{new Date(m.at).toLocaleTimeString()}</span><span class="min-w-0 break-words {m.kind === 'error' ? 'text-danger' : ''}">{m.text}</span></li>
          {/each}
        </ul>
      {/if}
      <h3 class="mb-1.5 mt-3 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Recent runs{table ? ` mentioning ${table.name}` : conn ? ` on ${conn.name}` : ""}</h3>
      {#if recent.length === 0}<p class="text-fg-muted">None yet.</p>{/if}
      <ul class="space-y-1.5">
        {#each recent as h (h.at + h.sql)}
          <li>
            <p class="truncate font-mono" title={h.sql}>{h.sql.replace(/\s+/g, " ")}</p>
            <p class="text-[11px] text-fg-muted">{new Date(h.at).toLocaleTimeString()}{#if h.ms !== undefined} · {h.ms} ms{/if}{#if h.ok === false} · <span class="text-danger">failed</span>{/if}</p>
          </li>
        {/each}
      </ul>
      {#if session && !table}
        <p class="mt-3 text-[11px] text-fg-muted">{session.runs} run{session.runs === 1 ? "" : "s"} on this connection since it opened. The server's own activity (sessions, load) isn't read.</p>
      {/if}
    {:else}
      <!-- Properties: how it is set up -->
      {#if doc}
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          <dt class="text-fg-muted">Document</dt><dd class="break-words">{doc.title}</dd>
          <dt class="text-fg-muted">Row limit</dt><dd class="font-mono">{doc.table && databases.isPaged(doc) ? `${doc.page.size} per page` : doc.limit.toLocaleString()}</dd>
          <dt class="text-fg-muted">Read-only</dt><dd>{databases.isReadOnlyConnection(doc.connId) ? "yes, by the server" : doc.readOnly ? "yes, in this tab" : "no"}</dd>
          {#if doc.savedId}<dt class="text-fg-muted">Saved as</dt><dd class="break-words">{databases.saved.find((q) => q.id === doc.savedId)?.name ?? "a deleted query"}</dd>{/if}
          {#if doc.table && databases.isPaged(doc)}
            <dt class="text-fg-muted">Order</dt><dd class="font-mono break-words">{doc.page.order.map((o) => o.column + (o.desc ? " DESC" : "")).join(", ") || "the server's"}</dd>
            {#if doc.page.filter.trim()}<dt class="text-fg-muted">Filter</dt><dd class="font-mono break-words">{doc.page.filter}</dd>{/if}
          {/if}
        </dl>
      {:else if table && meta?.supported}
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          <dt class="text-fg-muted">Constraints</dt><dd>{meta.constraints.length}</dd>
          <dt class="text-fg-muted">Foreign keys</dt><dd>{meta.constraints.filter((c) => c.kind === "foreign_key").length}</dd>
          <dt class="text-fg-muted">Referenced by</dt><dd>{meta.referenced_by.length} table{meta.referenced_by.length === 1 ? "" : "s"}</dd>
          {#each meta.column_notes as [col, note] (col)}<dt class="text-fg-muted">{col}</dt><dd>{note}</dd>{/each}
        </dl>
      {:else if focus.kind === "node"}
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          <dt class="text-fg-muted">Kind</dt><dd>{focus.node.kind}</dd>
          <dt class="text-fg-muted">Path</dt><dd class="break-all font-mono">{focus.path.join(" › ")}</dd>
          {#if focus.node.detail}<dt class="text-fg-muted">Detail</dt><dd class="break-words font-mono">{focus.node.detail}</dd>{/if}
        </dl>
      {:else if conn}
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5">
          <dt class="text-fg-muted">Group</dt><dd>{conn.group || "none"}</dd>
          <dt class="text-fg-muted">Password</dt><dd>{rec?.data?.password === undefined ? "none saved" : "saved in the vault (never shown)"}</dd>
          <dt class="text-fg-muted">Connect timeout</dt><dd class="font-mono">15 s</dd>
          {#each Object.entries(conn.options ?? {}).filter(([k]) => !/pass|secret|token|key/i.test(k)) as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="break-words font-mono">{v}</dd>{/each}
          {#if conn.notes}<dt class="text-fg-muted">Notes</dt><dd class="whitespace-pre-wrap break-words">{conn.notes}</dd>{/if}
        </dl>
      {/if}
    {/if}
  </div>
</aside>
