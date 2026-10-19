// The `tmux -CC` wire protocol, as a streaming state machine.
//
// `tmux -CC` (iTerm2's mode) opens with a DCS introducer, `ESC P 1000p`, then speaks line by line until
// `%exit`, after which the host terminal gets `ESC \`. Before and after that the bytes belong to the ordinary
// shell, so the parser hands them back untouched (`raw`) for the terminal to show, and keeps every protocol line
// out of the terminal.
//
// Pane output arrives as `%output %<pane> <data>` with every byte below 0x20, and the backslash, written as
// `\ooo` (octal). The rest, UTF-8 included, is plain. Command replies are framed by `%begin` and `%end` (or
// `%error`) and carry a command number, so a client can pair a reply with the command that asked for it.

export type TmuxEvent =
  | { type: "output"; paneId: number; data: Uint8Array }
  | { type: "layout-change"; windowId: number; layout: string; visibleLayout?: string; flags?: string }
  | { type: "window-add"; windowId: number }
  | { type: "window-close"; windowId: number }
  | { type: "window-renamed"; windowId: number; name: string }
  | { type: "window-pane-changed"; windowId: number; paneId: number }
  | { type: "session-changed"; sessionId: number; name: string }
  | { type: "session-renamed"; name: string }
  | { type: "session-window-changed"; sessionId: number; windowId: number }
  | { type: "sessions-changed" }
  | { type: "pane-mode-changed"; paneId: number }
  | { type: "client-detached"; client: string }
  | { type: "exit"; reason?: string }
  | { type: "response"; num: number; ok: boolean; lines: string[]; /** "1" for the reply to a command this client sent, "0" for one tmux ran for itself (the `attach` that started control mode). */ flags: string };

/** `ESC P 1000 p`: what tmux prints when it enters control mode with `-CC`. */
const DCS = [0x1b, 0x50, 0x31, 0x30, 0x30, 0x30, 0x70];
const ESC = 0x1b;
const BACKSLASH = 0x5c;
const NL = 0x0a;
const CR = 0x0d;
const SPACE = 0x20;
const PERCENT = 0x25;
const ZERO = 0x30;

const text = new TextDecoder();

/** `%12`, `@3` or `$0` to its number; `NaN` for anything else. */
function idOf(sigil: string, s: string | undefined): number {
  return s && s[0] === sigil && /^\d+$/.test(s.slice(1)) ? Number(s.slice(1)) : NaN;
}

/** Undo tmux's escaping of pane output: `\ooo` is one byte. A lone backslash stays as it is. */
export function unescapeOutput(src: Uint8Array): Uint8Array {
  let at = src.indexOf(BACKSLASH);
  if (at < 0) return src;
  const out = new Uint8Array(src.length);
  let n = 0;
  let from = 0;
  while (at >= 0) {
    out.set(src.subarray(from, at), n);
    n += at - from;
    const a = src[at + 1];
    const b = src[at + 2];
    const c = src[at + 3];
    const octal = (d: number | undefined) => d !== undefined && d >= ZERO && d <= ZERO + 7;
    if (octal(a) && octal(b) && octal(c)) {
      out[n++] = (((a! - ZERO) << 6) | ((b! - ZERO) << 3) | (c! - ZERO)) & 0xff;
      from = at + 4;
    } else {
      out[n++] = BACKSLASH;
      from = at + 1;
    }
    at = src.indexOf(BACKSLASH, from);
  }
  out.set(src.subarray(from), n);
  n += src.length - from;
  return out.subarray(0, n);
}

function startsWith(line: Uint8Array, word: string): boolean {
  if (line.length < word.length) return false;
  for (let i = 0; i < word.length; i++) if (line[i] !== word.charCodeAt(i)) return false;
  return true;
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  if (a.length === 0) return b;
  const out = new Uint8Array(a.length + b.length);
  out.set(a);
  out.set(b, a.length);
  return out;
}

