import { describe, expect, it } from "vitest";
import { buildRowEdit, cellAfterEdit, cellText, editBlock, isCut, quoteIdent, quoteName, sortedOrder, toCsv, toJson, toTsv, visibleRange } from "../dbdata";
import { HISTORY_KEY, addEntry, loadHistory, saveHistory } from "../dbhistory";
import type { DbCell, DbColumn, DbQueryResult, DbTableInfo } from "../types";

const col = (name: string, kind: DbColumn["kind"] = "text"): DbColumn => ({ name, kind, data_type: kind });
const result = (columns: DbColumn[], rows: DbCell[][]): DbQueryResult => ({ columns, rows, truncated: false, affected_rows: null, last_insert_id: null, elapsed_ms: 1 });
const info = (pk: string[], names = ["id", "name"]): DbTableInfo => ({
  columns: names.map((name) => ({ name, data_type: "x", nullable: true, primary_key: pk.includes(name), default: null })),
  primary_key: pk,
});

describe("cells", () => {
  it("shows NULL as a word and cut values with their size", () => {
    expect(cellText(null)).toBe("NULL");
    expect(cellText(0)).toBe("0");
    expect(cellText("")).toBe("");
    expect(cellText({ truncated: true, preview: "ab", bytes: 99 })).toBe("ab… (cut, 99 bytes)");
    expect(isCut({ truncated: true, preview: "", bytes: 1 })).toBe(true);
    expect(isCut("x")).toBe(false);
    expect(isCut(null)).toBe(false);
  });
});

describe("export", () => {
  const r = result([col("id", "number"), col("note")], [[1, "a,b"], [2, 'say "hi"'], [3, null], [4, "line\nbreak"]]);

  it("writes CSV that round-trips commas, quotes, newlines and NULL", () => {
    expect(toCsv(r)).toBe('id,note\r\n1,"a,b"\r\n2,"say ""hi"""\r\n3,\r\n4,"line\nbreak"\r\n');
  });

  it("writes TSV with tabs as the separator", () => {
    expect(toTsv(r).split("\r\n")[1]).toBe("1\ta,b");
  });

  it("defuses spreadsheet formulas but leaves negative numbers alone", () => {
    const f = result([col("v")], [["=SUM(A1)"], ["-5"], ["+1 555"], ["@x"], ["-1.5"]]);
    expect(toCsv(f).split("\r\n").slice(1, 6)).toEqual(["'=SUM(A1)", "-5", "'+1 555", "'@x", "-1.5"]);
  });

  it("writes JSON with real nulls and numbers", () => {
    expect(JSON.parse(toJson(r))).toEqual([
      { id: 1, note: "a,b" },
      { id: 2, note: 'say "hi"' },
      { id: 3, note: null },
      { id: 4, note: "line\nbreak" },
    ]);
  });

  it("keeps columns that share a name", () => {
    const d = result([col("id"), col("id")], [[1, 2]]);
    expect(JSON.parse(toJson(d))).toEqual([{ id: 1, id_2: 2 }]);
  });

  it("exports the preview of a cut value rather than a placeholder", () => {
    const c = result([col("t")], [[{ truncated: true, preview: "abc", bytes: 9 }]]);
    expect(toCsv(c)).toBe("t\r\nabc\r\n");
  });
});

describe("sorting loaded rows", () => {
  const rows: DbCell[][] = [[10, "b"], [2, "a"], [null, "c"], [33, null]];

  it("sorts numbers as numbers, with NULL first ascending and last descending", () => {
    expect(sortedOrder(rows, 0, "asc", "number")).toEqual([2, 1, 0, 3]);
    expect(sortedOrder(rows, 0, "desc", "number")).toEqual([3, 0, 1, 2]);
  });

  it("sorts text naturally and case-insensitively", () => {
    const t: DbCell[][] = [["item10"], ["Item2"], ["item1"]];
    expect(sortedOrder(t, 0, "asc", "text")).toEqual([2, 1, 0]);
  });

  it("leaves the order alone with no column, and ties stay stable", () => {
    expect(sortedOrder(rows, null, "asc", undefined)).toEqual([0, 1, 2, 3]);
    expect(sortedOrder([["a"], ["a"], ["a"]], 0, "desc", "text")).toEqual([0, 1, 2]);
  });

  it("does not move the data", () => {
    const copy = JSON.stringify(rows);
    sortedOrder(rows, 0, "asc", "number");
    expect(JSON.stringify(rows)).toBe(copy);
  });

  it("copes with 100k rows", () => {
    const big: DbCell[][] = Array.from({ length: 100_000 }, (_, i) => [(i * 7919) % 100_003]);
    const t = performance.now();
    const o = sortedOrder(big, 0, "asc", "number");
    expect(performance.now() - t).toBeLessThan(2000);
    expect(big[o[0]][0]).toBe(Math.min(...big.slice(0, 1000).map((r) => r[0] as number), 0));
  });
});

describe("virtual window", () => {
  it("draws only what fits plus a margin", () => {
    expect(visibleRange(0, 280, 28, 100_000)).toEqual({ start: 0, end: 18 });
    expect(visibleRange(2800, 280, 28, 100_000)).toEqual({ start: 92, end: 118 });
  });
  it("clamps at both ends and handles empty results", () => {
    expect(visibleRange(10_000_000, 280, 28, 50)).toEqual({ start: 41, end: 50 });
    expect(visibleRange(-50, 280, 28, 5)).toEqual({ start: 0, end: 5 });
    expect(visibleRange(0, 280, 28, 0)).toEqual({ start: 0, end: 0 });
  });
});

