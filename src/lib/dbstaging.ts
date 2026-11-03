// Pending changes of the table view. An edit, a deleted row or a new row is held here, drawn differently in the
// grid, and sent to the server only when the person reviews and commits the whole set (one transaction). Nothing
// here talks to a database. Rows are named by their position in the loaded page, which doesn't change until the
// page is reloaded, and found on the server by their primary key as it was when loaded.
import { isCut } from "./dbdata";
import type { DbCell, DbChange, DbQueryResult, DbTableInfo } from "./types";

export interface Staging {
  /** Changed cells of loaded rows: row position → column name → new value (null is NULL). */
  edits: Record<number, Record<string, string | null>>;
  /** Positions of rows marked for deletion. */
  deletes: number[];
  /** New rows: column name → value. A column left out gets the table's default. */
  inserts: Record<string, string | null>[];
}

export const emptyStaging = (): Staging => ({ edits: {}, deletes: [], inserts: [] });

export const pendingCount = (s: Staging) => Object.keys(s.edits).length + s.deletes.length + s.inserts.length;
export const hasPending = (s: Staging) => pendingCount(s) > 0;

const asText = (c: DbCell): string | null => (c === null ? null : isCut(c) ? c.preview : String(c));

/** The cell as text, for comparing with what was typed. */
export function originalText(result: DbQueryResult, row: number, col: number): string | null {
  return asText(result.rows[row]?.[col] ?? null);
}

/** Stage a new value for a cell. Setting it back to what the server has removes the edit. */
export function stageEdit(s: Staging, result: DbQueryResult, row: number, col: number, value: string | null): Staging {
  const name = result.columns[col].name;
  const edits = { ...s.edits };
  const row_ = { ...(edits[row] ?? {}) };
  if (originalText(result, row, col) === value) delete row_[name];
  else row_[name] = value;
  if (Object.keys(row_).length === 0) delete edits[row];
  else edits[row] = row_;
  return { ...s, edits };
}

/** Mark a loaded row for deletion (or unmark it). A row that is deleted can't also be edited. */
export function toggleDelete(s: Staging, row: number): Staging {
  if (s.deletes.includes(row)) return { ...s, deletes: s.deletes.filter((r) => r !== row) };
  const edits = { ...s.edits };
  delete edits[row];
  return { ...s, edits, deletes: [...s.deletes, row] };
}

export function addInsert(s: Staging): Staging {
  return { ...s, inserts: [...s.inserts, {}] };
}

export function setInsertValue(s: Staging, index: number, column: string, value: string | null | undefined): Staging {
  const inserts = s.inserts.map((r, i) => {
    if (i !== index) return r;
    const next = { ...r };
    if (value === undefined) delete next[column];
    else next[column] = value;
    return next;
  });
  return { ...s, inserts };
}

export function removeInsert(s: Staging, index: number): Staging {
  return { ...s, inserts: s.inserts.filter((_, i) => i !== index) };
}

export type CellMark = "edited" | "deleted" | null;

export function cellMark(s: Staging, row: number, column: string): CellMark {
  if (s.deletes.includes(row)) return "deleted";
  return column in (s.edits[row] ?? {}) ? "edited" : null;
}

/** What a cell shows: the staged value when there is one, else the loaded one. */
export function shownValue(s: Staging, result: DbQueryResult, row: number, col: number): DbCell {
  const staged = s.edits[row]?.[result.columns[col].name];
  return staged !== undefined ? staged : (result.rows[row]?.[col] ?? null);
}

/** Why a delete or an in-place edit of this row can't be staged, or null when it can. */
export function rowBlock(info: DbTableInfo | null, result: DbQueryResult, row: number): string | null {
  if (!info) return "Open the table to change its rows.";
  if (info.primary_key.length === 0) return "This table has no primary key, so a row can't be told apart from another.";
  const names = result.columns.map((c) => c.name);
  const missing = info.primary_key.filter((k) => !names.includes(k));
  if (missing.length) return `The page doesn't include the key column ${missing.join(", ")}.`;
  for (const k of info.primary_key) {
    const v = result.rows[row]?.[names.indexOf(k)];
    if (v === null || v === undefined) return "This row's key is NULL, so it can't be identified.";
    if (isCut(v)) return "This row's key is too long to use.";
  }
  return null;
}

/** The changes to send: edits and deletes name their row by its key as loaded; inserts carry only what was typed. */
export function toChanges(database: string, table: string, info: DbTableInfo, result: DbQueryResult, s: Staging): DbChange[] {
  const names = result.columns.map((c) => c.name);
  const keyOf = (row: number) => info.primary_key.map((k) => ({ column: k, value: asText(result.rows[row][names.indexOf(k)]) }));
  const out: DbChange[] = [];
  for (const [rowText, cols] of Object.entries(s.edits)) {
    const row = Number(rowText);
    out.push({ op: "update", database, table, key: keyOf(row), changes: Object.entries(cols).map(([column, value]) => ({ column, value })) });
  }
  for (const row of s.deletes) out.push({ op: "delete", database, table, key: keyOf(row) });
  for (const values of s.inserts) out.push({ op: "insert", database, table, values: Object.entries(values).map(([column, value]) => ({ column, value })) });
  return out;
}

/** A one-line description of what is pending, for the bar above the grid. */
export function pendingSummary(s: Staging): string {
  const edited = Object.keys(s.edits).length;
  const parts = [
    edited && `${edited} row${edited === 1 ? "" : "s"} edited`,
    s.deletes.length && `${s.deletes.length} to delete`,
    s.inserts.length && `${s.inserts.length} new`,
  ].filter(Boolean);
  return parts.join(", ");
}

/** Whether a statement of a reviewed set is a DELETE, for the extra care deleting asks for. */
export const hasDeletes = (s: Staging) => s.deletes.length > 0;
