// Filtering the rows the viewer holds: text or a regular expression, severity (exactly one, or that and worse), host,
// unit and time. This works on what has already arrived; asking a host for older entries is a different thing (the
// stream's "history").
import { hayLowerOf, hayOf, type Row, type Sev, SEVS } from "./logrows";

export type TimeRange = "live" | "5m" | "15m" | "1h" | "custom";
export const RANGE_MS: Record<Exclude<TimeRange, "live" | "custom">, number> = { "5m": 5 * 60_000, "15m": 15 * 60_000, "1h": 3_600_000 };

export interface LogFilter {
  text: string;
  regex: boolean;
  /** "all", or a bucket. */
  sev: Sev | "all";
  /** With a bucket: that one and the worse ones, instead of exactly that one. */
  atLeast: boolean;
  /** Hosts to show; empty means all. */
  hosts: string[];
  /** Units to show; empty means all. A row matches a unit by the unit's name or the program's. */
  units: string[];
  range: TimeRange;
  from: number | null;
  to: number | null;
}

export const noFilter = (): LogFilter => ({ text: "", regex: false, sev: "all", atLeast: true, hosts: [], units: [], range: "live", from: null, to: null });

/** Whether any part of the filter is set (so "N of M" is worth showing). */
export function isFiltered(f: LogFilter): boolean {
  return f.text.trim() !== "" || f.sev !== "all" || f.hosts.length > 0 || f.units.length > 0 || f.range !== "live";
}

export const MAX_PATTERN = 200;

/**
 * Why a regular expression is refused, or null. A browser's regular expressions can take exponential time on some
 * patterns; this refuses the shapes that cause it (a repeated group that itself repeats, or repeats alternatives that
 * overlap) and anything long. It is a guard, not a proof, so a filter pass also stops at a time limit.
 */
export function regexRisk(pattern: string): string | null {
  if (pattern.length > MAX_PATTERN) return `The pattern is longer than ${MAX_PATTERN} characters.`;
  // A group containing an unbounded repeat, itself repeated: (a+)+  (.*)*  (a*){2,}
  if (/\((?:[^()\\]|\\.)*(?:[+*]|\{\d+,\d*\})(?:[^()\\]|\\.)*\)(?:[+*]|\{\d+,\d*\})/.test(pattern)) return "A repeated group that repeats inside itself can take very long to run. Simplify the pattern.";
  // A repeated alternation where one alternative is the start of another: (a|ab)*
  const alt = /\(\??:?((?:[^()\\|]|\\.)+(?:\|(?:[^()\\|]|\\.)+)+)\)(?:[+*]|\{\d+,\d*\})/.exec(pattern);
  if (alt) {
    const parts = alt[1].split("|");
    for (let i = 0; i < parts.length; i++) for (let j = 0; j < parts.length; j++) if (i !== j && parts[j].startsWith(parts[i])) return "A repeated choice with overlapping options can take very long to run. Simplify the pattern.";
  }
  // Back-references make matching exponential too.
  if (/\\[1-9]/.test(pattern)) return "Back-references aren't supported in a filter.";
  return null;
}

export interface Compiled {
  /** Why the text can't be used (a bad or risky pattern), else null. */
  error: string | null;
  test(r: Row): boolean;
  /** Where the text matches in `s`, as [start, end) pairs, for highlighting. */
  spans(s: string): [number, number][];
  /** True if a time or text pattern is in use. */
  active: boolean;
}

/** The longest stretch of a row's text a pattern is run on. */
const HAY_LIMIT = 4000;

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function compile(f: LogFilter, now = Date.now()): Compiled {
  let error: string | null = null;
  let re: RegExp | null = null;
  const text = f.text.trim();
  const needle = text.toLowerCase();
  if (text && f.regex) {
    error = regexRisk(f.text);
    if (!error) {
      try {
        re = new RegExp(f.text, "i");
      } catch (e) {
        error = e instanceof Error ? e.message.replace(/^Invalid regular expression: /, "") : "Not a valid pattern.";
      }
    }
  }
  const sevIndex = f.sev === "all" ? -1 : SEVS.indexOf(f.sev);
  const hosts = f.hosts.length ? new Set(f.hosts) : null;
  const units = f.units.length ? new Set(f.units) : null;
  let from = -Infinity;
  let to = Infinity;
  if (f.range === "custom") {
    if (f.from !== null) from = f.from;
    if (f.to !== null) to = f.to;
  } else if (f.range !== "live") {
    from = now - RANGE_MS[f.range];
  }
  const test = (r: Row): boolean => {
    if (sevIndex >= 0) {
      const i = SEVS.indexOf(r.sev);
      if (f.atLeast ? i > sevIndex : i !== sevIndex) return false;
    }
    if (hosts && !hosts.has(r.hostId)) return false;
    if (units && !units.has(r.unit) && !units.has(r.ident)) return false;
    if (r.ts < from || r.ts > to) return false;
    if (text && !error) {
      if (re) {
        const hay = hayOf(r);
        return re.test(hay.length > HAY_LIMIT ? hay.slice(0, HAY_LIMIT) : hay);
      }
      return hayLowerOf(r).includes(needle);
    }
    return true;
  };
  const spans = (s: string): [number, number][] => {
    if (!text || error) return [];
    const out: [number, number][] = [];
    const g = re ? new RegExp(re.source, "gi") : new RegExp(escapeRe(text), "gi");
    let m: RegExpExecArray | null;
    const hay = s.length > HAY_LIMIT ? s.slice(0, HAY_LIMIT) : s;
    while ((m = g.exec(hay)) && out.length < 50) {
      if (m[0].length === 0) {
        g.lastIndex++;
        continue;
      }
      out.push([m.index, m.index + m[0].length]);
    }
    return out;
  };
  return { error, test, spans, active: (!!text && !error) || f.range !== "live" };
}

export interface Filtered {
  rows: Row[];
  /** True if the pass stopped at its time limit, so `rows` is only the part checked. */
  timedOut: boolean;
}

/** The rows that pass, newest last. Stops after `budgetMs` so a slow pattern can't freeze the window. */
export function applyFilter(rows: Row[], c: Compiled, budgetMs = 250): Filtered {
  if (!c.active && !c.error) {
    // Severity, host and unit may still be set: run the test, but it is cheap.
  }
  const out: Row[] = [];
  const start = performance.now();
  for (let i = 0; i < rows.length; i++) {
    if ((i & 255) === 255 && performance.now() - start > budgetMs) return { rows: out, timedOut: true };
    if (c.test(rows[i])) out.push(rows[i]);
  }
  return { rows: out, timedOut: false };
}

/** A text to search for rows like this one: the first words of the message that aren't numbers or hex ids. */
export function similarTo(message: string): string {
  const words = message.split(/\s+/).filter((w) => w && !/\d/.test(w) && w.length > 1);
  const take = words.slice(0, 4).join(" ");
  return take || message.slice(0, 40);
}
