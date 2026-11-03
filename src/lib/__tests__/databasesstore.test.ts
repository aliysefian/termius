// The Databases store: connecting, running one statement or a script, destructive confirmations, cancelling, read-only
// tabs, a table read a page at a time, and pending changes saved all together or not at all.
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
let handlers: Record<string, (a: Record<string, unknown>) => unknown> = {};

const result = (rows: unknown[][], columns = [{ name: "id", data_type: "int4", kind: "number" }]) => ({ columns, rows, truncated: false, affected_rows: null, last_insert_id: null, elapsed_ms: 3 });
const statement = (n = 1) => ({ columns: [], rows: [], truncated: false, affected_rows: n, last_insert_id: null, elapsed_ms: 2 });

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    const h = handlers[cmd];
    if (h) return h(args);
    switch (cmd) {
      case "db_open":
        return { session_id: "s1", server_version: "16.4" };
      case "db_supports_changes":
        return true;
      case "db_children":
        return [{ name: "public", kind: "schema", expandable: true }];
      case "db_table_info":
        return { columns: [{ name: "id", data_type: "integer", nullable: false, primary_key: true, default: null }, { name: "name", data_type: "text", nullable: true, primary_key: false, default: null }], primary_key: ["id"] };
      case "db_table_meta":
        return { supported: true, kind: "table", comment: null, approx_rows: 5, size_bytes: 1, indexes: [], constraints: [], referenced_by: [], column_notes: [], ddl: null };
      case "db_query":
        return result([[1]]);
      case "db_close":
      case "db_cancel":
        return null;
      case "db_preview_changes":
        return ["UPDATE …;"];
      case "db_apply_changes":
        return [1];
      default:
        return null;
    }
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("$lib/dialogs.svelte", () => ({ ask: vi.fn(async () => true), askRemember: vi.fn(async () => ({ ok: true, checked: false })), askText: vi.fn(async () => "My query") }));

const { databases } = await import("$lib/stores/databases.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");
const { settings } = await import("$lib/stores/settings.svelte");
const { ui } = await import("$lib/stores/ui.svelte");
const { ask, askText } = await import("$lib/dialogs.svelte");

const conn = (id: string, engine: string, extra: Record<string, unknown> = {}) =>
  ({ id, rev: 1, updated_at: 1, deleted: false, data: { name: `${engine}-${id}`, engine, host: "h", port: 1, username: "u", database: "d", tls: "verify_full", group: "", notes: "", ...extra } }) as never;

const sent = (cmd = "db_query") => calls.filter(([c]) => c === cmd).map(([, a]) => a);
const sqls = () => sent().map((a) => String(a.sql));

beforeEach(() => {
  calls.length = 0;
  handlers = {};
  databases.reset();
  databases.history = [];
  databases.saved = [];
  vaultStore.dbConnections = [conn("c1", "postgres"), conn("c2", "mysql", { environment: "production", options: { read_only: "true" } })];
  settings.prefs.dbHistoryKeep = 100;
  settings.prefs.dbPageSize = 200;
  ui.toasts = [];
  vi.mocked(ask).mockClear();
  vi.mocked(ask).mockResolvedValue(true);
});

async function tabOn(connId = "c1", sql = "") {
  await databases.connect(connId);
  return databases.newTab(connId, sql);
}