describe("inline editing", () => {
  const cols = [col("id", "number"), col("name"), col("blob", "binary")];
  const r = result(cols, [[7, "x", "0x00"], [null, "y", "0x01"]]);

  it("allows a plain cell of a keyed table", () => {
    expect(editBlock(info(["id"], ["id", "name", "blob"]), r, 1, 0)).toBeNull();
  });
  it("explains each refusal", () => {
    const i = info(["id"], ["id", "name", "blob"]);
    expect(editBlock(null, r, 1, 0)).toMatch(/open a table/i);
    expect(editBlock(info([], ["id", "name", "blob"]), r, 1, 0)).toMatch(/no primary key/);
    expect(editBlock(i, r, 2, 0)).toMatch(/binary/i);
    expect(editBlock(i, r, 1, 1)).toMatch(/key is NULL/);
    expect(editBlock(info(["id"], ["id", "name", "blob"]), result([col("name")], [["x"]]), 0, 0)).toMatch(/key column id/);
    const cut = result([col("id", "number"), col("t")], [[1, { truncated: true, preview: "a", bytes: 99999 }]]);
    expect(editBlock(info(["id"], ["id", "t"]), cut, 1, 0)).toMatch(/too long/);
    const dup = result([col("id", "number"), col("id")], [[1, 2]]);
    expect(editBlock(info(["id"], ["id"]), dup, 1, 0)).toMatch(/share this name/);
  });
  it("finds the row by its key, not its position", () => {
    const e = buildRowEdit("shop", "items", info(["id"], ["id", "name", "blob"]), r, 0, 1, "new");
    expect(e).toEqual({ database: "shop", table: "items", key: [{ column: "id", value: "7" }], changes: [{ column: "name", value: "new" }] });
  });
  it("uses every column of a composite key and keeps NULL as NULL", () => {
    const c = result([col("a", "number"), col("b"), col("v")], [[1, "k", null]]);
    const e = buildRowEdit("d", "t", info(["a", "b"], ["a", "b", "v"]), c, 0, 2, null);
    expect(e.key).toEqual([{ column: "a", value: "1" }, { column: "b", value: "k" }]);
    expect(e.changes).toEqual([{ column: "v", value: null }]);
  });
  it("shows an edited value as it will read back", () => {
    expect(cellAfterEdit("number", "12")).toBe(12);
    expect(cellAfterEdit("number", "1.50")).toBe("1.50");
    expect(cellAfterEdit("number", "abc")).toBe("abc");
    expect(cellAfterEdit("text", "x")).toBe("x");
    expect(cellAfterEdit("text", null)).toBeNull();
  });
});

describe("names", () => {
  it("quotes MySQL identifiers with backticks and doubles any inside", () => {
    expect(quoteName("mysql", "shop", "items")).toBe("`shop`.`items`");
    expect(quoteName("mysql", "a`b")).toBe("`a``b`");
  });
  it("quotes PostgreSQL identifiers with double quotes and doubles any inside", () => {
    expect(quoteName("postgres", "public", "items")).toBe('"public"."items"');
    expect(quoteName("postgres", 'Odd "Name"')).toBe('"Odd ""Name"""');
    expect(quoteName("postgres", "a`b")).toBe('"a`b"');
  });
  it("falls back to MySQL quoting for an unknown engine, never to bare names", () => {
    expect(quoteIdent("", "x")).toBe("`x`");
  });
});

describe("query history", () => {
  const e = (sql: string, connection = "c", at = 1) => ({ sql, connection, at });
  it("keeps newest first and moves repeats to the top", () => {
    let h = addEntry([], e("SELECT 1"));
    h = addEntry(h, e("SELECT 2"));
    h = addEntry(h, e("SELECT 1", "c", 3));
    expect(h.map((x) => x.sql)).toEqual(["SELECT 1", "SELECT 2"]);
    expect(h[0].at).toBe(3);
  });
  it("treats the same text on another connection as a different entry", () => {
    const h = addEntry(addEntry([], e("SELECT 1", "a")), e("SELECT 1", "b"));
    expect(h).toHaveLength(2);
  });
  it("ignores blanks and caps its length", () => {
    expect(addEntry([], e("   "))).toEqual([]);
    let h: ReturnType<typeof addEntry> = [];
    for (let i = 0; i < 150; i++) h = addEntry(h, e(`SELECT ${i}`), 100);
    expect(h).toHaveLength(100);
    expect(h[0].sql).toBe("SELECT 149");
  });
  it("survives missing, broken and hostile storage", () => {
    expect(loadHistory(undefined)).toEqual([]);
    expect(loadHistory({ getItem: () => "not json" })).toEqual([]);
    expect(loadHistory({ getItem: () => '{"a":1}' })).toEqual([]);
    expect(loadHistory({ getItem: () => '[{"sql":"x"},{"sql":"ok","at":1,"connection":"c"}]' })).toEqual([{ sql: "ok", at: 1, connection: "c" }]);
    expect(loadHistory({ getItem: () => { throw new Error("blocked"); } })).toEqual([]);
    expect(() => saveHistory([e("x")], { setItem: () => { throw new Error("full"); } })).not.toThrow();
  });
  it("round-trips through storage", () => {
    const store = new Map<string, string>();
    const s = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    saveHistory([e("SELECT 1")], s);
    expect(store.has(HISTORY_KEY)).toBe(true);
    expect(loadHistory(s)).toEqual([e("SELECT 1")]);
  });
});
