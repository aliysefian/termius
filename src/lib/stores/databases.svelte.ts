// The Databases view's live state: open sessions, their browse trees, and the documents (SQL tabs and table tabs).
// Saved connections live in the vault store; everything here is per window and goes away when the vault locks.
import * as api from "$lib/api";
import { ask, askText } from "$lib/dialogs.svelte";
import { buildRowEdit, cellAfterEdit, editBlock, type SortDir } from "$lib/dbdata";
import { browseText, engineInfo } from "$lib/dbengines";
import { addEntry, finishEntry, loadHistory, saveHistory, type HistoryEntry } from "$lib/dbhistory";
import { countSql, explainSql, filterProblem, isPageable, pageSql, type OrderBy } from "$lib/dbpaging";
import { isScript, splitStatements } from "$lib/dbstatement";
import { addInsert, emptyStaging, hasPending, pendingSummary, removeInsert, rowBlock, setInsertValue, stageEdit, toChanges, toggleDelete, type Staging } from "$lib/dbstaging";
import { load as loadSaved, newSaved, remove as removeSaved, save as saveSaved, upsert as upsertSaved, type SavedQuery } from "$lib/savedqueries";
import { pathKey } from "$lib/dbtree";
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type DbQueryResult, type DbTableInfo, type DbTableMeta, type DbTreeNode, type Uuid } from "$lib/types";
import type { SqlMetadataProvider } from "$lib/sqlcompletion";

export { pathKey };
export const ROW_LIMITS = [100, 1000, 10_000, 100_000] as const;
export const PAGE_SIZES = [50, 100, 200, 500, 1000] as const;
export const DEFAULT_LIMIT = 1000;

export type ResultTab = "results" | "messages" | "history" | "saved";
export type TableView = "data" | "structure" | "indexes" | "constraints" | "relationships" | "ddl";

export interface Message {
  at: number;
  kind: "info" | "success" | "error" | "warning";
  text: string;
  sql?: string;
  ms?: number;
}

/** One result of a run. A script that has several statements that return rows has several. */
export interface ResultSet {
  title: string;
  sql: string;
  result: DbQueryResult;
}

export interface PageState {
  size: number;
  offset: number;
  filter: string;
  filterError: string | null;
  order: OrderBy[];
  /** Another page exists after this one (one more row than the page size came back). */
  hasNext: boolean;
  /** An exact count, only when the person asked for it. */
  count: number | null;
  countLoading: boolean;
}

export interface QueryTab {
  id: string;
  connId: Uuid;
  title: string;
  sql: string;
  result: DbQueryResult | null;
  error: string | null;
  running: boolean;
  queryId: string | null;
  startedAt: number;
  limit: number;
  /** Set when the tab browses one table, which is what makes its rows editable. */
  table: { database: string; table: string } | null;
  info: DbTableInfo | null;
  sort: { col: number | null; dir: SortDir };

  /** The saved query this tab was opened from or saved as, and the text it had then (for the unsaved dot). */
  savedId: string | null;
  baseline: string;
  messages: Message[];
  resultTab: ResultTab;
  /** More than one result: all of them, and which is shown. `result` is always the one shown. */
  sets: ResultSet[];
  activeSet: number;
  /** Start each statement in a read-only transaction (PostgreSQL). */
  readOnly: boolean;

  // Table documents
  view: TableView;
  meta: DbTableMeta | null;
  metaLoading: boolean;
  metaError: string | null;
  page: PageState;
  staging: Staging;
  /** The review step before saving staged changes. */
  review: { sql: string[]; busy: boolean; error: string | null } | null;
  /** The text of the cell being looked at in full. */
  viewer: { column: string; type: string; text: string } | null;
}

export interface ConnState {
  sessionId: Uuid;
  version: string;
  connectedAt: number;
  /** Whether reviewed change sets (insert, delete, several edits) work on this connection. */
  changes: boolean;
  /** Queries run on it since it opened. */
  runs: number;
  /** Children by path; the root is the empty path. */
  nodes: Record<string, DbTreeNode[]>;
  open: Record<string, boolean>;
  loading: Record<string, boolean>;
  failed: Record<string, string>;
}

const MESSAGES_KEEP = 200;

class DatabasesStore {
  sessions = $state<Record<Uuid, ConnState>>({});
  connecting = $state<Record<Uuid, boolean>>({});
  tabs = $state<QueryTab[]>([]);
  activeTabId = $state<string | null>(null);
  history = $state<HistoryEntry[]>(loadHistory());
  saved = $state<SavedQuery[]>(loadSaved());
  /** Table details read for the inspector and the table view, by connection and table. */
  metaCache = $state<Record<string, DbTableMeta | "loading" | { error: string }>>({});
  #completionChildren = new Map<string, Promise<DbTreeNode[]>>();
  #completionTables = new Map<string, Promise<DbTableInfo | null>>();
  #completionProviders = new Map<Uuid, SqlMetadataProvider>();

  active = $derived(this.tabs.find((t) => t.id === this.activeTabId) ?? null);