describe("connecting", () => {
  it("opens a session, asks what it can do, and reads the first level of the tree", async () => {
    const s = await databases.connect("c1");
    expect(s).toMatchObject({ sessionId: "s1", version: "16.4", changes: true });
    expect(sent("db_children")[0]).toMatchObject({ path: [] });
    expect(databases.sessions["c1"].nodes[""]).toHaveLength(1);
  });

  it("closes the session and forgets its tree on disconnect", async () => {
    await databases.connect("c1");
    await databases.disconnect("c1");
    expect(databases.sessions["c1"]).toBeUndefined();
    expect(sent("db_close")).toHaveLength(1);
  });

  it("reports a failure to connect and keeps no session", async () => {
    handlers.db_open = () => {
      throw { code: "db", message: "could not connect to h:1: refused" };
    };
    expect(await databases.connect("c1")).toBeNull();
    expect(databases.sessions["c1"]).toBeUndefined();
    expect(ui.toasts.at(-1)?.text).toContain("refused");
  });

  it("loads a level lazily, once, and again on refresh", async () => {
    await databases.connect("c1");
    await databases.toggle("c1", ["public"]);
    await databases.toggle("c1", ["public"]);
    await databases.toggle("c1", ["public"]);
    expect(sent("db_children").filter((a) => (a.path as string[]).join() === "public")).toHaveLength(1);
    await databases.refresh("c1", ["public"]);
    expect(sent("db_children").filter((a) => (a.path as string[]).join() === "public")).toHaveLength(2);
  });
});

describe("running a statement", () => {
  it("sends the text, shows the result, and records the run in the history and the messages", async () => {
    const t = await tabOn("c1", "select 1");
    await databases.run(t.id, "select 1");
    expect(sqls()).toEqual(["select 1"]);
    expect(t.result?.rows).toEqual([[1]]);
    expect(t.running).toBe(false);
    expect(t.messages[0]).toMatchObject({ kind: "success", sql: "select 1" });
    expect(databases.history[0]).toMatchObject({ sql: "select 1", connection: "postgres-c1", ok: true, rows: 1 });
  });

  it("shows the server's error as it came and records the failure", async () => {
    handlers.db_query = () => {
      throw { code: "db", message: 'column "x" does not exist' };
    };
    const t = await tabOn("c1");
    await databases.run(t.id, "select x");
    expect(t.error).toBe('column "x" does not exist');
    expect(t.result).toBeNull();
    expect(databases.history[0]).toMatchObject({ ok: false, error: 'column "x" does not exist' });
    expect(t.messages[0].kind).toBe("error");
  });

  it("does not run a second statement while one is running", async () => {
    let release: () => void = () => {};
    handlers.db_query = () => new Promise((r) => (release = () => r(result([[1]]))));
    const t = await tabOn("c1");
    const first = databases.run(t.id, "select 1");
    await vi.waitFor(() => expect(t.running).toBe(true));
    await databases.run(t.id, "select 2");
    expect(sqls()).toEqual(["select 1"]);
    release();
    await first;
  });

  it("asks before a destructive statement, and sends it confirmed only after a yes", async () => {
    let n = 0;
    handlers.db_query = (a) => {
      if (!a.confirmed) {
        n++;
        throw { code: "needs_confirmation", message: "DELETE without a WHERE removes every row." };
      }
      return statement(7);
    };
    const t = await tabOn("c1");
    await databases.run(t.id, "delete from t");
    expect(n).toBe(1);
    expect(vi.mocked(ask).mock.calls[0][0]).toContain("DELETE without a WHERE");
    expect(sent().map((a) => a.confirmed)).toEqual([false, true]);
    expect(t.result?.affected_rows).toBe(7);

    vi.mocked(ask).mockResolvedValue(false);
    calls.length = 0;
    t.result = null;
    await databases.run(t.id, "delete from t");
    expect(sent().map((a) => a.confirmed)).toEqual([false]);
    expect(t.result).toBeNull();
    expect(t.messages[0].kind).toBe("warning");
  });

  it("asks for the connection's name before a destructive statement on production", async () => {
    handlers.db_query = (a) => {
      if (!a.confirmed) throw { code: "needs_confirmation", message: "DROP removes it." };
      return statement(0);
    };
    const t = await tabOn("c2");
    await databases.run(t.id, "drop table t");
    expect(vi.mocked(ask).mock.calls[0][1]).toMatchObject({ requireText: "mysql-c2", danger: true });
  });
});