export interface Fed {
  /** Bytes that are not protocol: the shell before `tmux -CC` started, and after it ended. */
  raw: Uint8Array;
  events: TmuxEvent[];
}

export class TmuxControlModeParser {
  /** True between the DCS introducer and `%exit`. */
  active = false;
  /** A partial line, or the start of what might become the DCS introducer. */
  private held: Uint8Array = new Uint8Array(0);
  /** After `%exit`: the `ESC \` that closes the DCS is still to come. */
  private awaitingTerminator = false;
  /** An open `%begin` block: its command number and the reply lines so far. */
  private block: { num: number; flags: string; lines: string[] } | null = null;

  /** `startActive` for a stream that is control mode from its first byte, as `tmux -C` over a plain exec channel. */
  constructor(startActive = false) {
    this.active = startActive;
  }

  feed(chunk: Uint8Array): Fed {
    let data = concat(this.held, chunk);
    this.held = new Uint8Array(0);
    const events: TmuxEvent[] = [];
    const raw: Uint8Array[] = [];
    while (data.length > 0) {
      if (!this.active) {
        data = this.feedRaw(data, raw);
      } else {
        const nl = data.indexOf(NL);
        if (nl < 0) {
          this.held = data;
          break;
        }
        let end = nl;
        if (end > 0 && data[end - 1] === CR) end--;
        this.line(data.subarray(0, end), events);
        data = data.subarray(nl + 1);
      }
    }
    return { raw: raw.length === 1 ? raw[0] : raw.reduce(concat, new Uint8Array(0)), events };
  }

  /** Outside control mode: pass bytes through, looking for the introducer. Returns what is left to look at. */
  private feedRaw(data: Uint8Array, raw: Uint8Array[]): Uint8Array {
    if (this.awaitingTerminator) {
      // `ESC \` right after %exit; anything else means the terminator never came.
      if (data[0] === ESC && data.length === 1) {
        this.held = data;
        return new Uint8Array(0);
      }
      this.awaitingTerminator = false;
      if (data[0] === ESC && data[1] === BACKSLASH) data = data.subarray(2);
      if (data.length === 0) return data;
    }
    const at = indexOfSeq(data, DCS);
    if (at >= 0) {
      if (at > 0) raw.push(data.subarray(0, at));
      this.active = true;
      return data.subarray(at + DCS.length);
    }
    // Hold back a tail that could be the start of the introducer, completed by the next chunk.
    let keep = 0;
    for (let n = Math.min(DCS.length - 1, data.length); n > 0; n--) {
      if (prefixMatches(data, data.length - n, n)) {
        keep = n;
        break;
      }
    }
    if (keep) this.held = data.subarray(data.length - keep);
    if (data.length > keep) raw.push(data.subarray(0, data.length - keep));
    return new Uint8Array(0);
  }

