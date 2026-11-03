import { describe, expect, it } from "vitest";
import { buildRows, groupKey, highlightParts, matches, moveFocus, parentIndex, passesType, pathKey, type ConnectionView, type SessionView } from "../dbtree";
import type { DbTreeNode } from "../types";

const n = (name: string, kind: DbTreeNode["kind"], expandable = kind === "table" || kind === "view" || kind === "schema"): DbTreeNode => ({ name, kind, expandable });

function conn(nodes: Record<string, DbTreeNode[]>, open: Record<string, boolean> = {}, extra: Partial<SessionView> = {}): ConnectionView {
  return { id: "c1", name: "shop", group: "", engine: "postgres", session: { nodes, open, loading: {}, failed: {}, ...extra } };
}

const base = { query: "", type: "all" as const, groups: {}, collapsed: {} };
const labels = (rows: ReturnType<typeof buildRows>) => rows.map((r) => `${"  ".repeat(r.depth)}${r.kind === "group" ? `[${r.label} ${r.count}]` : r.label}`);

describe("buildRows", () => {
  const tree = {
    "": [n("public", "schema"), n("warehouse", "schema")],
    public: [n("customers", "table"), n("items", "table"), n("v_items", "view"), n("add_one", "function", false), n("noop", "procedure", false), n("myseq", "sequence", false)],
  };

  it("shows a closed connection as one row and a disconnected one without a tree", () => {
    expect(buildRows([{ id: "c1", name: "shop", group: "", engine: "postgres" }], base).map((r) => r.connected)).toEqual([false]);
    expect(labels(buildRows([conn(tree)], { ...base, collapsed: { c1: true } }))).toEqual(["shop"]);
  });

  it("groups a schema's objects by kind, with counts, and opens Tables and Views first", () => {
    const rows = buildRows([conn(tree, { public: true })], base);
    expect(labels(rows)).toEqual([
      "shop",
      "  public",
      "    [Tables 2]",
      "      customers",
      "      items",
      "    [Views 1]",
      "      v_items",
      "    [Functions 1]",
      "    [Procedures 1]",
      "    [Sequences 1]",
      "  warehouse",
    ]);
    // Routines are closed until opened.
    const open = buildRows([conn(tree, { public: true })], { ...base, groups: { [groupKey("c1", ["public"], "functions")]: true } });
    expect(labels(open)).toContain("      add_one");
  });

  it("does not group a level that holds one kind of thing", () => {
    const rows = buildRows([conn({ "": [n("db1", "database"), n("db2", "database")] })], base);
    expect(labels(rows)).toEqual(["shop", "  db1", "  db2"]);
  });

  it("groups a table's columns and indexes", () => {
    const t = { ...tree, "public\u0000items": [n("id", "column", false), n("name", "column", false), n("items_pkey", "index", false), n("items_trg", "trigger", false)] };
    const rows = buildRows([conn(t, { public: true, [pathKey(["public", "items"])]: true })], base);
    const at = labels(rows);
    expect(at).toContain("        [Columns 2]");
    expect(at).toContain("        [Indexes 1]");
    expect(at).toContain("        [Triggers 1]");
  });

  it("shows loading and error lines where a level is not there", () => {
    const loading = buildRows([conn({ "": [n("public", "schema")] }, { public: true }, { loading: { public: true } })], base);
    expect(loading.at(-1)?.kind).toBe("loading");
    const failed = buildRows([conn({ "": [n("public", "schema")] }, { public: true }, { failed: { public: "permission denied" } })], base);
    expect(failed.at(-1)).toMatchObject({ kind: "error", label: "permission denied" });
  });

  it("names a connection group once, above its connections", () => {
    const a: ConnectionView = { ...conn(tree), id: "a", name: "alpha", group: "Production" };
    const b: ConnectionView = { id: "b", name: "beta", group: "Production", engine: "mysql" };
    const c: ConnectionView = { id: "c", name: "gamma", group: "", engine: "mysql" };
    const rows = buildRows([a, b, c], { ...base, collapsed: { a: true } });
    expect(rows.map((r) => `${r.kind}:${r.label}`)).toEqual(["section:Production", "connection:alpha", "connection:beta", "connection:gamma"]);
  });

  it("gives every row a distinct key", () => {
    const rows = buildRows([conn(tree, { public: true })], base);
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
  });

  describe("search", () => {
    it("keeps matching objects with the path to them, groups opened, and hides the rest", () => {
      const rows = buildRows([conn(tree, { public: true })], { ...base, query: "item" });
      expect(labels(rows)).toEqual(["shop", "  public", "    [Tables 1]", "      items", "    [Views 1]", "      v_items"]);
    });

    it("opens a table whose loaded columns match", () => {
      const t = { ...tree, "public\u0000items": [n("id", "column", false), n("sku", "column", false)] };
      const rows = buildRows([conn(t, { public: true })], { ...base, query: "sku" });
      expect(labels(rows)).toContain("        sku");
    });

    it("is case-insensitive and finds a schema by name", () => {
      expect(labels(buildRows([conn(tree)], { ...base, query: "WARE" }))).toEqual(["shop", "  warehouse"]);
    });

    it("hides a connection with no match, but keeps one whose name matches", () => {
      const a = conn(tree);
      const b: ConnectionView = { id: "c2", name: "reports", group: "", engine: "mysql" };
      expect(buildRows([a, b], { ...base, query: "zzz" })).toEqual([]);
      expect(buildRows([a, b], { ...base, query: "report" }).map((r) => r.label)).toEqual(["reports"]);
    });

    it("lists what the server found that the tree hasn't loaded, once", () => {
      const hits = { c1: [
        { parent: ["warehouse"], name: "pairs", kind: "table" as const, detail: null },
        { parent: ["public"], name: "items", kind: "table" as const, detail: null },
      ] };
      const rows = buildRows([conn(tree, { public: true })], { ...base, query: "i", hits });
      const hitRows = rows.filter((r) => r.kind === "hit");
      expect(hitRows.map((r) => r.label)).toEqual(["pairs"]);
      expect(rows.find((r) => r.kind === "hits-header")?.label).toBe("More on the server (1)");
      const busy = buildRows([conn(tree)], { ...base, query: "zz", searching: { c1: true } });
      expect(busy.find((r) => r.kind === "hits-header")?.label).toContain("Searching");
    });

    it("filters by object type", () => {
      const rows = buildRows([conn(tree, { public: true })], { ...base, type: "views" });
      expect(labels(rows)).toEqual(["shop", "  public", "    v_items", "  warehouse"].filter((l) => l !== "  warehouse" || true));
      expect(passesType("function", "routines") && passesType("procedure", "routines") && !passesType("table", "routines")).toBe(true);
      expect(passesType("sequence", "other") && !passesType("sequence", "tables")).toBe(true);
    });
  });
});