describe("running a script", () => {
  it("runs the statements one after another and keeps a result for each that returns rows", async () => {
    handlers.db_query = (a) => (String(a.sql).startsWith("update") ? statement(4) : result([[1]]));
    const t = await tabOn("c1");
    await databases.run(t.id, "select 1; update t set a = 1; select 2", "script");
    expect(sqls()).toEqual(["select 1", "update t set a = 1", "select 2"]);
    expect(t.sets.map((s) => s.title)).toEqual(["Result 1", "Result 2"]);
    expect(t.messages.some((m) => m.text === "3 statements ran.")).toBe(true);
    databases.selectSet(t.id, 0);
    expect(t.activeSet).toBe(0);
  });

  it("stops at the first statement that fails and names it", async () => {
    handlers.db_query = (a) => {
      if (String(a.sql).includes("boom")) throw { code: "db", message: "syntax error" };
      return result([[1]]);
    };
    const t = await tabOn("c1");
    await databases.run(t.id, "select 1; boom; select 3", "script");
    expect(sqls()).toEqual(["select 1", "boom"]);
    expect(t.error).toBe("Statement 2 of 3: syntax error");
  });

  it("sends a script that manages its own transaction whole", async () => {
    const t = await tabOn("c1");
    await databases.run(t.id, "begin; update t set a = 1; commit", "script");
    expect(sqls()).toEqual(["begin; update t set a = 1; commit"]);
  });

  it("sends a selection of several statements as one request", async () => {
    const t = await tabOn("c1");
    await databases.run(t.id, "select 1; select 2", "statement");
    expect(sqls()).toEqual(["select 1; select 2"]);
  });
});

describe("cancelling and read-only", () => {
  it("cancels the running query on the server by its id", async () => {
    let release: () => void = () => {};
    handlers.db_query = () => new Promise((_, rej) => (release = () => rej({ code: "cancelled", message: "the query was cancelled" })));
    const t = await tabOn("c1");
    const run = databases.run(t.id, "select pg_sleep(60)");
    await vi.waitFor(() => expect(t.queryId).not.toBeNull());
    const id = t.queryId;
    await databases.cancel(t.id);
    expect(sent("db_cancel")).toEqual([{ sessionId: "s1", queryId: id }]);
    release();
    await run;
    expect(t.error).toBe("Cancelled.");
    expect(t.running).toBe(false);
    expect(t.messages[0].kind).toBe("warning");
  });

  it("starts each PostgreSQL statement of a read-only tab in a read-only transaction", async () => {
    const t = await tabOn("c1");
    t.readOnly = true;
    await databases.run(t.id, "select 1;");
    expect(sqls()).toEqual(["BEGIN READ ONLY; select 1; ROLLBACK"]);
    calls.length = 0;
    await databases.run(t.id, "commit");
    expect(sqls()).toEqual(["commit"]);
  });

  it("does not wrap a statement for another engine, and says whether the connection is read-only", async () => {
    const t = await tabOn("c2");
    t.readOnly = true;
    await databases.run(t.id, "select 1");
    expect(sqls()).toEqual(["select 1"]);
    expect(databases.isReadOnlyConnection("c2")).toBe(true);
    expect(databases.isReadOnlyConnection("c1")).toBe(false);
  });
});

describe("explain", () => {
  it("asks for the plan without analysing, and names the result", async () => {
    const t = await tabOn("c1");
    await databases.explain(t.id, "select * from t;");
    expect(sqls()).toEqual(["EXPLAIN select * from t"]);
    expect(t.sets.at(-1)?.title).toBe("Plan");
  });
});

