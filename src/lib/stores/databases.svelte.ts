// The Databases view's live state: open sessions, their browse trees, and the
// query tabs. Saved connections live in the vault store; everything here is
// per window and goes away when the vault locks.
import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { buildRowEdit, cellAfterEdit, editBlock, quoteName, type SortDir } from "$lib/dbdata";
import { addEntry, loadHistory, saveHistory, type HistoryEntry } from "$lib/dbhistory";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type DbQueryResult, type DbTableInfo, type DbTreeNode, type Uuid } from "$lib/types";

export const ROW_LIMITS = [100, 1000, 10_000, 100_000] as const;
export const DEFAULT_LIMIT = 1000;

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
}

export interface ConnState {
  sessionId: Uuid;
  version: string;
  /** Children by path; the root is the empty path. */
  nodes: Record<string, DbTreeNode[]>;
  open: Record<string, boolean>;
  loading: Record<string, boolean>;
  failed: Record<string, string>;
}

/** Tree paths are used as object keys. */
export const pathKey = (path: string[]) => path.join("\u0000");

class DatabasesStore {
  sessions = $state<Record<Uuid, ConnState>>({});
  connecting = $state<Record<Uuid, boolean>>({});
  tabs = $state<QueryTab[]>([]);
  activeTabId = $state<string | null>(null);
  history = $state<HistoryEntry[]>(loadHistory());

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
    }
  }

  connName(connId: Uuid): string {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.name ?? "database";
  }

  isProduction(connId: Uuid): boolean {
    return vaultStore.dbConnections.find((c) => c.id === connId)?.data?.environment === "production";
  }

  // -- connections -------------------------------------------------------

  async connect(connId: Uuid): Promise<ConnState | null> {
    if (this.sessions[connId]) return this.sessions[connId];
    if (this.connecting[connId]) return null;
    this.connecting[connId] = true;
    try {
      const opened = await api.db.open(connId);
      this.sessions[connId] = { sessionId: opened.session_id, version: opened.server_version, nodes: {}, open: {}, loading: {}, failed: {} };
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
    for (const t of this.tabs.filter((t) => t.connId === connId)) {
      if (t.queryId) t.error = "The connection was closed.";
      t.running = false;
    }
    if (s) await api.db.close(s.sessionId).catch(() => {});
  }

  /** The session went away under us (server closed it, tunnel dropped). */
  #lost(connId: Uuid) {
    delete this.sessions[connId];
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

  /** Reload the tree below `path` (everything, by default). */
  async refresh(connId: Uuid, path: string[] = []) {
    const s = this.sessions[connId];
    if (!s) return;
    const prefix = pathKey(path);
    for (const k of Object.keys(s.nodes)) {
      if (prefix === "" || k === prefix || k.startsWith(prefix + "\u0000")) delete s.nodes[k];
    }
    await this.loadChildren(connId, path, true);
    for (const [k, isOpen] of Object.entries(s.open)) {
      if (isOpen && !s.nodes[k] && (prefix === "" || k.startsWith(prefix))) await this.loadChildren(connId, k.split("\u0000"));
    }
  }

  // -- tabs --------------------------------------------------------------

  newTab(connId: Uuid, sql = "", title?: string): QueryTab {
    const n = this.tabs.filter((t) => t.connId === connId && !t.table).length + 1;
    this.tabs.push({
      id: crypto.randomUUID(),
      connId,
      title: title ?? `${this.connName(connId)} ${n}`,
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
    });
    this.activeTabId = this.tabs[this.tabs.length - 1].id;
    return this.tabs[this.tabs.length - 1];
  }

  /** Open a table's rows in a tab (the same table twice reuses its tab). */
  async openTable(connId: Uuid, database: string, table: string) {
    let tab = this.tabs.find((t) => t.connId === connId && t.table?.database === database && t.table.table === table);
    if (!tab) {
      tab = this.newTab(connId, `SELECT * FROM ${quoteName(database, table)}`, table);
      tab.table = { database, table };
    }
    this.activeTabId = tab.id;
    const s = await (this.sessions[connId] ?? this.connect(connId));
    if (!s) return;
    const [info] = await Promise.all([api.db.tableInfo(s.sessionId, database, table).catch(() => null), this.run(tab.id)]);
    tab.info = info;
  }

  async closeTab(id: string) {
    const i = this.tabs.findIndex((t) => t.id === id);
    if (i < 0) return;
    const tab = this.tabs[i];
    if (tab.running && !(await ask(`Stop the running query in "${tab.title}" and close it?`, { confirm: "Stop and close" }))) return;
    if (tab.running) await this.cancel(id);
    this.tabs.splice(i, 1);
    if (this.activeTabId === id) this.activeTabId = this.tabs[Math.min(i, this.tabs.length - 1)]?.id ?? null;
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

  /** Run the tab's statement (or `sql`, such as a selection). Destructive ones ask first. */
  async run(tabId: string, sql?: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    const text = (sql ?? tab?.sql ?? "").trim();
    if (!tab || !text || tab.running) return;
    const connected = this.sessions[tab.connId] ?? (await this.connect(tab.connId));
    if (!connected) {
      tab.error = "Couldn't connect. The reason is in the notification.";
      return;
    }
    tab.running = true;
    tab.error = null;
    tab.queryId = crypto.randomUUID();
    tab.startedAt = Date.now();
    this.history = addEntry(this.history, { sql: text, at: Date.now(), connection: this.connName(tab.connId) });
    saveHistory(this.history);
    try {
      let confirmed = false;
      for (;;) {
        try {
          const res = await api.db.query(connected.sessionId, tab.queryId, text, tab.limit, confirmed);
          tab.result = res;
          tab.sort = { col: null, dir: "asc" };
          break;
        } catch (e) {
          if (isApiError(e) && e.code === "needs_confirmation" && !confirmed) {
            if (!(await this.#confirmDestructive(tab.connId, e.message, text))) return;
            confirmed = true;
            continue;
          }
          throw e;
        }
      }
    } catch (e) {
      tab.error = isApiError(e) && e.code === "cancelled" ? "Cancelled." : errorMessage(e);
      if (isApiError(e) && e.code === "no_session") this.#lost(tab.connId);
    } finally {
      tab.running = false;
      tab.queryId = null;
    }
  }

  async cancel(tabId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
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
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    if (tab.sort.col !== col) tab.sort = { col, dir: "asc" };
    else if (tab.sort.dir === "asc") tab.sort = { col, dir: "desc" };
    else tab.sort = { col: null, dir: "asc" };
  }

  // -- editing -----------------------------------------------------------

  /** Why this cell can't be edited, or null if it can. */
  editBlock(tab: QueryTab, row: number, col: number): string | null {
    return tab.result ? editBlock(tab.info, tab.result, col, row) : "No rows";
  }

  /** Show the UPDATE, and on approval run it. Returns whether the cell changed. */
  async editCell(tabId: string, row: number, col: number, value: string | null): Promise<boolean> {
    const tab = this.tabs.find((t) => t.id === tabId);
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
      const ok = await ask(`${sql}\n\nThis changes one row${prod ? " on a PRODUCTION database" : ""}.`, {
        title: "Run this UPDATE?",
        confirm: "Run UPDATE",
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
      ui.notify("info", "1 row updated.");
      return true;
    } catch (e) {
      ui.notify("error", errorMessage(e));
      return false;
    }
  }
}

export const databases = new DatabasesStore();
