import { describe, expect, it } from "vitest";
import { addInsert, cellMark, emptyStaging, hasPending, pendingCount, pendingSummary, removeInsert, rowBlock, setInsertValue, shownValue, stageEdit, toChanges, toggleDelete } from "../dbstaging";
import type { DbQueryResult, DbTableInfo } from "../types";

const result: DbQueryResult = {
  columns: [
    { name: "id", data_type: "int4", kind: "number" },
    { name: "name", data_type: "text", kind: "text" },
    { name: "note", data_type: "text", kind: "text" },
  ],
  rows: [
    [1, "a", null],
    [2, "b", "x"],
    [3, "c", "y"],
  ],
  truncated: false,
  affected_rows: null,
  last_insert_id: null,
  elapsed_ms: 1,
};
const info: DbTableInfo = {
  columns: [
    { name: "id", data_type: "integer", nullable: false, primary_key: true, default: null },
    { name: "name", data_type: "text", nullable: true, primary_key: false, default: null },
    { name: "note", data_type: "text", nullable: true, primary_key: false, default: null },
  ],
  primary_key: ["id"],
};

describe("staging cells", () => {
  it("holds an edit, shows it, and marks the cell", () => {
    const s = stageEdit(emptyStaging(), result, 0, 1, "alpha");
    expect(shownValue(s, result, 0, 1)).toBe("alpha");
    expect(shownValue(s, result, 1, 1)).toBe("b");
    expect(cellMark(s, 0, "name")).toBe("edited");
    expect(cellMark(s, 0, "note")).toBeNull();
    expect(hasPending(s)).toBe(true);
  });

  it("drops the edit when the value goes back to what the server has", () => {
    let s = stageEdit(emptyStaging(), result, 1, 2, "changed");
    s = stageEdit(s, result, 1, 2, "x");
    expect(hasPending(s)).toBe(false);
    // NULL is a value too: typing nothing as NULL on a NULL cell is no change.
    expect(hasPending(stageEdit(emptyStaging(), result, 0, 2, null))).toBe(false);
    expect(hasPending(stageEdit(emptyStaging(), result, 1, 2, null))).toBe(true);
  });

  it("never mutates the staging it was given", () => {
    const a = emptyStaging();
    stageEdit(a, result, 0, 1, "z");
    expect(a).toEqual(emptyStaging());
  });
});

describe("staging rows", () => {
  it("marks a row for deletion and unmarks it; a deleted row loses its edits", () => {
    let s = stageEdit(emptyStaging(), result, 2, 1, "zzz");
    s = toggleDelete(s, 2);
    expect(s.edits[2]).toBeUndefined();
    expect(cellMark(s, 2, "name")).toBe("deleted");
    s = toggleDelete(s, 2);
    expect(s.deletes).toEqual([]);
  });

  it("collects new rows and their values", () => {
    let s = addInsert(emptyStaging());
    s = addInsert(s);
    s = setInsertValue(s, 0, "name", "n");
    s = setInsertValue(s, 0, "note", null);
    s = setInsertValue(s, 0, "note", undefined);
    expect(s.inserts).toEqual([{ name: "n" }, {}]);
    expect(removeInsert(s, 1).inserts).toEqual([{ name: "n" }]);
  });
});

describe("toChanges", () => {
  it("names each row by its key as loaded and sends only what changed", () => {
    let s = stageEdit(emptyStaging(), result, 1, 1, "bee");
    s = stageEdit(s, result, 1, 2, null);
    s = toggleDelete(s, 2);
    s = setInsertValue(addInsert(s), 0, "name", "new");
    expect(toChanges("public", "t", info, result, s)).toEqual([
      { op: "update", database: "public", table: "t", key: [{ column: "id", value: "2" }], changes: [{ column: "name", value: "bee" }, { column: "note", value: null }] },
      { op: "delete", database: "public", table: "t", key: [{ column: "id", value: "3" }] },
      { op: "insert", database: "public", table: "t", values: [{ column: "name", value: "new" }] },
    ]);
  });

  it("uses every column of a composite key", () => {
    const composite: DbTableInfo = { ...info, primary_key: ["id", "name"] };
    const s = stageEdit(emptyStaging(), result, 0, 2, "q");
    const [c] = toChanges("d", "t", composite, result, s);
    expect(c).toMatchObject({ key: [{ column: "id", value: "1" }, { column: "name", value: "a" }] });
  });
});

describe("rowBlock", () => {
  it("allows rows of a table with a key, and says why not otherwise", () => {
    expect(rowBlock(info, result, 0)).toBeNull();
    expect(rowBlock(null, result, 0)).toMatch(/Open the table/);
    expect(rowBlock({ ...info, primary_key: [] }, result, 0)).toMatch(/no primary key/);
    expect(rowBlock({ ...info, primary_key: ["missing"] }, result, 0)).toMatch(/missing/);
    const nullKey: DbQueryResult = { ...result, rows: [[null, "a", null]] };
    expect(rowBlock(info, nullKey, 0)).toMatch(/NULL/);
  });
});

describe("summary", () => {
  it("counts what is pending", () => {
    let s = stageEdit(emptyStaging(), result, 0, 1, "z");
    s = toggleDelete(s, 1);
    s = addInsert(s);
    expect(pendingCount(s)).toBe(3);
    expect(pendingSummary(s)).toBe("1 row edited, 1 to delete, 1 new");
    expect(pendingSummary(emptyStaging())).toBe("");
  });
});
