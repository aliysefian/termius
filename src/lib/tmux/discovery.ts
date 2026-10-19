// What the host's tmux and screen report, parsed from the probe's output (src-tauri/src/multiplexer.rs), and the
// shell commands that start or attach them.
import { shellQuote } from "$lib/cwdsync";

export interface TmuxVersion {
  major: number;
  minor: number;
  /** `tmux master` or `next-3.5`: a build from source, newer than any release we know of. */
  dev: boolean;
  raw: string;
}

export interface TmuxSession {
  /** `$0`. */
  id: string;
  name: string;
  windows: number;
  /** How many clients are attached. */
  attached: number;
  /** Unix seconds. */
  created: number;
  /** Unix seconds; 0 when the tmux is too old to say. */
  lastAttached: number;
  /** One entry per window, for the hover preview. */
  windowList: { index: number; name: string; command: string }[];
}

export interface ScreenSession {
  /** `1234.name`: what `screen -r` takes. */
  id: string;
  name: string;
  state: "attached" | "detached" | "other";
}

export interface MultiplexerProbe {
  tmux: { version: TmuxVersion | null; sessions: TmuxSession[] };
  screen: ScreenSession[];
}

/** `tmux 3.3a`, `tmux next-3.4`, `tmux master`. */
export function parseTmuxVersion(line: string): TmuxVersion | null {
  const t = line.trim();
  if (!/^tmux\b/i.test(t)) return null;
  if (/\bmaster\b/i.test(t)) return { major: Infinity, minor: 0, dev: true, raw: t };
  const m = t.match(/(next-)?(\d+)\.(\d+)/);
  if (!m) return null;
  return { major: Number(m[2]), minor: Number(m[3]), dev: !!m[1], raw: t };
}

const atLeast = (v: TmuxVersion | null, major: number, minor: number) => !!v && (v.major > major || (v.major === major && v.minor >= minor));

/** `-CC` arrived in tmux 1.8. A host that has tmux but won't say which gets the standard attach. */
export const supportsControlMode = (v: TmuxVersion | null) => atLeast(v, 1, 8);
/** `send-keys -H` (hex bytes) arrived in 3.0; older versions need `-l` text and key names. */
export const supportsHexKeys = (v: TmuxVersion | null) => atLeast(v, 3, 0);

function sections(out: string): Map<string, string[]> {
  const map = new Map<string, string[]>();
  let cur: string[] | null = null;
  for (const line of out.split(/\r?\n/)) {
    const h = line.match(/^@@([a-z-]+)$/);
    if (h) {
      cur = [];
      map.set(h[1], cur);
    } else if (cur && line.trim()) cur.push(line);
  }
  return map;
}

const int = (s: string | undefined) => (s && /^\d+$/.test(s) ? Number(s) : 0);

/** `screen -ls`: `\t1234.name\t(Detached)` lines; "No Sockets found" and the footer are skipped. */
export function parseScreenList(lines: string[]): ScreenSession[] {
  const out: ScreenSession[] = [];
  for (const l of lines) {
    const m = l.match(/^\s+(\d+\.\S+)\s+(?:\([^)]*\)\s+)*\((Attached|Detached|Multi|Dead|Private)[^)]*\)/i);
    if (!m) continue;
    const state = /attached|multi/i.test(m[2]) && !/detached/i.test(m[2]) ? "attached" : /detached/i.test(m[2]) ? "detached" : "other";
    // A dead screen can't be resumed; leave it out.
    if (/dead/i.test(m[2])) continue;
    out.push({ id: m[1], name: m[1].replace(/^\d+\./, ""), state });
  }
  return out;
}

export function parseProbe(output: string): MultiplexerProbe {
  const s = sections(output);
  const version = parseTmuxVersion(s.get("tmux-version")?.[0] ?? "");
  const windows = new Map<string, TmuxSession["windowList"]>();
  const lastAttached = new Map<string, number>();
  for (const l of s.get("tmux-windows") ?? []) {
    // Session name first, then index, window name, command, last attached. A `|` in a session name loses its preview.
    const f = l.split("|");
    if (f.length < 5) continue;
    const name = f[0];
    const list = windows.get(name) ?? [];
    list.push({ index: int(f[1]), name: f.slice(2, f.length - 2).join("|"), command: f[f.length - 2] });
    windows.set(name, list);
    lastAttached.set(name, int(f[f.length - 1]));
  }
  const sessions: TmuxSession[] = [];
  for (const l of s.get("tmux-sessions") ?? []) {
    const f = l.split("|");
    if (f.length < 5) continue;
    // `#{session_name}` is free text: whatever lies between the id and the three trailing numbers is the name.
    const name = f.slice(1, f.length - 3).join("|");
    sessions.push({
      id: f[0],
      name,
      windows: int(f[f.length - 3]),
      attached: int(f[f.length - 2]),
      created: int(f[f.length - 1]),
      lastAttached: lastAttached.get(name) ?? 0,
      windowList: windows.get(name) ?? [],
    });
  }
  return { tmux: { version, sessions }, screen: parseScreenList(s.get("screen") ?? []) };
}

/** A session name tmux accepts without surprises: no `.` or `:` (tmux rewrites them), no control characters. */
export function validSessionName(name: string): boolean {
  // eslint-disable-next-line no-control-regex
  return name.length <= 100 && !/[.:\x00-\x1f\x7f]/.test(name);
}

/** What to type into the remote shell. Every name is quoted: it came from the host. */
export const attachCommand = {
  tmuxNative: (name: string) => `tmux -CC -u attach -t ${shellQuote(name)}`,
  tmuxStandard: (name: string) => `tmux attach -t ${shellQuote(name)}`,
  /** An empty name lets tmux number the session itself. */
  newNative: (name: string) => (name ? `tmux -CC -u new -s ${shellQuote(name)}` : "tmux -CC -u new"),
  newStandard: (name: string) => (name ? `tmux new -s ${shellQuote(name)}` : "tmux new"),
  screen: (id: string) => `screen -r ${shellQuote(id)}`,
  newScreen: (name: string) => (name ? `screen -S ${shellQuote(name)}` : "screen"),
};

export type MuxAction = "newWindow" | "nextWindow" | "prevWindow" | "splitRight" | "splitDown" | "detach";

/**
 * Keys for the quick toolbar when native mode isn't possible (tmux < 1.8, or only screen): the prefix (Ctrl+B for
 * tmux, Ctrl+A for screen) and then the key that does the work. Screen's splits need more keystrokes to be useful
 * (focus the new region, start a window in it), so it has none.
 */
const PREFIX_KEYS: Record<"tmux" | "screen", { prefix: string; keys: Partial<Record<MuxAction, string>> }> = {
  tmux: { prefix: "\x02", keys: { newWindow: "c", nextWindow: "n", prevWindow: "p", splitRight: "%", splitDown: '"', detach: "d" } },
  screen: { prefix: "\x01", keys: { newWindow: "c", nextWindow: "n", prevWindow: "p", detach: "d" } },
};

/** The bytes to type for `action`, or null if this multiplexer has no one-key version of it. */
export function prefixSequence(kind: "tmux" | "screen", action: MuxAction): string | null {
  const k = PREFIX_KEYS[kind];
  const key = k.keys[action];
  return key ? k.prefix + key : null;
}
