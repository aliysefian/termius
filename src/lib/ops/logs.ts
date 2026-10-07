// Following logs on several hosts: what to run, and turning the output into lines that can be filtered,
// coloured and kept to a fixed size. Pure, so it can be tested without a connection.
import { shq } from "./quote";

export const PRIORITIES = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"] as const;
export type Priority = (typeof PRIORITIES)[number];

export type LogSource =
  | { kind: "journal"; unit?: string; priority?: Priority; kernel?: boolean }
  | { kind: "file"; path: string };

/** A systemd unit name: letters, digits and `:_.@-`, ending in a unit type. */
export const UNIT = /^[A-Za-z0-9:_.@\\-]+\.(service|socket|timer|target|mount|path|slice|scope)$/;

export const MAX_LINES = 5000;
export const MAX_TAIL = 1000;

export function sourceError(src: LogSource): string | null {
  if (src.kind === "journal") {
    if (src.unit && !UNIT.test(src.unit)) return "The unit must look like nginx.service.";
    if (src.priority && !PRIORITIES.includes(src.priority)) return "Unknown priority.";
    return null;
  }
  if (!src.path.trim()) return "Give the path of a log file.";
  if (!src.path.startsWith("/")) return "The path must be absolute (start with /).";
  if (src.path.length > 1024 || src.path.includes("\0")) return "That path isn't usable.";
  return null;
}

/** The script that follows `src`, starting with the last `tail` lines. Throws for a source that doesn't pass `sourceError`. */
export function logScript(src: LogSource, tail: number): string {
  const err = sourceError(src);
  if (err) throw new Error(err);
  const n = Math.min(Math.max(Math.trunc(tail) || 0, 0), MAX_TAIL);
  if (src.kind === "file") return `exec tail -n ${n} -F -- ${shq(src.path)}`;
  const parts = ["exec journalctl", "--follow", `-n ${n}`, "--no-pager", "-o short-iso"];
  if (src.kernel) parts.push("-k");
  if (src.unit) parts.push(`-u ${shq(src.unit)}`);
  if (src.priority) parts.push(`-p ${src.priority}`);
  return parts.join(" ");
}

// -- lines ---------------------------------------------------------------------------------------

export type Level = "error" | "warn" | "info" | "debug";

export interface LogLine {
  /** Counts up per view, so a list can key on it. */
  id: number;
  hostId: string;
  host: string;
  text: string;
  level: Level;
  at: number;
}

// Colour and cursor sequences some programs write into logs; a log view shows text.
// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;

export function cleanLine(raw: string): string {
  return raw.replace(ANSI, "").replace(/\r/g, "");
}

/** Cut a stream of chunks into whole lines, holding back the part of a line that hasn't finished arriving. */
export class LineAssembler {
  #rest = "";

  push(chunk: string): string[] {
    const text = this.#rest + chunk;
    const parts = text.split("\n");
    this.#rest = parts.pop() ?? "";
    // A line that never ends must not grow without bound.
    if (this.#rest.length > 16_384) {
      parts.push(this.#rest);
      this.#rest = "";
    }
    return parts.map(cleanLine);
  }

  /** The stream ended: whatever is left is a line too. */
  finish(): string[] {
    const last = cleanLine(this.#rest);
    this.#rest = "";
    return last ? [last] : [];
  }
}

const ERROR = /\b(emerg|alert|crit(ical)?|fatal|panic|err(or)?s?|exception|traceback|failed|failure|denied|refused|segfault)\b/i;
const WARN = /\b(warn(ing)?s?|deprecated|timeout|timed out|retry(ing)?)\b/i;
const DEBUG = /\b(debug|trace)\b/i;

/** A rough level from the words in a line. Good enough to colour by; never used to decide anything. */
export function detectLevel(text: string): Level {
  if (ERROR.test(text)) return "error";
  if (WARN.test(text)) return "warn";
  if (DEBUG.test(text)) return "debug";
  return "info";
}

// -- filtering -------------------------------------------------------------------------------------

export interface LogFilter {
  text: string;
  regex: boolean;
  /** Levels to show; empty means all. */
  levels: Level[];
  /** Hosts to show; empty means all. */
  hosts: string[];
}

export const NO_FILTER: LogFilter = { text: "", regex: false, levels: [], hosts: [] };

/** A filter's text as a matcher, or null for no text, or an Error for a pattern that doesn't compile. */
export function compileText(f: Pick<LogFilter, "text" | "regex">): RegExp | null | Error {
  const t = f.text;
  if (!t) return null;
  if (t.length > 300) return new Error("The pattern is too long.");
  try {
    return new RegExp(f.regex ? t : t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  } catch (e) {
    return e instanceof Error ? e : new Error("Not a valid pattern.");
  }
}

export function matches(f: LogFilter, line: LogLine, re: RegExp | null): boolean {
  if (f.levels.length && !f.levels.includes(line.level)) return false;
  if (f.hosts.length && !f.hosts.includes(line.hostId)) return false;
  if (re) {
    re.lastIndex = 0;
    return re.test(line.text);
  }
  return true;
}

/** Where the filter's text matched in a line, as [start, end) pairs, for highlighting. */
export function highlights(text: string, re: RegExp | null): [number, number][] {
  if (!re) return [];
  const out: [number, number][] = [];
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) && out.length < 50) {
    if (m[0].length === 0) {
      re.lastIndex++;
      continue;
    }
    out.push([m.index, m.index + m[0].length]);
  }
  return out;
}

/** A fixed-size window onto a stream of lines: the newest `max` are kept. */
export class LogBuffer {
  lines: LogLine[] = [];
  #next = 1;

  constructor(readonly max = MAX_LINES) {}

  add(hostId: string, host: string, texts: string[], at = Date.now()): LogLine[] {
    const added = texts.map((text) => ({ id: this.#next++, hostId, host, text, level: detectLevel(text), at }));
    this.lines = this.lines.concat(added);
    if (this.lines.length > this.max) this.lines = this.lines.slice(this.lines.length - this.max);
    return added;
  }

  clear() {
    this.lines = [];
  }
}