describe("helpers", () => {
  it("matches case-insensitively", () => {
    expect(matches("Items", "tem")).toBe(true);
    expect(matches("Items", "")).toBe(true);
    expect(matches("Items", "x")).toBe(false);
  });

  it("splits a name into matching and plain parts", () => {
    expect(highlightParts("customer_items", "item")).toEqual([
      { text: "customer_", match: false },
      { text: "item", match: true },
      { text: "s", match: false },
    ]);
    expect(highlightParts("abc", "")).toEqual([{ text: "abc", match: false }]);
    expect(highlightParts("aXbxc", "x").filter((p) => p.match)).toHaveLength(2);
  });

  it("moves focus within the rows", () => {
    const rows = buildRows([conn({ "": [n("a", "database"), n("b", "database")] })], { query: "", type: "all", groups: {}, collapsed: {} });
    expect(moveFocus(rows, 0, "ArrowDown")).toBe(1);
    expect(moveFocus(rows, 2, "ArrowDown")).toBe(2);
    expect(moveFocus(rows, 1, "ArrowUp")).toBe(0);
    expect(moveFocus(rows, 0, "ArrowUp")).toBe(0);
    expect(moveFocus(rows, 1, "End")).toBe(2);
    expect(moveFocus([], 0, "Home")).toBe(-1);
  });

  it("finds a row's parent", () => {
    const rows = buildRows([conn({ "": [n("a", "database")] }, {})], { query: "", type: "all", groups: {}, collapsed: {} });
    expect(parentIndex(rows, 1)).toBe(0);
    expect(parentIndex(rows, 0)).toBe(-1);
  });
});
