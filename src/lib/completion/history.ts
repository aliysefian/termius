// The commands typed before, and which one to suggest for what is typed now.
//
// Kept in memory only. Nothing here touches storage: what is saved between
// sessions is the existing "Remember commands" history (settings), which seeds
// this index when it is on. Without it, the index holds this session's
// commands and they are gone when the app closes.
import { fuzzyScore } from "$lib/fuzzy";
import { looksLikeSecret } from "$lib/guard";

export interface Entry {
  command: string;
  at: number;
  /** The exit code, or null when the shell didn't say. */
  exit: number | null;
  /** The folder it ran in, when the shell said (OSC 7). */
  cwd?: string;
  /** The command that ran just before it, for "this usually follows that". */
  prev?: string;
}

export interface Query {
  /** What has been typed so far. */
  text: string;
  /** The host (or "local") the person is typing on. */
  host: string;
  cwd?: string | null;
  /** The command that finished just before this line. */
  prev?: string | null;
}

/** Longest command kept; longer ones are scripts, not something to suggest. */
export const MAX_COMMAND_LENGTH = 500;
export const MAX_PER_HOST = 1000;
export const MAX_TOTAL = 5000;

// Best-effort: a command can hide a secret in more ways than a list can name. Anything that matches
// is never stored and never suggested.
const SENSITIVE: [RegExp, string][] = [
  [/(?<![A-Za-z])(pass(word|wd|phrase)?|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|credentials?|auth)s?\s*[=:]\s*\S/i, "assigns a password, token or key"],
  [/--(password|passwd|passphrase|pass|token|secret|api[_-]?key|access[_-]?key|client[_-]?secret|auth)(=|\s+)\S/i, "passes a password, token or key as an option"],
  [/\b(mysql|mysqldump|mysqladmin|mariadb)\b.*\s-p\S/i, "passes a database password"],
  [/\sssh-?pass\b|\bsshpass\b/i, "uses sshpass"],
  [/\s-u\s+\S+:\S+|--user[= ]\S+:\S+/i, "passes user:password"],
  [/\b(authorization|x-api-key|x-auth-token|proxy-authorization)\s*:/i, "sends an authorization header"],
  [/\bbearer\s+[A-Za-z0-9._~+/=-]{8,}/i, "contains a bearer token"],
  [/:\/\/[^\s/:@]+:[^\s/@]+@/, "has a password in a URL"],
  [/-pass(in|out)?\s+(pass|env|file|fd):/i, "passes an openssl password"],
];

/** Why a command must not be kept or suggested, or null. */
export function sensitiveReason(command: string): string | null {
  const secret = looksLikeSecret(command);
  if (secret) return secret;
  for (const [re, why] of SENSITIVE) if (re.test(command)) return why;
  // A long run of letters and digits that is not a word or a path is almost always a key or token.
  for (const word of command.split(/[\s=:"']+/)) {
    if (word.length >= 32 && /^[A-Za-z0-9_-]+$/.test(word) && /[0-9]/.test(word) && /[A-Za-z]/.test(word)) return "contains a long key-like string";
  }
  return null;
}

export interface Candidate {
  command: string;
  exit?: number | null;
  /** The shell showed it started with a space, its convention for "don't record this". */
  leadingSpace?: boolean;
}

/** Why a command must not enter the history, or null if it may. */
export function rejectReason(c: Candidate): string | null {
  const text = c.command;
  if (c.leadingSpace) return "starts with a space";
  if (!text.trim()) return "empty";
  // A control character (a tab, an escape) would do something other than type when it is accepted.
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1f\x7f]/.test(text)) return "has a control character";
  if (text.length > MAX_COMMAND_LENGTH) return "too long";
  // 126 and 127: the shell could not run it (not executable, not found), so it is not a command to repeat.
  if (c.exit === 126 || c.exit === 127) return "did not start";
  return sensitiveReason(text);
}

export interface Held extends Entry {
  host: string;
}

/** Highest score wins; the more recent breaks a tie. A match on this host beats any match elsewhere. */
function score(e: Held, q: Query): number {
  let s = 0;
  if (e.host === q.host) s += 100;
  if (q.prev && e.prev === q.prev) s += 30;
  if (q.cwd && e.cwd === q.cwd) s += 20;
  if (e.exit === 0) s += 10;
  else if (e.exit === null) s += 5;
  return s;
}

