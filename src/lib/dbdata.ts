// Pure helpers for the Databases view: turning results into text, sorting
// loaded rows, picking which rows to draw, and building inline edits.
import type { DbCell, DbCellEdit, DbColumn, DbQueryResult, DbRowEdit, DbTableInfo } from "./types";

/** A cell the server cut short: shown, never editable. */
export const isCut = (c: DbCell): c is { truncated: true; preview: string; bytes: number } =>
  typeof c === "object" && c !== null && "truncated" in c;

/** What a cell shows in the grid. NULL is its own word, not an empty string. */
export function cellText(c: DbCell): string {
  if (c === null) return "NULL";
  if (isCut(c)) return `${c.preview}… (cut, ${c.bytes} bytes)`;
  return String(c);
}

/** What a cell becomes in an export: NULL is empty (CSV) or null (JSON). */
function plain(c: DbCell): string | number | boolean | null {
  if (c === null) return null;
  if (isCut(c)) return c.preview;
  return c;
}

function csvField(v: string, sep: string): string {
  // Quote anything a spreadsheet could misread, and defuse formulas.
  const needsQuote = v.includes(sep) || /["\r\n]/.test(v) || /^\s|\s$/.test(v);
  const safe = /^[=+\-@\t\r]/.test(v) && !/^-?\d+(\.\d+)?$/.test(v) ? `'${v}` : v;
  return needsQuote ? `"${safe.replace(/"/g, '""')}"` : safe;
}

function delimited(r: Pick<DbQueryResult, "columns" | "rows">, sep: string): string {
  const line = (cells: string[]) => cells.map((c) => csvField(c, sep)).join(sep);
  const out = [line(r.columns.map((c) => c.name))];
  for (const row of r.rows) out.push(line(row.map((c) => (plain(c) === null ? "" : String(plain(c))))));
  return out.join("\r\n") + "\r\n";
}

export const toCsv = (r: Pick<DbQueryResult, "columns" | "rows">) => delimited(r, ",");
export const toTsv = (r: Pick<DbQueryResult, "columns" | "rows">) => delimited(r, "\t");

/** An array of objects keyed by column name. Duplicate names get a suffix so none is lost. */
export function toJson(r: Pick<DbQueryResult, "columns" | "rows">): string {
  const seen = new Map<string, number>();
  const names = r.columns.map((c) => {
    const n = seen.get(c.name) ?? 0;
    seen.set(c.name, n + 1);
    return n === 0 ? c.name : `${c.name}_${n + 1}`;
  });
  return JSON.stringify(
    r.rows.map((row) => Object.fromEntries(row.map((c, i) => [names[i], plain(c)]))),
    null,
    2,
  );
}

export type SortDir = "asc" | "desc";

/** Row positions in display order. Sorting never moves the data, so edits keep pointing at the right row. */
export function sortedOrder(rows: DbCell[][], col: number | null, dir: SortDir, kind: DbColumn["kind"] | undefined): number[] {
  const order = rows.map((_, i) => i);
  if (col === null) return order;
  const num = kind === "number";
  const key = (i: number): string | number | null => {
    const c = rows[i][col];
    if (c === null) return null;
    const v = plain(c);
    if (num) {
      const n = typeof v === "number" ? v : Number(v);
      return Number.isNaN(n) ? String(v) : n;
    }
    return String(v);
  };
  const sign = dir === "asc" ? 1 : -1;
  return order.sort((a, b) => {
    const x = key(a);
    const y = key(b);
    // NULLs sort first ascending, last descending, like MySQL.
    if (x === null || y === null) return x === y ? a - b : (x === null ? -1 : 1) * sign;
    if (typeof x === "number" && typeof y === "number") return (x - y) * sign || a - b;
    // Numbers before text when a numeric column holds both.
    if (typeof x !== typeof y) return (typeof x === "number" ? -1 : 1) * sign;
    return String(x).localeCompare(String(y), undefined, { numeric: true, sensitivity: "base" }) * sign || a - b;
  });
}

/** Which rows to draw for a scroll position: a window plus a little either side. */
export function visibleRange(scrollTop: number, viewport: number, rowHeight: number, total: number, overscan = 8): { start: number; end: number } {
  if (total <= 0 || rowHeight <= 0) return { start: 0, end: 0 };
  // A scroll position past the end (rows shrank under it) still draws the last rows.
  const first = Math.min(total - 1, Math.floor(Math.max(0, scrollTop) / rowHeight));
  const count = Math.ceil(Math.max(0, viewport) / rowHeight);
  return { start: Math.max(0, first - overscan), end: Math.min(total, first + count + overscan) };
}

/** Whether a cell can be edited in place, and if not, why (shown on hover). */
export function editBlock(info: DbTableInfo | null, result: DbQueryResult, col: number, row: number): string | null {
  if (!info) return "Open a table to edit its rows";
  if (info.primary_key.length === 0) return "This table has no primary key, so a row can't be identified";
  const names = result.columns.map((c) => c.name);
  const missing = info.primary_key.filter((k) => !names.includes(k));
  if (missing.length) return `The result doesn't include the key column ${missing.join(", ")}`;
  const column = result.columns[col];
  if (!info.columns.some((c) => c.name === column.name)) return "Not a column of this table";
  if (names.filter((n) => n === column.name).length > 1) return "Two columns share this name";
  if (column.kind === "binary") return "Binary values can't be edited here";
  const cell = result.rows[row]?.[col];
  if (cell !== undefined && isCut(cell)) return "This value is too long to edit here";
  for (const k of info.primary_key) {
    const v = result.rows[row]?.[names.indexOf(k)];
    if (v === null || v === undefined) return "The row's key is NULL";
    if (isCut(v)) return "The row's key is too long to use";
  }
  return null;
}

const asValue = (c: DbCell): string | null => (c === null ? null : String(plain(c)));

/** The edit for one cell: which table, which row (by key), what changes. */
export function buildRowEdit(
  database: string,
  table: string,
  info: DbTableInfo,
  result: DbQueryResult,
  row: number,
  col: number,
  value: string | null,
): DbRowEdit {
  const names = result.columns.map((c) => c.name);
  const key: DbCellEdit[] = info.primary_key.map((k) => ({ column: k, value: asValue(result.rows[row][names.indexOf(k)]) }));
  return { database, table, key, changes: [{ column: result.columns[col].name, value }] };
}

/** What a typed value turns into in the grid after a successful save. */
export function cellAfterEdit(kind: DbColumn["kind"], value: string | null): DbCell {
  if (value === null) return null;
  if (kind === "number" && value.trim() !== "" && Number.isFinite(Number(value)) && Math.abs(Number(value)) <= Number.MAX_SAFE_INTEGER && /^-?\d+(\.\d+)?$/.test(value.trim())) {
    // Keep decimals as typed so "1.50" doesn't become 1.5.
    return value.includes(".") ? value.trim() : Number(value);
  }
  return value;
}

const BACKTICK = /`/g;
/** `db`.`table` for use in a SELECT. */
export const quoteName = (...parts: string[]) => parts.map((p) => `\`${p.replace(BACKTICK, "``")}\``).join(".");