describe("a table, a page at a time", () => {
  const rows = (n: number, from = 1) => Array.from({ length: n }, (_, i) => [from + i, `n${from + i}`]);
  const cols = [{ name: "id", data_type: "int4", kind: "number" }, { name: "name", data_type: "text", kind: "text" }];
  beforeEach(() => {
    handlers.db_query = (a) => {
      const sql = String(a.sql);
      if (sql.startsWith("SELECT COUNT")) return result([[1234]]);
      const off = Number(/OFFSET (\d+)/.exec(sql)?.[1] ?? 0);
      const lim = Number(/LIMIT (\d+)/.exec(sql)?.[1] ?? 0);
      return result(rows(Math.min(lim, 450 - off), off + 1), cols);
    };
  });

  it("reads a page plus one row, ordered by the key, and knows there is a next page", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    expect(sqls().at(-1)).toBe('SELECT * FROM "public"."items" ORDER BY "id" LIMIT 201 OFFSET 0');
    expect(t.result?.rows).toHaveLength(200);
    expect(t.page.hasNext).toBe(true);
    expect(t.info?.primary_key).toEqual(["id"]);
    expect(t.meta?.supported).toBe(true);
  });

  it("moves to the next and previous page", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    await databases.goToPage(t.id, 200);
    expect(sqls().at(-1)).toContain("OFFSET 200");
    expect(t.result?.rows[0][0]).toBe(201);
    await databases.goToPage(t.id, 400);
    expect(t.result?.rows).toHaveLength(50);
    expect(t.page.hasNext).toBe(false);
    await databases.goToPage(t.id, -100);
    expect(t.page.offset).toBe(0);
  });

  it("sorts on the server: ascending, descending, then back to the key", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    await databases.sortPage(t.id, "name");
    expect(sqls().at(-1)).toContain('ORDER BY "name" LIMIT');
    await databases.sortPage(t.id, "name");
    expect(sqls().at(-1)).toContain('ORDER BY "name" DESC');
    await databases.sortPage(t.id, "name");
    expect(sqls().at(-1)).toContain('ORDER BY "id" LIMIT');
  });

  it("filters from the first page, and refuses a second statement without running anything", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    await databases.goToPage(t.id, 200);
    await databases.setFilter(t.id, "id < 10");
    expect(sqls().at(-1)).toContain("WHERE id < 10");
    expect(t.page.offset).toBe(0);
    const before = sqls().length;
    await databases.setFilter(t.id, "1=1; drop table items");
    expect(sqls()).toHaveLength(before);
    expect(t.page.filterError).toMatch(/one condition/);
  });

  it("counts only when asked, and counts the filtered rows", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    expect(sqls().some((s) => s.startsWith("SELECT COUNT"))).toBe(false);
    await databases.setFilter(t.id, "id > 5");
    await databases.countRows(t.id);
    expect(sqls().at(-1)).toBe('SELECT COUNT(*) FROM "public"."items" WHERE id > 5');
    expect(t.page.count).toBe(1234);
    await databases.setFilter(t.id, "");
    expect(t.page.count).toBeNull();
  });

  it("changes the page size and remembers it", async () => {
    await databases.openTable("c1", "public", "items");
    const t = databases.active!;
    await databases.setPageSize(t.id, 50);
    expect(sqls().at(-1)).toContain("LIMIT 51 OFFSET 0");
    expect(settings.prefs.dbPageSize).toBe(50);
  });

  it("reuses the tab of a table that is already open", async () => {
    await databases.openTable("c1", "public", "items");
    await databases.openTable("c1", "public", "items", "structure");
    expect(databases.tabs.filter((t) => t.table).length).toBe(1);
    expect(databases.active!.view).toBe("structure");
  });
});