export class HistoryIndex {
  // Map keeps insertion order, so the first key is the oldest and the cap evicts it.
  #hosts = new Map<string, Map<string, Held>>();
  #total = 0;

  constructor(
    private readonly maxPerHost = MAX_PER_HOST,
    private readonly maxTotal = MAX_TOTAL,
  ) {}

  get size() {
    return this.#total;
  }

  /** Keep `entry` for `host` unless it must not be kept. Returns whether it was. */
  add(host: string, entry: Entry & { leadingSpace?: boolean }): boolean {
    if (rejectReason(entry)) return false;
    const map = this.#hosts.get(host) ?? new Map<string, Held>();
    this.#hosts.set(host, map);
    const had = map.delete(entry.command);
    const { leadingSpace: _ignored, ...rest } = entry;
    void _ignored;
    map.set(entry.command, { ...rest, host });
    if (!had) this.#total++;
    // The oldest entries go first, from this host, then from the biggest.
    while (map.size > this.maxPerHost) this.#evict(map);
    while (this.#total > this.maxTotal) {
      let biggest: Map<string, Held> | undefined;
      for (const m of this.#hosts.values()) if (!biggest || m.size > biggest.size) biggest = m;
      if (!biggest || biggest.size === 0) break;
      this.#evict(biggest);
    }
    return true;
  }

  #evict(map: Map<string, Held>) {
    const first = map.keys().next();
    if (first.done) return;
    map.delete(first.value);
    this.#total--;
  }

  /** Load entries kept elsewhere (the saved history), oldest first so the newest end up newest. */
  seed(host: string, entries: Entry[]) {
    for (const e of [...entries].sort((a, b) => a.at - b.at)) this.add(host, e);
  }

  clear(host?: string) {
    if (host === undefined) {
      this.#hosts.clear();
      this.#total = 0;
      return;
    }
    this.#total -= this.#hosts.get(host)?.size ?? 0;
    this.#hosts.delete(host);
  }

  /**
   * Commands that fuzzy-match `text` (all of them for an empty line), best first. For the popup,
   * which can afford to look at everything once per key.
   */
  search(text: string, host: string, limit: number): Held[] {
    const found: { e: Held; s: number }[] = [];
    for (const map of this.#hosts.values()) {
      for (const e of map.values()) {
        const m = fuzzyScore(text, e.command);
        if (m === null) continue;
        // This host beats others by more than any difference in match quality; success and recency break ties.
        found.push({ e, s: m + (e.host === host ? 2000 : 0) + (e.exit === 0 ? 5 : 0) + e.at / 1e13 });
      }
    }
    found.sort((a, b) => b.s - a.s);
    return found.slice(0, limit).map((f) => f.e);
  }

  /** The text that would complete what has been typed, or null. */
  suggest(q: Query): string | null {
    const text = q.text;
    if (!text.trim() || text.includes("\n")) return null;
    let best: Held | null = null;
    let bestScore = -1;
    for (const map of this.#hosts.values()) {
      for (const e of map.values()) {
        const c = e.command;
        if (c.length <= text.length || !c.startsWith(text)) continue;
        const s = score(e, q);
        if (s > bestScore || (s === bestScore && best && e.at > best.at)) {
          // Nothing sensitive is in the index: add() refuses it, and seed() goes through add().
          best = e;
          bestScore = s;
        }
      }
    }
    return best ? best.command.slice(text.length) : null;
  }
}

/** The index the app uses. */
export const completionHistory = new HistoryIndex();

/** Hosts whose own history was read this run; once is enough. */
export const seededHosts = new Set<string>();

/**
 * Commands read from a host's own history files, as entries for `seed`. Oldest first in `lines`; each gets
 * a time a second apart, ending now, so the newest stay newest. Lines that are empty, very long, start
 * with a space (the convention for "don't record") or repeat the one before are skipped; `add` then
 * refuses anything that looks like a secret.
 */
export function entriesFromHistoryLines(lines: string[], now: number): Entry[] {
  const keep: string[] = [];
  for (const raw of lines) {
    const line = raw.replace(/\r$/, "");
    if (!line.trim() || /^\s/.test(line) || line.length > 500 || line === keep[keep.length - 1]) continue;
    keep.push(line.trim());
  }
  return keep.map((command, i) => ({ command, at: now - (keep.length - 1 - i) * 1000, exit: null }));
}
