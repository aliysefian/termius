// The SQL behind the table view: one page of a table's rows with a filter and an order, a count on request, and
// EXPLAIN. The text is built here and sent as an ordinary query, so it is subject to the same safety checks and
// read-only session as anything typed in the editor. Identifiers are quoted per engine (dbengines.quoteName); the
// filter is an expression the person typed, never built from a value, and is refused if it holds a second statement.
import { quoteIdent, quoteName } from "./dbengines";
import { splitStatements } from "./dbstatement";

export interface OrderBy {
  column: string;
  desc: boolean;
}

export interface PageOptions {
  /** A `WHERE` expression without the word, as typed. */
  filter?: string;
  order?: OrderBy[];
  limit: number;
  offset: number;
}

/** Engines whose tables a page can be read from with LIMIT / OFFSET or its standard form. */
export const PAGEABLE = ["mysql", "postgres", "rqlite", "mssql", "oracle"] as const;
export const isPageable = (engine: string) => (PAGEABLE as readonly string[]).includes(engine);

/** Why a filter can't be used, or null when it is a single condition. */
export function filterProblem(filter: string, engine = "postgres"): string | null {
  const f = filter.trim();
  if (f === "") return null;
  if (splitStatements(f, engine).length > 1) return "A filter is one condition; remove the semicolon and what follows it.";
  if (/^\s*where\b/i.test(f)) return "Type the condition without the word WHERE.";
  return null;
}

const orderClause = (engine: string, order: OrderBy[]) => order.map((o) => `${quoteIdent(engine, o.column)}${o.desc ? " DESC" : ""}`).join(", ");

/** One page of a table, or null for an engine that isn't SQL. `database` is the first level above the table (a schema or a database). */
export function pageSql(engine: string, database: string, table: string, o: PageOptions): string | null {
  if (!isPageable(engine)) return null;
  const target = quoteName(engine, database, table);
  const where = o.filter?.trim() ? ` WHERE ${o.filter.trim()}` : "";
  const order = o.order?.length ? ` ORDER BY ${orderClause(engine, o.order)}` : "";
  const limit = Math.max(1, Math.floor(o.limit));
  const offset = Math.max(0, Math.floor(o.offset));
  if (engine === "mssql") {
    // OFFSET … FETCH needs an ORDER BY; with none given, any order will do.
    return `SELECT * FROM ${target}${where} ${order.trim() || "ORDER BY (SELECT NULL)"} OFFSET ${offset} ROWS FETCH NEXT ${limit} ROWS ONLY`;
  }
  if (engine === "oracle") {
    return `SELECT * FROM ${target}${where}${order} OFFSET ${offset} ROWS FETCH NEXT ${limit} ROWS ONLY`;
  }
  return `SELECT * FROM ${target}${where}${order} LIMIT ${limit} OFFSET ${offset}`;
}

/** An exact count. It reads the whole table (or the filtered part), so it runs only when asked. */
export function countSql(engine: string, database: string, table: string, filter?: string): string | null {
  if (!isPageable(engine)) return null;
  const where = filter?.trim() ? ` WHERE ${filter.trim()}` : "";
  return `SELECT COUNT(*) FROM ${quoteName(engine, database, table)}${where}`;
}

/** The statement that shows a query's plan without running it, or null where the engine needs more than one statement. */
export function explainSql(engine: string, statement: string): string | null {
  const text = statement.trim().replace(/;+\s*$/, "");
  if (text === "") return null;
  switch (engine) {
    case "postgres":
    case "mysql":
      return `EXPLAIN ${text}`;
    case "rqlite":
      return `EXPLAIN QUERY PLAN ${text}`;
    default:
      return null;
  }
}

/** The page number (1-based) an offset is on. */
export const pageOf = (offset: number, size: number) => Math.floor(offset / size) + 1;

/** The row range a page shows, "1–200", or "0" for an empty page. */
export function rangeText(offset: number, shown: number): string {
  return shown === 0 ? "0" : `${(offset + 1).toLocaleString()}–${(offset + shown).toLocaleString()}`;
}