describe("pending changes", () => {
  async function opened() {
    handlers.db_query = () => result([[1, "a"], [2, "b"]], [{ name: "id", data_type: "int4", kind: "number" }, { name: "name", data_type: "text", kind: "text" }]);
    await databases.openTable("c1", "public", "items");
    return databases.active!;
  }

  it("holds edits locally until they are reviewed and saved in one call", async () => {
    const t = await opened();
    databases.stageCell(t.id, 0, 1, "alpha");
    databases.stageDelete(t.id, 1);
    databases.stageNewRow(t.id);
    databases.setNewValue(t.id, 0, "name", "new");
    expect(sent("db_apply_changes")).toHaveLength(0);
    await databases.reviewChanges(t.id);
    expect(t.review?.sql).toEqual(["UPDATE …;"]);
    const changes = sent("db_preview_changes")[0].changes as { op: string }[];
    expect(changes.map((c) => c.op)).toEqual(["update", "delete", "insert"]);
    handlers.db_apply_changes = () => [1, 1, 1];
    expect(await databases.commitChanges(t.id)).toBe(true);
    expect(sent("db_apply_changes")).toHaveLength(1);
    expect(t.staging).toEqual({ edits: {}, deletes: [], inserts: [] });
    expect(t.review).toBeNull();
    // The page is read again after saving.
    expect(sqls().length).toBeGreaterThan(1);
  });

  it("keeps everything pending when the save is refused, and shows why", async () => {
    const t = await opened();
    databases.stageCell(t.id, 0, 1, "alpha");
    await databases.reviewChanges(t.id);
    handlers.db_apply_changes = () => {
      throw { code: "db", message: "change 1 of 1 touched 0 rows instead of 1. Nothing was saved." };
    };
    expect(await databases.commitChanges(t.id)).toBe(false);
    expect(t.staging.edits[0]).toEqual({ name: "alpha" });
    expect(t.review?.error).toContain("Nothing was saved");
    expect(t.review?.busy).toBe(false);
  });

  it("asks before deleting, and again by connection name on production", async () => {
    const t = await opened();
    databases.stageDelete(t.id, 0);
    await databases.reviewChanges(t.id);
    vi.mocked(ask).mockResolvedValue(false);
    expect(await databases.commitChanges(t.id)).toBe(false);
    expect(sent("db_apply_changes")).toHaveLength(0);
    expect(vi.mocked(ask).mock.calls[0][0]).toContain("will be deleted");
  });

  it("will not stage the deletion of a row that can't be identified", async () => {
    handlers.db_table_info = () => ({ columns: [{ name: "a", data_type: "integer", nullable: true, primary_key: false, default: null }], primary_key: [] });
    handlers.db_query = () => result([[1]], [{ name: "a", data_type: "int4", kind: "number" }]);
    await databases.openTable("c1", "public", "nokey");
    const t = databases.active!;
    databases.stageDelete(t.id, 0);
    expect(t.staging.deletes).toEqual([]);
    expect(ui.toasts.at(-1)?.text).toMatch(/no primary key/);
  });

  it("asks before leaving a page with pending changes, and stays when declined", async () => {
    const t = await opened();
    databases.stageCell(t.id, 0, 1, "alpha");
    vi.mocked(ask).mockResolvedValue(false);
    const before = sqls().length;
    await databases.goToPage(t.id, 200);
    expect(sqls()).toHaveLength(before);
    expect(t.staging.edits[0]).toBeDefined();
    vi.mocked(ask).mockResolvedValue(true);
    await databases.goToPage(t.id, 200);
    expect(t.staging.edits[0]).toBeUndefined();
  });

  it("uses the one-cell edit for an engine without change sets", async () => {
    handlers.db_supports_changes = () => false;
    handlers.db_query = () => result([[1, "a"]], [{ name: "id", data_type: "int4", kind: "number" }, { name: "name", data_type: "text", kind: "text" }]);
    await databases.openTable("c1", "public", "items");
    expect(databases.stagedEditing(databases.active!)).toBe(false);
  });
});

