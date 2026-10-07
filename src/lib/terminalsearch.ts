// Searching what every open terminal has on screen and in its scrollback at once. The terminals register what
// they can read and how to show a line; the search itself is plain functions over lines of text.

export interface Source {
  paneId: string;
  label: string;
  /** Every line of the terminal, scrollback included, oldest first. */
  lines: () => string[];
  /** Scroll to a line (its index in `lines`) and mark it. */
  reveal: (line: number) => void;
}

const sources = new Map<string, Source>();

export function registerTerminal(s: Source): () => void {
  sources.set(s.paneId, s);
  return () => {
    if (sources.get(s.paneId) === s) sources.delete(s.paneId);
  };
}

export const registered = (): Source[] => [...sources.values()];

export interface Hit {
  paneId: string;
  label: string;
  /** Index of the line in the terminal's lines. */
  line: number;
  text: string;
  /** Where the match is in `text`. */
  from: number;
  to: number;
}

export const MAX_HITS = 200;
export const MAX_PATTERN = 300;
/** A line longer than this is cut for display (the match stays in view). */
const EXCERPT = 240;

export interface Options {
  regex?: boolean;
  matchCase?: boolean;
}

/** The pattern as a matcher, null for an empty query, an Error for one that doesn't compile. */
export function compile(query: string, o: Options = {}): RegExp | null | Error {
  if (!query) return null;
  if (query.length > MAX_PATTERN) return new Error("The pattern is too long.");
  try {
    return new RegExp(o.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), o.matchCase ? "" : "i");
  } catch (e) {
    return e instanceof Error ? e : new Error("Not a valid pattern.");
  }
}

/** Cut a long line down around the match. */
function excerpt(text: string, from: number, to: number): { text: string; from: number; to: number } {
  if (text.length <= EXCERPT) return { text, from, to };
  const start = Math.max(0, Math.min(from - 60, text.length - EXCERPT));
  const cut = text.slice(start, start + EXCERPT);
  return { text: (start > 0 ? "…" : "") + cut, from: from - start + (start > 0 ? 1 : 0), to: Math.min(to - start, EXCERPT) + (start > 0 ? 1 : 0) };
}

/** Every line of every source that matches, newest lines first within a terminal, at most MAX_HITS in all. */
export function search(list: Source[], query: string, o: Options = {}): { hits: Hit[]; truncated: boolean; error: string | null } {
  const re = compile(query, o);
  if (re instanceof Error) return { hits: [], truncated: false, error: re.message };
  if (!re) return { hits: [], truncated: false, error: null };
  const hits: Hit[] = [];
  let truncated = false;
  outer: for (const s of list) {
    const lines = s.lines();
    for (let i = lines.length - 1; i >= 0; i--) {
      const m = re.exec(lines[i]);
      if (!m || m[0] === "") continue;
      if (hits.length >= MAX_HITS) {
        truncated = true;
        break outer;
      }
      const e = excerpt(lines[i], m.index, m.index + m[0].length);
      hits.push({ paneId: s.paneId, label: s.label, line: i, ...e });
    }
  }
  return { hits, truncated, error: null };
}
