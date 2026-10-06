// Asking the host for file names and a few fixed lists, without flooding it:
// answers are kept for a few seconds, a burst of keys makes one question (the
// last), a stale answer is dropped, and a host that says no is not asked again.
// The question itself is the app's `completion_lookup` command (see
// src-tauri/src/completion.rs); this only decides whether and when to ask.

export interface Entry {
  name: string;
  dir: boolean;
}

export interface Reply {
  entries: Entry[];
  /** There were more than `entries` holds. */
  truncated: boolean;
}

export type Request =
  | { kind: "dir"; dir: string; prefix: string; limit?: number }
  | { kind: "generator"; id: string; dir?: string };

export interface Options {
  fetch: (req: Request) => Promise<Reply>;
  /** Asked before every question: the pane's "look up remote paths" setting. Off means nothing is sent, ever. */
  enabled: () => boolean;
  /** Wait for typing to settle. */
  delay?: number;
  /** How long an answer is reused. */
  ttl?: number;
  /** How long a failed question is not repeated. */
  backoff?: number;
  now?: () => number;
  sleep?: (ms: number) => Promise<void>;
}

interface Kept {
  at: number;
  prefix: string;
  reply: Reply;
}

export const DEBOUNCE_MS = 150;
export const TTL_MS = 5000;
export const BACKOFF_MS = 10_000;

export class RemoteLookup {
  readonly #o: Required<Options>;
  #gen = 0;
  #refused = false;
  #kept = new Map<string, Kept[]>();
  #failed = new Map<string, number>();
  /** Questions sent; for tests and diagnostics. */
  sent = 0;

  constructor(o: Options) {
    this.#o = {
      delay: DEBOUNCE_MS,
      ttl: TTL_MS,
      backoff: BACKOFF_MS,
      now: Date.now,
      sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      ...o,
    };
  }

  /** The host said it won't open a channel for this; nothing more is asked this session. */
  get refused(): boolean {
    return this.#refused;
  }

  clear() {
    this.#kept.clear();
    this.#failed.clear();
  }

  /** Entries of `dir` (absolute, or starting with ~) that start with `prefix`; null when there is nothing to show (yet). */
  async dir(dir: string, prefix: string): Promise<Reply | null> {
    const key = `dir:${dir}`;
    return this.#ask(key, prefix, { kind: "dir", dir, prefix }, (r) => ({ ...r, entries: r.entries.filter((e) => e.name.startsWith(prefix)) }));
  }

  async generator(id: string, dir?: string): Promise<Reply | null> {
    return this.#ask(`gen:${id}:${dir ?? ""}`, "", { kind: "generator", id, dir }, (r) => r);
  }

  async #ask(key: string, prefix: string, req: Request, narrow: (r: Reply) => Reply): Promise<Reply | null> {
    const mine = ++this.#gen;
    if (!this.#o.enabled() || this.#refused) return null;
    const now = this.#o.now();
    const failedAt = this.#failed.get(key);
    if (failedAt !== undefined && now - failedAt < this.#o.backoff) return null;

    // A listing that wasn't cut answers every longer prefix too.
    const usable = (this.#kept.get(key) ?? []).find((k) => now - k.at < this.#o.ttl && (k.prefix === prefix || (!k.reply.truncated && prefix.startsWith(k.prefix))));
    if (usable) return narrow(usable.reply);

    await this.#o.sleep(this.#o.delay);
    if (mine !== this.#gen || !this.#o.enabled() || this.#refused) return null;
    this.sent++;
    try {
      const reply = await this.#o.fetch(req);
      const list = (this.#kept.get(key) ?? []).filter((k) => k.prefix !== prefix);
      list.push({ at: this.#o.now(), prefix, reply });
      this.#kept.set(key, list.slice(-4));
      return mine === this.#gen ? reply : null;
    } catch (e) {
      if ((e as { code?: string } | null)?.code === "completion_refused") this.#refused = true;
      else this.#failed.set(key, this.#o.now());
      return null;
    }
  }
}

// -- turning the word being typed into a question and the answer into text --------------------

/** Join `rest` onto an absolute folder, resolving . and .. . */
function joinPath(base: string, rest: string): string {
  const out: string[] = [];
  for (const part of `${base}/${rest}`.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") out.pop();
    else out.push(part);
  }
  return "/" + out.join("/");
}

/**
 * The folder to list and the prefix to match for the word `value` (unquoted), or null when it can't be
 * known: a relative path needs the shell's folder, which only shells that report it (OSC 7) give.
 */
export function pathQuestion(value: string, cwd: string | null | undefined): { dir: string; prefix: string; dirText: string } | null {
  const slash = value.lastIndexOf("/");
  const dirText = value.slice(0, slash + 1);
  const prefix = value.slice(slash + 1);
  if (value.startsWith("~") && !(value === "~" || value.startsWith("~/"))) return null; // ~user is not looked up
  if (value === "~") return { dir: "~", prefix: "", dirText: "~/" };
  if (dirText.startsWith("/")) return { dir: joinPath("/", dirText), prefix, dirText };
  if (dirText.startsWith("~/")) return { dir: dirText.replace(/\/+$/, "") || "~", prefix, dirText };
  if (!cwd || !cwd.startsWith("/")) return null;
  return { dir: joinPath(cwd, dirText), prefix, dirText };
}

/** A name as a shell word: special characters get a backslash, so the line means the same name. */
export function escapeName(name: string, first = false): string {
  const e = name.replace(/[\s"'`$\;&|()<>*?[\]{}!#]/g, "\\$&");
  return first && e.startsWith("~") ? "\\" + e : e;
}

/** Folders first, then by name; hidden names only when asked for with a leading dot. */
export function arrange(entries: Entry[], prefix: string, onlyDirs: boolean): Entry[] {
  return entries
    .filter((e) => (prefix.startsWith(".") || !e.name.startsWith(".")) && (!onlyDirs || e.dir))
    .sort((a, b) => Number(b.dir) - Number(a.dir) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}