  private line(line: Uint8Array, events: TmuxEvent[]) {
    if (this.block) {
      const head = startsWith(line, "%end ") ? "end" : startsWith(line, "%error ") ? "error" : null;
      if (head) {
        const f = text.decode(line).split(" ");
        if (Number(f[2]) === this.block.num) {
          events.push({ type: "response", num: this.block.num, ok: head === "end", lines: this.block.lines, flags: this.block.flags });
          this.block = null;
          return;
        }
      }
      this.block.lines.push(text.decode(line));
      return;
    }
    if (line.length === 0 || line[0] !== PERCENT) return;
    if (startsWith(line, "%output ")) return this.output(line, 8, false, events);
    if (startsWith(line, "%extended-output ")) return this.output(line, 17, true, events);

    const s = text.decode(line);
    const sp = s.indexOf(" ");
    const name = sp < 0 ? s : s.slice(0, sp);
    const rest = sp < 0 ? "" : s.slice(sp + 1);
    const f = rest.split(" ");
    switch (name) {
      case "%begin": {
        const num = Number(f[1]);
        this.block = { num: Number.isFinite(num) ? num : -1, flags: f[2] ?? "1", lines: [] };
        return;
      }
      case "%layout-change": {
        const windowId = idOf("@", f[0]);
        if (Number.isNaN(windowId) || !f[1]) return;
        events.push({ type: "layout-change", windowId, layout: f[1], visibleLayout: f[2], flags: f[3] });
        return;
      }
      // tmux 3.x reports a window killed from our own client as "unlinked" (it is unlinked from the session
      // before it is destroyed), so these mean the same here. Windows are asked for again after either.
      case "%window-add":
      case "%unlinked-window-add":
      case "%window-close":
      case "%unlinked-window-close": {
        const windowId = idOf("@", f[0]);
        if (!Number.isNaN(windowId)) events.push({ type: name.endsWith("add") ? "window-add" : "window-close", windowId });
        return;
      }
      case "%window-renamed":
      case "%unlinked-window-renamed": {
        const windowId = idOf("@", f[0]);
        if (!Number.isNaN(windowId)) events.push({ type: "window-renamed", windowId, name: f.slice(1).join(" ") });
        return;
      }
      case "%window-pane-changed": {
        const windowId = idOf("@", f[0]);
        const paneId = idOf("%", f[1]);
        if (!Number.isNaN(windowId) && !Number.isNaN(paneId)) events.push({ type: "window-pane-changed", windowId, paneId });
        return;
      }
      case "%session-changed": {
        const sessionId = idOf("$", f[0]);
        if (!Number.isNaN(sessionId)) events.push({ type: "session-changed", sessionId, name: f.slice(1).join(" ") });
        return;
      }
      case "%session-renamed":
        events.push({ type: "session-renamed", name: rest });
        return;
      case "%session-window-changed": {
        const sessionId = idOf("$", f[0]);
        const windowId = idOf("@", f[1]);
        if (!Number.isNaN(sessionId) && !Number.isNaN(windowId)) events.push({ type: "session-window-changed", sessionId, windowId });
        return;
      }
      case "%sessions-changed":
        events.push({ type: "sessions-changed" });
        return;
      case "%pane-mode-changed": {
        const paneId = idOf("%", f[0]);
        if (!Number.isNaN(paneId)) events.push({ type: "pane-mode-changed", paneId });
        return;
      }
      case "%client-detached":
        events.push({ type: "client-detached", client: rest });
        return;
      case "%exit":
        events.push({ type: "exit", reason: rest || undefined });
        this.active = false;
        this.awaitingTerminator = true;
        this.block = null;
        return;
      default:
        // %pause, %continue, %subscription-changed, … nothing here uses them.
        return;
    }
  }

  private output(line: Uint8Array, from: number, extended: boolean, events: TmuxEvent[]) {
    // `%output %<pane> <data>`; the extended form is `%extended-output %<pane> <age> : <data>`.
    let sp = line.indexOf(SPACE, from);
    if (sp < 0) return;
    const paneId = idOf("%", text.decode(line.subarray(from, sp)));
    if (Number.isNaN(paneId)) return;
    let start = sp + 1;
    if (extended) {
      sp = line.indexOf(SPACE, start);
      if (sp < 0 || line[sp + 1] !== 0x3a /* : */) return;
      start = sp + 3;
    }
    events.push({ type: "output", paneId, data: unescapeOutput(line.subarray(start)) });
  }
}

function indexOfSeq(data: Uint8Array, seq: number[]): number {
  outer: for (let i = data.indexOf(seq[0]); i >= 0 && i + seq.length <= data.length; i = data.indexOf(seq[0], i + 1)) {
    for (let j = 1; j < seq.length; j++) if (data[i + j] !== seq[j]) continue outer;
    return i;
  }
  return -1;
}

/** Whether `data[from .. from+n]` is the first `n` bytes of the introducer. */
function prefixMatches(data: Uint8Array, from: number, n: number): boolean {
  for (let i = 0; i < n; i++) if (data[from + i] !== DCS[i]) return false;
  return true;
}