  constructor() {
    // The backend closes every session when the vault locks; forget ours too.
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  reset() {
    if (Object.keys(this.sessions).length || this.tabs.length) {
      this.sessions = {};
      this.connecting = {};
      this.tabs = [];
      this.activeTabId = null;
      this.metaCache = {};
    }
    this.#completionChildren.clear();
    this.#completionTables.clear();
    this.#completionProviders.clear();
  }

  connName(connId: Uuid): string {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.name ?? "database";
  }

  engineOf(connId: Uuid): string {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.engine ?? "mysql";
  }

  isProduction(connId: Uuid): boolean {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.environment === "production";
  }

  /** Whether the connection's sessions start read-only (the server enforces it). */
  isReadOnlyConnection(connId: Uuid): boolean {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.options?.read_only === "true";
  }

  defaultNamespace(connId: Uuid): string | undefined {
    const connection = vaultStore.dbConnections.find((c) => c.id === connId)?.data;
    if (connection?.engine === "postgres") return "public";
    return connection?.engine === "mysql" ? connection.database || undefined : undefined;
  }

  completionProvider(connId: Uuid): SqlMetadataProvider {
    let provider = this.#completionProviders.get(connId);
    if (!provider) {
      provider = {
        children: (path) => this.completionChildren(connId, path),
        tableInfo: (namespace, table) => this.completionTableInfo(connId, namespace, table),
        defaultNamespace: this.defaultNamespace(connId),
      };
      this.#completionProviders.set(connId, provider);
    }
    return provider;
  }

  // -- connections -------------------------------------------------------

  async connect(connId: Uuid): Promise<ConnState | null> {
    if (this.sessions[connId]) return this.sessions[connId];
    if (this.connecting[connId]) return null;
    this.connecting[connId] = true;
    try {
      const opened = await api.db.open(connId);
      const changes = await api.db.supportsChanges(opened.session_id).catch(() => false);
      this.sessions[connId] = { sessionId: opened.session_id, version: opened.server_version, connectedAt: Date.now(), changes, runs: 0, nodes: {}, open: {}, loading: {}, failed: {} };
      await this.loadChildren(connId, []);
      return this.sessions[connId];
    } catch (e) {
      ui.notify("error", `${this.connName(connId)}: ${errorMessage(e)}`);
      return null;
    } finally {
      this.connecting[connId] = false;
    }
  }

  async disconnect(connId: Uuid) {
    const s = this.sessions[connId];
    delete this.sessions[connId];
    this.#clearCompletion(connId);
    for (const t of this.tabs.filter((t) => t.connId === connId)) {
      if (t.queryId) t.error = "The connection was closed.";
      t.running = false;
    }
    for (const k of Object.keys(this.metaCache)) if (k.startsWith(`${connId}\u0000`)) delete this.metaCache[k];
    if (s) await api.db.close(s.sessionId).catch(() => {});
  }

  /** The session went away under us (server closed it, tunnel dropped). */
  #lost(connId: Uuid) {
    delete this.sessions[connId];
    this.#clearCompletion(connId);
  }

  // -- tree --------------------------------------------------------------

  async loadChildren(connId: Uuid, path: string[], force = false) {
    const s = this.sessions[connId];
    const key = pathKey(path);
    if (!s || s.loading[key] || (!force && s.nodes[key])) return;
    s.loading[key] = true;
    delete s.failed[key];
    try {
      s.nodes[key] = await api.db.children(s.sessionId, path);
    } catch (e) {
      s.failed[key] = errorMessage(e);
      if (isApiError(e) && e.code === "no_session") this.#lost(connId);
    } finally {
      s.loading[key] = false;
    }
  }

  async toggle(connId: Uuid, path: string[]) {
    const s = this.sessions[connId];
    if (!s) return;
    const key = pathKey(path);
    s.open[key] = !s.open[key];
    if (s.open[key]) await this.loadChildren(connId, path);
  }

  /** Open every level down to `path` (loading each), for revealing a search hit. */
  async reveal(connId: Uuid, path: string[]) {
    const s = this.sessions[connId];
    if (!s) return;
    for (let i = 1; i <= path.length; i++) {
      const prefix = path.slice(0, i);
      s.open[pathKey(prefix)] = true;
      await this.loadChildren(connId, prefix);
    }
  }

  /** Reload the tree below `path` (everything, by default). */
  async refresh(connId: Uuid, path: string[] = []) {
    const s = this.sessions[connId];
    if (!s) return;
    const prefix = pathKey(path);
    this.#clearCompletion(connId, path);
    for (const k of Object.keys(s.nodes)) {
      if (prefix === "" || k === prefix || k.startsWith(prefix + "\u0000")) delete s.nodes[k];
    }
    for (const k of Object.keys(this.metaCache)) if (k.startsWith(`${connId}\u0000`)) delete this.metaCache[k];
    await this.loadChildren(connId, path, true);
    for (const [k, isOpen] of Object.entries(s.open)) {
      if (isOpen && !s.nodes[k] && (prefix === "" || k.startsWith(prefix))) await this.loadChildren(connId, k.split("\u0000"));
    }
  }

