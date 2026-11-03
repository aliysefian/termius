import { beforeEach, describe, expect, it } from "vitest";
import { addEntry, finishEntry, loadHistory, redactSecrets, saveHistory, search as searchHistory, type HistoryEntry } from "../dbhistory";
import { load, newSaved, remove, save, search, sortSaved, upsert, SAVED_KEY } from "../savedqueries";

class Mem {
  data: Record<string, string> = {};
  getItem(k: string) { return this.data[k] ?? null; }
  setItem(k: string, v: string) { this.data[k] = v; }
}

describe("saved queries", () => {
  let mem: Mem;
  beforeEach(() => (mem = new Mem()));

  it("saves, loads, edits and removes", () => {
    const a = newSaved({ name: "  Big orders ", sql: "select 1", connectionId: "c1", connectionName: "shop" }, 10);
    expect(a).toMatchObject({ name: "Big orders", description: "", pinned: false, updatedAt: 10 });
    let list = upsert([], a);
    list = upsert(list, { ...a, sql: "select 2", updatedAt: 20 });
    expect(list).toHaveLength(1);
    save(list, mem);
    expect(load(mem)[0].sql).toBe("select 2");
    expect(remove(list, a.id)).toEqual([]);
  });

  it("sorts pinned first, then newest, and searches every field", () => {
    const a = { ...newSaved({ name: "old", sql: "select a" }, 1), pinned: false };
    const b = { ...newSaved({ name: "new", sql: "select b", description: "monthly report" }, 5), pinned: false };
    const c = { ...newSaved({ name: "pin", sql: "select c", connectionName: "warehouse" }, 0), pinned: true };
    expect(sortSaved([a, b, c]).map((q) => q.name)).toEqual(["pin", "new", "old"]);
    expect(search([a, b, c], "REPORT").map((q) => q.name)).toEqual(["new"]);
    expect(search([a, b, c], "warehouse").map((q) => q.name)).toEqual(["pin"]);
    expect(search([a, b, c], "")).toHaveLength(3);
  });

  it("survives junk in storage and gives a name to an unnamed query", () => {
    mem.setItem(SAVED_KEY, "not json");
    expect(load(mem)).toEqual([]);
    mem.setItem(SAVED_KEY, JSON.stringify([{ id: "1", name: "n", sql: "s" }, { nope: true }, 5]));
    expect(load(mem)).toHaveLength(1);
    expect(newSaved({ name: " ", sql: "x" }).name).toBe("Untitled query");
  });
});

describe("history", () => {
  it("hides the password in a statement that sets one", () => {
    expect(redactSecrets("CREATE USER bob IDENTIFIED BY 'hunter2'")).toBe("CREATE USER bob IDENTIFIED BY '***'");
    expect(redactSecrets("ALTER USER bob WITH PASSWORD 'it''s secret'")).toBe("ALTER USER bob WITH PASSWORD '***'");
    expect(redactSecrets("SET PASSWORD FOR 'bob'@'%' = 'x1'")).toBe("SET PASSWORD FOR 'bob'@'%' = '***'");
    expect(redactSecrets("select 'password' from t")).toBe("select 'password' from t");
  });

  it("records a run when it starts and how it ended when it finishes", () => {
    const started = { sql: "select 1", at: 100, connection: "shop" };
    let list = addEntry([], started);
    expect(list[0].ok).toBeUndefined();
    list = finishEntry(list, started, { ms: 12, rows: 1, ok: true });
    expect(list[0]).toMatchObject({ ok: true, ms: 12, rows: 1 });
    list = addEntry(list, { sql: "select 1", at: 200, connection: "shop" });
    expect(list).toHaveLength(2);
    const failed = { sql: "select x", at: 300, connection: "shop" };
    list = finishEntry(addEntry(list, failed), failed, { ms: 3, rows: null, ok: false, error: "column x does not exist" });
    expect(list[0]).toMatchObject({ ok: false, error: "column x does not exist" });
  });

  it("keeps only as many as asked, or none", () => {
    let list: HistoryEntry[] = [];
    for (let i = 0; i < 5; i++) list = addEntry(list, { sql: `select ${i}`, at: i, connection: "c", ok: true }, 3);
    expect(list.map((h) => h.sql)).toEqual(["select 4", "select 3", "select 2"]);
    expect(addEntry(list, { sql: "select 9", at: 9, connection: "c" }, 0)).toEqual([]);
  });

  it("searches sql, connection and error text, and persists", () => {
    const mem = new Mem();
    const list: HistoryEntry[] = [
      { sql: "select * from orders", at: 1, connection: "shop", ok: true },
      { sql: "select x", at: 2, connection: "warehouse", ok: false, error: "no such column" },
    ];
    expect(searchHistory(list, "ORDERS")).toHaveLength(1);
    expect(searchHistory(list, "warehouse")).toHaveLength(1);
    expect(searchHistory(list, "such column")).toHaveLength(1);
    saveHistory(list, 100, mem);
    expect(loadHistory(mem)).toEqual(list);
    saveHistory(list, 1, mem);
    expect(loadHistory(mem)).toHaveLength(1);
  });
});