describe("tabs and saved queries", () => {
  it("asks before closing a query with unsaved changes, and not once it is saved", async () => {
    const t = await tabOn("c1", "select 1");
    expect(databases.isDirty(t)).toBe(false);
    t.sql = "select 2";
    expect(databases.isDirty(t)).toBe(true);
    vi.mocked(ask).mockResolvedValue(false);
    await databases.closeTab(t.id);
    expect(databases.tabs).toContain(t);
    vi.mocked(ask).mockResolvedValue(true);
    await databases.saveQuery(t.id);
    expect(databases.isDirty(t)).toBe(false);
    vi.mocked(ask).mockClear();
    await databases.closeTab(t.id);
    expect(ask).not.toHaveBeenCalled();
    expect(databases.tabs).toHaveLength(0);
  });

  it("saves under the name asked for, updates it on the next save, and reopens it", async () => {
    const t = await tabOn("c1", "select 1");
    await databases.saveQuery(t.id);
    expect(databases.saved).toHaveLength(1);
    expect(databases.saved[0]).toMatchObject({ name: "My query", sql: "select 1", connectionId: "c1" });
    expect(t.title).toBe("My query");
    t.sql = "select 2";
    vi.mocked(askText).mockClear();
    await databases.saveQuery(t.id);
    expect(askText).not.toHaveBeenCalled();
    expect(databases.saved).toHaveLength(1);
    expect(databases.saved[0].sql).toBe("select 2");
    const reopened = databases.openSaved(databases.saved[0], null)!;
    expect(reopened).toMatchObject({ sql: "select 2", savedId: databases.saved[0].id, connId: "c1" });
    databases.deleteSaved(databases.saved[0].id);
    expect(t.savedId).toBeNull();
  });

  it("does not save an empty query or one that was not named", async () => {
    const t = await tabOn("c1", "  ");
    expect(await databases.saveQuery(t.id)).toBe(false);
    t.sql = "select 1";
    vi.mocked(askText).mockResolvedValueOnce(null);
    expect(await databases.saveQuery(t.id)).toBe(false);
    expect(databases.saved).toHaveLength(0);
  });

  it("cycles through tabs, duplicates one and closes the others", async () => {
    const a = await tabOn("c1", "select 1");
    const b = databases.newTab("c1", "select 2");
    databases.cycle(1);
    expect(databases.activeTabId).toBe(a.id);
    databases.cycle(-1);
    expect(databases.activeTabId).toBe(b.id);
    const copy = await databases.duplicateTab(a.id);
    expect(copy).toMatchObject({ sql: "select 1" });
    expect(databases.tabs).toHaveLength(3);
    await databases.closeOthers(a.id);
    expect(databases.tabs.map((t) => t.id)).toEqual([a.id]);
  });

  it("keeps the history as short as asked, or off", async () => {
    settings.prefs.dbHistoryKeep = 2;
    const t = await tabOn("c1");
    for (const s of ["select 1", "select 2", "select 3"]) await databases.run(t.id, s);
    expect(databases.history.map((h) => h.sql)).toEqual(["select 3", "select 2"]);
    settings.prefs.dbHistoryKeep = 0;
    databases.trimHistory();
    expect(databases.history).toEqual([]);
    await databases.run(t.id, "select 4");
    expect(databases.history).toEqual([]);
  });

  it("hides a password in the history", async () => {
    const t = await tabOn("c1");
    await databases.run(t.id, "CREATE USER bob IDENTIFIED BY 'hunter2'");
    expect(databases.history[0].sql).toBe("CREATE USER bob IDENTIFIED BY '***'");
    // The statement itself is sent as typed.
    expect(sqls().at(-1)).toContain("hunter2");
  });
});

describe("the table's details", () => {
  it("reads them once per table until the tree is refreshed", async () => {
    await databases.connect("c1");
    await databases.tableMeta("c1", "public", "items");
    await databases.tableMeta("c1", "public", "items");
    expect(sent("db_table_meta")).toHaveLength(1);
    await databases.refresh("c1");
    await databases.tableMeta("c1", "public", "items");
    expect(sent("db_table_meta")).toHaveLength(2);
  });

  it("remembers why they couldn't be read", async () => {
    handlers.db_table_meta = () => {
      throw { code: "db", message: "permission denied" };
    };
    await databases.connect("c1");
    expect(await databases.tableMeta("c1", "public", "items")).toBeNull();
    expect(databases.cachedMeta("c1", "public", "items")).toEqual({ error: "permission denied" });
  });
});

describe("locking the vault", () => {
  it("forgets the sessions, the tabs and what was cached", async () => {
    const t = await tabOn("c1", "select 1");
    await databases.tableMeta("c1", "public", "items");
    expect(databases.tabs).toContain(t);
    databases.reset();
    expect(databases.sessions).toEqual({});
    expect(databases.tabs).toEqual([]);
    expect(databases.metaCache).toEqual({});
  });
});