  async completionChildren(connId: Uuid, path: string[]): Promise<DbTreeNode[]> {
    const session = this.sessions[connId];
    if (!session) return [];
    const nodeKey = pathKey(path);
    if (session.nodes[nodeKey]) return session.nodes[nodeKey];
    const key = `${connId}\u0000${nodeKey}`;
    let pending = this.#completionChildren.get(key);
    if (!pending) {
      pending = api.db.children(session.sessionId, path).then((nodes) => {
        if (this.sessions[connId]?.sessionId === session.sessionId) session.nodes[nodeKey] = nodes;
        return nodes;
      }).catch(() => []);
      this.#completionChildren.set(key, pending);
    }
    try {
      return await pending;
    } finally {
      if (this.#completionChildren.get(key) === pending) this.#completionChildren.delete(key);
    }
  }

  completionTableInfo(connId: Uuid, namespace: string, table: string): Promise<DbTableInfo | null> {
    const session = this.sessions[connId];
    if (!session) return Promise.resolve(null);
    const key = `${connId}\u0000${namespace}\u0000${table}`;
    let pending = this.#completionTables.get(key);
    if (!pending) {
      pending = api.db.tableInfo(session.sessionId, namespace, table).catch(() => null);
      this.#completionTables.set(key, pending);
    }
    return pending;
  }

  #clearCompletion(connId: Uuid, path: string[] = []) {
    const prefix = `${connId}\u0000${pathKey(path)}`;
    for (const key of this.#completionChildren.keys()) if (key.startsWith(prefix)) this.#completionChildren.delete(key);
    const tablePrefix = `${connId}\u0000${path[0] ?? ""}`;
    for (const key of this.#completionTables.keys()) if (key.startsWith(tablePrefix)) this.#completionTables.delete(key);
    if (path.length === 0) this.#completionProviders.delete(connId);
  }

  // -- table details -----------------------------------------------------

  /** What a table is made of (indexes, constraints, size…), read once and kept until the tree is refreshed. */
  async tableMeta(connId: Uuid, database: string, table: string): Promise<DbTableMeta | null> {
    const s = this.sessions[connId];
    if (!s) return null;
    const key = `${connId}\u0000${database}\u0000${table}`;
    const cached = this.metaCache[key];
    if (cached && cached !== "loading" && !("error" in cached)) return cached;
    if (cached === "loading") return null;
    this.metaCache[key] = "loading";
    try {
      const meta = await api.db.tableMeta(s.sessionId, database, table);
      this.metaCache[key] = meta;
      return meta;
    } catch (e) {
      this.metaCache[key] = { error: errorMessage(e) };
      return null;
    }
  }

  cachedMeta(connId: Uuid, database: string, table: string) {
    return this.metaCache[`${connId}\u0000${database}\u0000${table}`];
  }

  // -- documents ---------------------------------------------------------

  #blank(connId: Uuid, sql: string, title: string): QueryTab {
    return {
      id: crypto.randomUUID(),
      connId,
      title,
      sql,
      result: null,
      error: null,
      running: false,
      queryId: null,
      startedAt: 0,
      limit: DEFAULT_LIMIT,
      table: null,
      info: null,
      sort: { col: null, dir: "asc" },
      savedId: null,
      baseline: sql,
      messages: [],
      resultTab: "results",
      sets: [],
      activeSet: 0,
      readOnly: false,
      view: "data",
      meta: null,
      metaLoading: false,
      metaError: null,
      page: { size: settings.prefs.dbPageSize, offset: 0, filter: "", filterError: null, order: [], hasNext: false, count: null, countLoading: false },
      staging: emptyStaging(),
      review: null,
      viewer: null,
    };
  }

  newTab(connId: Uuid, sql = "", title?: string): QueryTab {
    const n = this.tabs.filter((t) => t.connId === connId && !t.table).length + 1;
    this.tabs.push(this.#blank(connId, sql, title ?? `${this.connName(connId)} ${n}`));
    this.activeTabId = this.tabs[this.tabs.length - 1].id;
    return this.tabs[this.tabs.length - 1];
  }

  tab(id: string) {
    return this.tabs.find((t) => t.id === id);
  }

  /** Whether a tab's table is read a page at a time (SQL engines) rather than as one query. */
  isPaged(tab: QueryTab): boolean {
    return !!tab.table && isPageable(this.engineOf(tab.connId));
  }

  isDirty(tab: QueryTab): boolean {
    return !tab.table && tab.sql !== tab.baseline && tab.sql.trim() !== "";
  }

  /** Open a table's rows in a tab (the same table twice reuses its tab). */
  async openTable(connId: Uuid, database: string, table: string, view: TableView = "data") {
    let tab = this.tabs.find((t) => t.connId === connId && t.table?.database === database && t.table.table === table);
    const fresh = !tab;
    if (!tab) {
      tab = this.newTab(connId, browseText(this.engineOf(connId), database, table), table);
      tab.table = { database, table };
    }
    tab.view = view;
    this.activeTabId = tab.id;
    const s = await (this.sessions[connId] ?? this.connect(connId));
    if (!s) return;
    if (!fresh) return;
    const id = tab.id;
    void this.loadMeta(id);
    const info = await api.db.tableInfo(s.sessionId, database, table).catch(() => null);
    const t = this.tab(id);
    if (!t) return;
    t.info = info;
    // The default order of a table with a key is its key, so pages are stable.
    if (this.isPaged(t) && info?.primary_key.length) t.page.order = info.primary_key.map((column) => ({ column, desc: false }));
    await this.loadPage(id);
  }

  async loadMeta(tabId: string) {
    const tab = this.tab(tabId);
    if (!tab?.table) return;
    tab.metaLoading = true;
    tab.metaError = null;
    const meta = await this.tableMeta(tab.connId, tab.table.database, tab.table.table);
    const again = this.tab(tabId);
    if (!again) return;
    again.metaLoading = false;
    if (meta) again.meta = meta;
    else {
      const c = this.cachedMeta(again.connId, tab.table.database, tab.table.table);
      again.metaError = c && typeof c === "object" && "error" in c ? c.error : null;
    }
  }

  async duplicateTab(id: string) {
    const t = this.tab(id);
    if (!t) return;
    if (t.table) {
      const copy = this.newTab(t.connId, t.sql, t.title);
      copy.table = null;
      copy.title = `${t.title} (query)`;
      return copy;
    }
    return this.newTab(t.connId, t.sql, `${t.title} copy`);
  }

  async closeTab(id: string, force = false) {
    const i = this.tabs.findIndex((t) => t.id === id);
    if (i < 0) return;
    const tab = this.tabs[i];
    if (tab.running && !(await ask(`Stop the running query in "${tab.title}" and close it?`, { confirm: "Stop and close" }))) return;
    if (!force && !tab.running && hasPending(tab.staging) && !(await ask(`"${tab.title}" has changes that are not saved to the database (${pendingSummary(tab.staging)}). Close it and discard them?`, { confirm: "Discard and close", danger: true }))) return;
    if (!force && !tab.running && this.isDirty(tab) && !(await ask(`"${tab.title}" has changes that are not saved as a query. Close it anyway?`, { confirm: "Close", danger: true }))) return;
    if (tab.running) await this.cancel(id);
    const at = this.tabs.findIndex((t) => t.id === id);
    if (at < 0) return;
    this.tabs.splice(at, 1);
    if (this.activeTabId === id) this.activeTabId = this.tabs[Math.min(at, this.tabs.length - 1)]?.id ?? null;
  }

  async closeOthers(id: string) {
    for (const t of [...this.tabs]) if (t.id !== id) await this.closeTab(t.id);
  }

  /** Move to the next or previous tab, wrapping. */
  cycle(delta: number) {
    if (this.tabs.length < 2) return;
    const i = this.tabs.findIndex((t) => t.id === this.activeTabId);
    this.activeTabId = this.tabs[(i + delta + this.tabs.length) % this.tabs.length].id;
  }

  // -- running -----------------------------------------------------------

  async #confirmDestructive(connId: Uuid, reason: string, sql: string): Promise<boolean> {
    const prod = this.isProduction(connId);
    const shown = sql.trim().length > 400 ? `${sql.trim().slice(0, 400)}…` : sql.trim();
    return ask(`${reason}\n\n${shown}\n\n${prod ? "This is a PRODUCTION database. " : ""}This can't be undone. Run it anyway?`, {
      title: prod ? `Destructive statement on ${this.connName(connId)} (production)` : "Destructive statement",
      confirm: "Run it",
      danger: true,
      requireText: prod ? this.connName(connId) : undefined,
    });
  }

  #log(tab: QueryTab, kind: Message["kind"], text: string, extra: Partial<Message> = {}) {
    tab.messages.unshift({ at: Date.now(), kind, text, ...extra });
    if (tab.messages.length > MESSAGES_KEEP) tab.messages.length = MESSAGES_KEEP;
  }

