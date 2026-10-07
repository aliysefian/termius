// Highlight rules: colour the words you care about in terminal output, and optionally raise a notice when one
// shows up. Everything here is plain functions; the terminal pane draws the ranges these return.

export interface HighlightRule {
  id: string;
  name: string;
  pattern: string;
  regex: boolean;
  matchCase: boolean;
  /** #rrggbb background for the matched text. */
  color: string;
  enabled: boolean;
  /** Raise a desktop notice (when the app is in the background) when new output matches. */
  notify: boolean;
}

export const MAX_RULES = 30;
export const MAX_PATTERN = 200;
/** Lines longer than this are not scanned; a rule can't make a huge line slow. */
export const MAX_LINE = 2000;

export const COLOR_RE = /^#[0-9a-fA-F]{6}$/;

export const PRESETS: Omit<HighlightRule, "id">[] = [
  { name: "Errors", pattern: "\\b(error|failed|failure|fatal|panic|denied|refused)\\b", regex: true, matchCase: false, color: "#7a2630", enabled: true, notify: false },
  { name: "Warnings", pattern: "\\b(warn(ing)?|deprecated|timeout|timed out)\\b", regex: true, matchCase: false, color: "#6b5416", enabled: true, notify: false },
  { name: "Success", pattern: "\\b(success(ful(ly)?)?|passed|done|ok)\\b", regex: true, matchCase: false, color: "#1f5a37", enabled: true, notify: false },
  { name: "IP addresses", pattern: "\\b\\d{1,3}(\\.\\d{1,3}){3}\\b", regex: true, matchCase: false, color: "#25476e", enabled: true, notify: false },
];

let counter = 0;
export const newId = () => `r${Date.now().toString(36)}${(counter++).toString(36)}`;

export function withId(p: Omit<HighlightRule, "id">): HighlightRule {
  return { id: newId(), ...p };
}

/** What is wrong with a rule, or null when it is usable. */
export function problem(r: Pick<HighlightRule, "pattern" | "regex" | "color">): string | null {
  if (!r.pattern) return "Enter the text to look for.";
  if (r.pattern.length > MAX_PATTERN) return "The pattern is too long.";
  if (!COLOR_RE.test(r.color)) return "Pick a colour.";
  if (r.regex) {
    try {
      const re = new RegExp(r.pattern);
      if (re.test("")) return "The pattern matches nothing in particular; make it more specific.";
    } catch {
      return "That is not a valid pattern.";
    }
  }
  return null;
}

interface Compiled {
  rule: HighlightRule;
  re: RegExp;
}

const cache = new Map<string, Compiled | null>();

function compile(r: HighlightRule): Compiled | null {
  const key = `${r.id}\u0000${r.pattern}\u0000${r.regex}\u0000${r.matchCase}`;
  if (cache.has(key)) {
    const c = cache.get(key) ?? null;
    return c ? { rule: r, re: c.re } : null;
  }
  let out: Compiled | null = null;
  if (!problem(r)) {
    try {
      out = { rule: r, re: new RegExp(r.regex ? r.pattern : r.pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), r.matchCase ? "g" : "gi") };
    } catch {
      out = null;
    }
  }
  if (cache.size > 200) cache.clear();
  cache.set(key, out);
  return out;
}

export interface Range {
  from: number;
  to: number;
  rule: HighlightRule;
}

/** Where the enabled rules match in one line. Earlier rules win where two overlap. */
export function matchLine(rules: HighlightRule[], text: string): Range[] {
  if (!text || text.length > MAX_LINE) return [];
  const taken: Range[] = [];
  for (const r of rules.slice(0, MAX_RULES)) {
    if (!r.enabled) continue;
    const c = compile(r);
    if (!c) continue;
    c.re.lastIndex = 0;
    let m: RegExpExecArray | null;
    let guard = 0;
    while ((m = c.re.exec(text)) && guard++ < 50) {
      if (m[0] === "") {
        c.re.lastIndex++;
        continue;
      }
      const from = m.index;
      const to = m.index + m[0].length;
      if (!taken.some((t) => from < t.to && to > t.from)) taken.push({ from, to, rule: r });
    }
  }
  return taken.sort((a, b) => a.from - b.from);
}

/** The rules that ask for a notice and match this line. */
export function triggered(rules: HighlightRule[], text: string): HighlightRule[] {
  const seen = new Set<string>();
  const out: HighlightRule[] = [];
  for (const m of matchLine(rules.filter((r) => r.notify), text)) {
    if (!seen.has(m.rule.id)) {
      seen.add(m.rule.id);
      out.push(m.rule);
    }
  }
  return out;
}

/** Keep only what is well-formed from saved settings (they could be edited by hand). */
export function sanitize(raw: unknown): HighlightRule[] {
  if (!Array.isArray(raw)) return [];
  const out: HighlightRule[] = [];
  for (const x of raw.slice(0, MAX_RULES)) {
    if (!x || typeof x !== "object") continue;
    const o = x as Record<string, unknown>;
    if (typeof o.pattern !== "string" || typeof o.color !== "string") continue;
    out.push({
      id: typeof o.id === "string" ? o.id : newId(),
      name: typeof o.name === "string" ? o.name.slice(0, 60) : "",
      pattern: o.pattern.slice(0, MAX_PATTERN),
      regex: o.regex === true,
      matchCase: o.matchCase === true,
      color: COLOR_RE.test(o.color) ? o.color : "#444444",
      enabled: o.enabled !== false,
      notify: o.notify === true,
    });
  }
  return out;
}
