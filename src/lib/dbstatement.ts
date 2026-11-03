// Which statement a Ctrl+Enter runs. The editor holds a script; the person means "the one my cursor is in" unless
// they selected something. Statements end at a `;` that is not inside a string, a quoted name, a comment or a
// PostgreSQL dollar-quoted body, so `SELECT ';'` and a function body with semicolons stay whole. Pure and tested.

export interface Statement {
  /** The statement's text, without its terminating `;`, trimmed. */
  text: string;
  /** Where it starts and ends in the script (the end is after the `;` when there is one). */
  from: number;
  to: number;
}

type Dialect = "postgres" | "mysql" | "other";

const dialectOf = (engine: string): Dialect => (engine === "postgres" ? "postgres" : engine === "mysql" ? "mysql" : "other");

/** The statements of a script, in order. Empty ones (blank lines, stray `;`) are dropped. */
export function splitStatements(sql: string, engine = "postgres"): Statement[] {
  const dialect = dialectOf(engine);
  const out: Statement[] = [];
  let start = 0;
  let i = 0;
  const push = (end: number, withTerminator: boolean) => {
    const raw = sql.slice(start, end);
    // A piece that is only comments and whitespace is not a statement.
    if (stripComments(raw).trim() !== "") out.push({ text: raw.trim(), from: start + (raw.length - raw.trimStart().length), to: withTerminator ? end + 1 : end });
    start = end + 1;
  };
  while (i < sql.length) {
    const c = sql[i];
    const next = sql[i + 1];
    if (c === "-" && next === "-") {
      i = lineEnd(sql, i);
    } else if (c === "#" && dialect === "mysql") {
      i = lineEnd(sql, i);
    } else if (c === "/" && next === "*") {
      const end = sql.indexOf("*/", i + 2);
      i = end < 0 ? sql.length : end + 2;
    } else if (c === "'" || c === '"' || c === "`") {
      i = quoteEnd(sql, i, c, dialect === "mysql" && c !== "`");
    } else if (c === "$" && dialect === "postgres") {
      const tag = /^\$[A-Za-z_\u0080-￿]?[\w\u0080-￿]*\$/.exec(sql.slice(i, i + 64))?.[0];
      if (tag) {
        const end = sql.indexOf(tag, i + tag.length);
        i = end < 0 ? sql.length : end + tag.length;
      } else i++;
    } else if (c === ";") {
      push(i, true);
      i++;
    } else i++;
  }
  push(sql.length, false);
  return out;
}

function lineEnd(sql: string, from: number): number {
  const n = sql.indexOf("\n", from);
  return n < 0 ? sql.length : n + 1;
}

/** The index after the quote that closes the one opened at `from`. A doubled quote is an escaped one. */
function quoteEnd(sql: string, from: number, quote: string, backslash: boolean): number {
  let i = from + 1;
  while (i < sql.length) {
    if (backslash && sql[i] === "\\") i += 2;
    else if (sql[i] === quote) {
      if (sql[i + 1] === quote) i += 2;
      else return i + 1;
    } else i++;
  }
  return sql.length;
}

/** Comments removed (strings kept), to tell a comment-only piece from a statement. */
function stripComments(sql: string): string {
  return sql.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/--[^\n]*/g, " ").replace(/^\s*#[^\n]*/gm, " ");
}

/**
 * The statement the cursor is in. At the very end of a statement (right after its `;`) or on blank space between
 * two, the statement before it wins, which is what a person who just typed one expects.
 */
export function statementAt(sql: string, pos: number, engine = "postgres"): Statement | null {
  const all = splitStatements(sql, engine);
  if (all.length === 0) return null;
  const inside = all.find((s) => pos >= s.from && pos <= s.to);
  if (inside) return inside;
  const before = [...all].reverse().find((s) => s.to <= pos);
  return before ?? all[0];
}

/** Whether running this text would run more than one statement. */
export const isScript = (sql: string, engine = "postgres") => splitStatements(sql, engine).length > 1;