  #record(tab: QueryTab, sql: string): { sql: string; at: number; connection: string } {
    const started = { sql, at: Date.now(), connection: this.connName(tab.connId) };
    const keep = settings.prefs.dbHistoryKeep;
    this.history = addEntry(this.history, started, keep);
    saveHistory(this.history, undefined, keep);
    return started;
  }

  #finish(started: { sql: string; at: number; connection: string }, outcome: { ms: number; rows: number | null; ok: boolean; error?: string }) {
    const keep = settings.prefs.dbHistoryKeep;
    this.history = finishEntry(this.history, started, outcome);
    saveHistory(this.history, undefined, keep);
  }

  /** One statement against the session, asking first when the server says it is destructive. Null: declined. */
  async #send(tab: QueryTab, sessionId: Uuid, text: string, limit: number): Promise<DbQueryResult | null> {
    let confirmed = false;
    for (;;) {
      tab.queryId = crypto.randomUUID();
      // A read-only tab on PostgreSQL begins each statement in a read-only transaction. A statement that ends the
      // transaction itself can get around it; a database account that can only read is the hard limit.
      const body = tab.readOnly && this.engineOf(tab.connId) === "postgres" && !this.#opensTransaction(text) ? `BEGIN READ ONLY; ${text.replace(/;\s*$/, "")}; ROLLBACK` : text;
      try {
        return await api.db.query(sessionId, tab.queryId, body, limit, confirmed);
      } catch (e) {
        if (isApiError(e) && e.code === "needs_confirmation" && !confirmed) {
          if (!(await this.#confirmDestructive(tab.connId, e.message, text))) return null;
          confirmed = true;
          continue;
        }
        throw e;
      }
    }
  }

  #opensTransaction(text: string) {
    return /^\s*(begin|start\s+transaction|commit|rollback|end|savepoint|release|set\s+transaction)\b/i.test(text);
  }

  /**
   * Run SQL in a tab. `mode` says what `text` is: a `statement` (or a selection) runs as one; a `script` is split and
   * run statement by statement, stopping at the first that fails, with a result for each statement that returns rows.
   * A script that manages its own transaction (BEGIN … COMMIT) is sent whole, because it can't be cut apart.
   */
  async run(tabId: string, text?: string, mode: "statement" | "script" = "statement") {
    const tab = this.tab(tabId);
    const sql = (text ?? tab?.sql ?? "").trim();
    if (!tab || !sql || tab.running) return;
    const connected = this.sessions[tab.connId] ?? (await this.connect(tab.connId));
    if (!connected) {
      tab.error = "Couldn't connect. The reason is in the notification.";
      this.#log(tab, "error", "Couldn't connect.");
      return;
    }
    const engine = this.engineOf(tab.connId);
    const whole = !engineInfo(engine).sql || mode === "statement" || !isScript(sql, engine) || splitStatements(sql, engine).some((s) => this.#opensTransaction(s.text));
    const parts = whole ? [sql] : splitStatements(sql, engine).map((s) => s.text);

    tab.running = true;
    tab.error = null;
    tab.startedAt = Date.now();
    tab.sets = [];
    tab.activeSet = 0;
    connected.runs++;
    let ran = 0;
    try {
      for (const [i, part] of parts.entries()) {
        const started = this.#record(tab, part);
        let res: DbQueryResult | null;
        try {
          res = await this.#send(tab, connected.sessionId, part, tab.limit);
        } catch (e) {
          const cancelled = isApiError(e) && e.code === "cancelled";
          const message = cancelled ? "Cancelled." : errorMessage(e);
          tab.error = parts.length > 1 ? `Statement ${i + 1} of ${parts.length}: ${message}` : message;
          this.#log(tab, cancelled ? "warning" : "error", tab.error, { sql: part, ms: Date.now() - started.at });
          this.#finish(started, { ms: Date.now() - started.at, rows: null, ok: false, error: message });
          if (isApiError(e) && e.code === "no_session") this.#lost(tab.connId);
          if (tab.sets.length === 0) tab.resultTab = "messages";
          return;
        }
        if (!res) {
          this.#log(tab, "warning", "Not run: the confirmation was declined.", { sql: part });
          this.#finish(started, { ms: 0, rows: null, ok: false, error: "declined" });
          return;
        }
        ran++;
        const rows = res.columns.length ? res.rows.length : (res.affected_rows ?? 0);
        this.#finish(started, { ms: res.elapsed_ms, rows, ok: true });
        this.#log(tab, "success", this.#summary(res), { sql: part, ms: res.elapsed_ms });
        if (res.columns.length || i === parts.length - 1) {
          tab.sets.push({ title: parts.length > 1 ? `Result ${tab.sets.length + 1}` : "Result", sql: part, result: res });
        }
      }
      // The last result that returned rows is shown; a script of only statements shows the last one's outcome.
      const lastRows = [...tab.sets].reverse().findIndex((s) => s.result.columns.length > 0);
      tab.activeSet = lastRows < 0 ? tab.sets.length - 1 : tab.sets.length - 1 - lastRows;
      tab.result = tab.sets[tab.activeSet]?.result ?? null;
      tab.sort = { col: null, dir: "asc" };
      tab.resultTab = "results";
      if (ran > 1) this.#log(tab, "info", `${ran} statements ran.`);
    } finally {
      tab.running = false;
      tab.queryId = null;
    }
  }

  #summary(r: DbQueryResult): string {
    if (r.columns.length === 0) {
      const n = r.affected_rows ?? 0;
      return `OK, ${n} row${n === 1 ? "" : "s"} affected${r.last_insert_id ? `, last insert id ${r.last_insert_id}` : ""} in ${r.elapsed_ms} ms`;
    }
    return `${r.rows.length.toLocaleString()} row${r.rows.length === 1 ? "" : "s"} returned in ${r.elapsed_ms} ms${r.truncated ? " (stopped at the row limit)" : ""}`;
  }

  /** Show the plan of a statement, without running it, as another result. */
  async explain(tabId: string, statement: string) {
    const tab = this.tab(tabId);
    if (!tab) return;
    const text = explainSql(this.engineOf(tab.connId), statement);
    if (!text) {
      ui.notify("info", "Explain is not available for this database type.");
      return;
    }
    await this.run(tabId, text, "statement");
    const t = this.tab(tabId);
    if (t?.sets.length) t.sets[t.sets.length - 1].title = "Plan";
  }

  selectSet(tabId: string, index: number) {
    const tab = this.tab(tabId);
    if (!tab?.sets[index]) return;
    tab.activeSet = index;
    tab.result = tab.sets[index].result;
    tab.sort = { col: null, dir: "asc" };
  }

  async cancel(tabId: string) {
    const tab = this.tab(tabId);
    const s = tab && this.sessions[tab.connId];
    if (!tab?.queryId || !s) return;
    try {
      await api.db.cancel(s.sessionId, tab.queryId);
    } catch (e) {
      ui.notify("error", `Couldn't cancel: ${errorMessage(e)}`);
    }
  }

  /** Click a header: ascending, then descending, then back to the server's order. */
  sortBy(tabId: string, col: number) {
    const tab = this.tab(tabId);
    if (!tab) return;
    if (tab.sort.col !== col) tab.sort = { col, dir: "asc" };
    else if (tab.sort.dir === "asc") tab.sort = { col, dir: "desc" };
    else tab.sort = { col: null, dir: "asc" };
  }

  // -- saved queries -----------------------------------------------------

  /** Save the tab's SQL: update the query it came from, or ask for a name. */
  async saveQuery(tabId: string, asNew = false): Promise<boolean> {
    const tab = this.tab(tabId);
    if (!tab || !tab.sql.trim()) return false;
    const existing = !asNew && tab.savedId ? this.saved.find((q) => q.id === tab.savedId) : undefined;
    let name = existing?.name;
    if (!existing) {
      name = (await askText("Name this query", tab.title.replace(/ \d+$/, ""), { title: "Save query", confirm: "Save", placeholder: "e.g. Orders this month" }))?.trim();
      if (!name) return false;
    }
    const q = existing
      ? { ...existing, sql: tab.sql, updatedAt: Date.now(), connectionId: tab.connId, connectionName: this.connName(tab.connId) }
      : newSaved({ name: name!, sql: tab.sql, connectionId: tab.connId, connectionName: this.connName(tab.connId) });
    this.saved = upsertSaved(this.saved, q);
    saveSaved(this.saved);
    tab.savedId = q.id;
    tab.baseline = tab.sql;
    if (!tab.table) tab.title = q.name;
    this.#log(tab, "info", `Saved as "${q.name}".`);
    ui.notify("info", `Saved "${q.name}" on this computer.`);
    return true;
  }

  updateSaved(id: string, patch: Partial<Pick<SavedQuery, "name" | "description" | "pinned">>) {
    const q = this.saved.find((x) => x.id === id);
    if (!q) return;
    this.saved = upsertSaved(this.saved, { ...q, ...patch, updatedAt: patch.pinned === undefined ? Date.now() : q.updatedAt });
    saveSaved(this.saved);
  }

  deleteSaved(id: string) {
    this.saved = removeSaved(this.saved, id);
    saveSaved(this.saved);
    for (const t of this.tabs) if (t.savedId === id) t.savedId = null;
  }

  /** Open a saved query in a new tab, on its own connection when that still exists. */
  openSaved(q: SavedQuery, fallbackConn: Uuid | null): QueryTab | null {
    const conn = (q.connectionId && vaultStore.dbConnections.some((c) => c.id === q.connectionId) ? q.connectionId : fallbackConn) ?? vaultStore.dbConnections.find((c) => c.data)?.id;
    if (!conn) return null;
    const tab = this.newTab(conn, q.sql, q.name);
    tab.savedId = q.id;
    tab.baseline = q.sql;
    return tab;
  }

  clearHistory() {
    this.history = [];
    saveHistory([], undefined, settings.prefs.dbHistoryKeep);
  }

  /** Apply a smaller retention right away. */
  trimHistory() {
    const keep = settings.prefs.dbHistoryKeep;
    this.history = this.history.slice(0, Math.max(0, keep));
    saveHistory(this.history, undefined, keep);
  }

  // -- a table, a page at a time ------------------------------------------

  /** Read the page the tab is on. Each read is an ordinary query, so the read-only session and the row limit apply. */
  async loadPage(tabId: string) {
    const tab = this.tab(tabId);
    if (!tab?.table) return;
    const engine = this.engineOf(tab.connId);
    if (!isPageable(engine)) {
      await this.run(tabId);
      return;
    }
    const problem = filterProblem(tab.page.filter, engine);
    tab.page.filterError = problem;
    if (problem) return;
    const connected = this.sessions[tab.connId] ?? (await this.connect(tab.connId));
    if (!connected) return;
    const sql = pageSql(engine, tab.table.database, tab.table.table, { filter: tab.page.filter, order: tab.page.order, limit: tab.page.size + 1, offset: tab.page.offset });
    if (!sql) return;
    tab.sql = sql;
    tab.running = true;
    tab.error = null;
    tab.startedAt = Date.now();
    const started = this.#record(tab, sql);
    try {
      const res = await api.db.query(connected.sessionId, (tab.queryId = crypto.randomUUID()), sql, tab.page.size + 1, true);
      tab.page.hasNext = res.rows.length > tab.page.size;
      if (tab.page.hasNext) res.rows.length = tab.page.size;
      tab.result = res;
      tab.sets = [{ title: "Data", sql, result: res }];
      tab.activeSet = 0;
      tab.staging = emptyStaging();
      this.#finish(started, { ms: res.elapsed_ms, rows: res.rows.length, ok: true });
      this.#log(tab, "success", `${res.rows.length.toLocaleString()} rows read from offset ${tab.page.offset.toLocaleString()} in ${res.elapsed_ms} ms`, { sql, ms: res.elapsed_ms });
    } catch (e) {
      const cancelled = isApiError(e) && e.code === "cancelled";
      tab.error = cancelled ? "Cancelled." : errorMessage(e);
      this.#log(tab, cancelled ? "warning" : "error", tab.error, { sql });
      this.#finish(started, { ms: Date.now() - started.at, rows: null, ok: false, error: tab.error });
      if (isApiError(e) && e.code === "no_session") this.#lost(tab.connId);
    } finally {
      tab.running = false;
      tab.queryId = null;
    }
  }

  /** Whether changing the page is allowed: pending changes would be lost, so the person is asked. */
  async #leave(tab: QueryTab): Promise<boolean> {
    if (!hasPending(tab.staging)) return true;
    return ask(`There are changes that are not saved to the database (${pendingSummary(tab.staging)}). Discard them and continue?`, { confirm: "Discard", danger: true });
  }

  async goToPage(tabId: string, offset: number) {
    const tab = this.tab(tabId);
    if (!tab || tab.running || !(await this.#leave(tab))) return;
    tab.page.offset = Math.max(0, offset);
    await this.loadPage(tabId);
  }

  async setFilter(tabId: string, filter: string) {
    const tab = this.tab(tabId);
    if (!tab || tab.running || !(await this.#leave(tab))) return;
    tab.page.filter = filter;
    tab.page.offset = 0;
    tab.page.count = null;
    await this.loadPage(tabId);
  }

  async setPageSize(tabId: string, size: number) {
    const tab = this.tab(tabId);
    if (!tab || tab.running || !(await this.#leave(tab))) return;
    tab.page.size = size;
    tab.page.offset = 0;
    settings.prefs.dbPageSize = size;
    await this.loadPage(tabId);
  }

  /** Click a header of a paged table: the server sorts, so the order applies to the whole table, not just this page. */
  async sortPage(tabId: string, column: string) {
    const tab = this.tab(tabId);
    if (!tab || tab.running || !(await this.#leave(tab))) return;
    const cur = tab.page.order[0];
    if (!cur || cur.column !== column) tab.page.order = [{ column, desc: false }];
    else if (!cur.desc) tab.page.order = [{ column, desc: true }];
    else tab.page.order = tab.info?.primary_key.map((c) => ({ column: c, desc: false })) ?? [];
    tab.page.offset = 0;
    await this.loadPage(tabId);
  }

  async refreshPage(tabId: string) {
    const tab = this.tab(tabId);
    if (!tab || tab.running || !(await this.#leave(tab))) return;
    await this.loadPage(tabId);
    void this.loadMeta(tabId);
  }

  /** An exact count of the rows (of the filtered rows). It reads the table, so it runs only when asked. */
  async countRows(tabId: string) {
    const tab = this.tab(tabId);
    const s = tab && this.sessions[tab.connId];
    if (!tab?.table || !s) return;
    const sql = countSql(this.engineOf(tab.connId), tab.table.database, tab.table.table, tab.page.filter);
    if (!sql) return;
    tab.page.countLoading = true;
    try {
      const res = await api.db.query(s.sessionId, crypto.randomUUID(), sql, 1, true);
      const n = Number(res.rows[0]?.[0]);
      tab.page.count = Number.isFinite(n) ? n : null;
    } catch (e) {
      ui.notify("error", `Couldn't count: ${errorMessage(e)}`);
    } finally {
      tab.page.countLoading = false;
    }
  }

  // -- pending changes ----------------------------------------------------

  /** Whether this tab's rows are changed through a reviewed set (PostgreSQL, MySQL) rather than one edit at a time. */
  stagedEditing(tab: QueryTab): boolean {
    return this.isPaged(tab) && !!this.sessions[tab.connId]?.changes && !!tab.info;
  }

  stageCell(tabId: string, row: number, col: number, value: string | null) {
    const tab = this.tab(tabId);
    if (!tab?.result) return;
    tab.staging = stageEdit(tab.staging, tab.result, row, col, value);
  }

  stageDelete(tabId: string, row: number) {
    const tab = this.tab(tabId);
    if (!tab?.result) return;
    const block = rowBlock(tab.info, tab.result, row);
    if (block) {
      ui.notify("info", block);
      return;
    }
    tab.staging = toggleDelete(tab.staging, row);
  }

  stageNewRow(tabId: string) {
    const tab = this.tab(tabId);
    if (tab) tab.staging = addInsert(tab.staging);
  }

  setNewValue(tabId: string, index: number, column: string, value: string | null | undefined) {
    const tab = this.tab(tabId);
    if (tab) tab.staging = setInsertValue(tab.staging, index, column, value);
  }

  dropNewRow(tabId: string, index: number) {
    const tab = this.tab(tabId);
    if (tab) tab.staging = removeInsert(tab.staging, index);
  }

  discardChanges(tabId: string) {
    const tab = this.tab(tabId);
    if (!tab) return;
    tab.staging = emptyStaging();
    tab.review = null;
  }

  /** Show exactly what would run, before anything does. */
  async reviewChanges(tabId: string) {
    const tab = this.tab(tabId);
    const s = tab && this.sessions[tab.connId];
    if (!tab?.table || !tab.result || !tab.info || !s || !hasPending(tab.staging)) return;
    const changes = toChanges(tab.table.database, tab.table.table, tab.info, tab.result, tab.staging);
    try {
      tab.review = { sql: await api.db.previewChanges(s.sessionId, changes), busy: false, error: null };
    } catch (e) {
      tab.review = { sql: [], busy: false, error: errorMessage(e) };
    }
  }

  /** Save the reviewed set in one transaction. On any failure nothing is saved and the pending changes stay. */
  async commitChanges(tabId: string): Promise<boolean> {
    const tab = this.tab(tabId);
    const s = tab && this.sessions[tab.connId];
    if (!tab?.table || !tab.result || !tab.info || !s || !tab.review) return false;
    const prod = this.isProduction(tab.connId);
    const deletes = tab.staging.deletes.length;
    if (deletes || prod) {
      const ok = await ask(
        `${pendingSummary(tab.staging)}.${deletes ? `\n\n${deletes} row${deletes === 1 ? "" : "s"} will be deleted. This can't be undone.` : ""}${prod ? "\n\nThis is a PRODUCTION database." : ""}`,
        { title: "Save these changes?", confirm: "Save changes", danger: true, requireText: prod ? this.connName(tab.connId) : undefined },
      );
      if (!ok) return false;
    }
    const changes = toChanges(tab.table.database, tab.table.table, tab.info, tab.result, tab.staging);
    tab.review.busy = true;
    tab.review.error = null;
    const started = this.#record(tab, tab.review.sql.join("\n"));
    try {
      const touched = await api.db.applyChanges(s.sessionId, changes);
      const total = touched.reduce((a, b) => a + b, 0);
      this.#finish(started, { ms: Date.now() - started.at, rows: total, ok: true });
      this.#log(tab, "success", `Saved ${changes.length} change${changes.length === 1 ? "" : "s"} (${total} row${total === 1 ? "" : "s"}) in one transaction.`, { sql: tab.review.sql.join("\n") });
      tab.review = null;
      tab.staging = emptyStaging();
      ui.notify("info", `Saved ${changes.length} change${changes.length === 1 ? "" : "s"}.`);
      for (const k of Object.keys(this.metaCache)) if (k.startsWith(`${tab.connId}\u0000${tab.table.database}\u0000${tab.table.table}`)) delete this.metaCache[k];
      await this.loadPage(tabId);
      void this.loadMeta(tabId);
      return true;
    } catch (e) {
      const message = errorMessage(e);
      this.#finish(started, { ms: Date.now() - started.at, rows: null, ok: false, error: message });
      this.#log(tab, "error", message);
      if (tab.review) {
        tab.review.busy = false;
        tab.review.error = message;
      }
      return false;
    }
  }

  // -- editing one cell at a time (engines without change sets) -----------

  /** Why this cell can't be edited, or null if it can. */
  editBlock(tab: QueryTab, row: number, col: number): string | null {
    return tab.result ? editBlock(tab.info, tab.result, col, row) : "No rows";
  }

  /** Show the UPDATE, and on approval run it. Returns whether the cell changed. */
  async editCell(tabId: string, row: number, col: number, value: string | null): Promise<boolean> {
    const tab = this.tab(tabId);
    const s = tab && this.sessions[tab.connId];
    if (!tab?.result || !tab.info || !tab.table || !s) return false;
    const block = editBlock(tab.info, tab.result, col, row);
    if (block) {
      ui.notify("error", block);
      return false;
    }
    const edit = buildRowEdit(tab.table.database, tab.table.table, tab.info, tab.result, row, col, value);
    try {
      const sql = await api.db.previewUpdate(s.sessionId, edit);
      const prod = this.isProduction(tab.connId);
      const noun = engineInfo(this.engineOf(tab.connId)).change;
      const ok = await ask(`${sql}\n\nThis changes one ${noun === "UPDATE" ? "row" : "document or value"}${prod ? " on a PRODUCTION database" : ""}.`, {
        title: `Run this ${noun}?`,
        confirm: `Run ${noun}`,
        // Always the careful kind: Cancel has the focus, so a stray Enter can't change data.
        danger: true,
        requireText: prod ? this.connName(tab.connId) : undefined,
      });
      if (!ok) return false;
      const changed = await api.db.applyUpdate(s.sessionId, edit);
      if (changed === 0) {
        ui.notify("error", "No row matched, so nothing changed. It may have been changed or deleted since you loaded it; run the query again.");
        return false;
      }
      tab.result.rows[row][col] = cellAfterEdit(tab.result.columns[col].kind, value);
      ui.notify("info", engineInfo(this.engineOf(tab.connId)).sql ? "1 row updated." : "Changed.");
      return true;
    } catch (e) {
      ui.notify("error", errorMessage(e));
      return false;
    }
  }
}

export const databases = new DatabasesStore();
