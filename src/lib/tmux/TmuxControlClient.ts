// A tmux control-mode session: the commands we send, the replies and notifications that come back, and the
// windows and panes they describe. It knows nothing about SSH or xterm; it is handed a `write` for the bytes
// going to tmux and is fed the bytes coming from it.
import { TmuxControlModeParser, type TmuxEvent } from "./TmuxControlModeParser";
import { parseLayout, type TmuxLayout } from "./tmuxLayoutParser";
import { supportsHexKeys, type TmuxVersion } from "./discovery";

export interface TmuxWindow {
  id: number;
  index: number;
  name: string;
  active: boolean;
  /** What is on screen: the zoomed pane alone, if one is zoomed. Null until tmux has said. */
  layout: TmuxLayout | null;
  activePaneId: number | null;
}

export type ClientEvent =
  | { type: "output"; paneId: number; data: Uint8Array }
  /** The windows, their layout or the active window changed. */
  | { type: "change" }
  | { type: "exit"; reason?: string };

export interface ClientOptions {
  /** Send these characters to tmux (they end in a newline). */
  write: (text: string) => void;
  version: TmuxVersion | null;
  /** The stream is control mode from its first byte (`tmux -C` on a channel with no terminal), with no DCS introducer. */
  plainControl?: boolean;
  /** How long a resize waits for the next one before it is sent. */
  debounceMs?: number;
}

/** Shells: a pane running one of these has nothing in the foreground that closing it would kill. */
export interface PaneSnapshot {
  /** The main screen, scrollback first. */
  lines: string[];
  /** The screen of a full-screen program (vim, htop) that is running, drawn over the main one. */
  alternateLines: string[] | null;
  /** On the visible screen, from 0. */
  cursor: { x: number; y: number };
}

const IDLE_SHELLS = new Set(["sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "ash", "nu"]);

export const isIdleShell = (command: string) => IDLE_SHELLS.has(command.trim().replace(/^-/, ""));

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** A tmux command argument in single quotes, `'` written as `\'` outside them. */
export function quoteArg(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

const NAMED_KEYS: Record<string, string> = {
  "\r": "Enter",
  "\n": "C-j",
  "\t": "Tab",
  "\x7f": "BSpace",
  "\x1b": "Escape",
  "\x1b[A": "Up",
  "\x1b[B": "Down",
  "\x1b[C": "Right",
  "\x1b[D": "Left",
  "\x1bOA": "Up",
  "\x1bOB": "Down",
  "\x1bOC": "Right",
  "\x1bOD": "Left",
  "\x1b[H": "Home",
  "\x1b[F": "End",
  "\x1bOH": "Home",
  "\x1bOF": "End",
  "\x1b[1~": "Home",
  "\x1b[4~": "End",
  "\x1b[2~": "IC",
  "\x1b[3~": "DC",
  "\x1b[5~": "PPage",
  "\x1b[6~": "NPage",
  "\x1b[Z": "BTab",
};
const SEQUENCES = Object.keys(NAMED_KEYS)
  .filter((k) => k.length > 1)
  .sort((a, b) => b.length - a.length);

/**
 * `send-keys` commands that type `data` into a pane. With `hex` (tmux 3.0+) the bytes go through untouched. Older
 * tmux has no `-H`, so text goes as literal text and control keys by name; only the common ones are known.
 */
export function sendKeysCommands(paneId: number, data: Uint8Array, hex: boolean): string[] {
  const target = `send-keys -t %${paneId}`;
  const out: string[] = [];
  if (hex) {
    // Keep every command line short: a paste can be large.
    for (let i = 0; i < data.length; i += 256) {
      const part = Array.from(data.subarray(i, i + 256), (b) => b.toString(16).padStart(2, "0"));
      out.push(`${target} -H ${part.join(" ")}`);
    }
    return out;
  }
  const s = decoder.decode(data);
  let literal = "";
  const flush = () => {
    if (literal) out.push(`${target} -l ${quoteArg(literal)}`);
    literal = "";
  };
  for (let i = 0; i < s.length; ) {
    const seq = SEQUENCES.find((k) => s.startsWith(k, i));
    const one = NAMED_KEYS[s[i]];
    if (seq) {
      flush();
      out.push(`${target} ${NAMED_KEYS[seq]}`);
      i += seq.length;
    } else if (one) {
      flush();
      out.push(`${target} ${one}`);
      i++;
    } else {
      const c = s.charCodeAt(i);
      if (c >= 1 && c <= 26) {
        flush();
        out.push(`${target} C-${String.fromCharCode(c + 96)}`);
      } else if (c >= 32) {
        literal += s[i];
      }
      i++;
    }
  }
  flush();
  return out;
}

interface Pending {
  resolve: (lines: string[]) => void;
  reject: (e: Error) => void;
  /** Called the moment the reply is read, before anything after it in the stream. */
  atReply?: () => void;
}

const WINDOW_FORMAT = "#{window_id}|#{window_index}|#{window_active}|#{window_layout}|#{window_visible_layout}|#{pane_id}|#{window_name}";

export class TmuxControlClient {
  readonly parser: TmuxControlModeParser;
  readonly windows = new Map<number, TmuxWindow>();
  activeWindowId: number | null = null;
  sessionName = "";
  sessionId: number | null = null;
  /** The control session ended (`%exit`), or was disposed. */
  closed = false;
  /** Set once tmux has said which session this client is on and the window list is in. */
  ready = false;

  private pending: Pending[] = [];
  private listeners = new Set<(e: ClientEvent) => void>();
  private timers = new Map<string, ReturnType<typeof setTimeout>>();
  private refreshing = false;
  /** Another ask came in while one was out. */
  private refreshAgain = false;
  private readonly hex: boolean;

  constructor(private readonly opts: ClientOptions) {
    this.parser = new TmuxControlModeParser(opts.plainControl);
    // A tmux that won't say its version is assumed recent: `-CC` already worked to get here.
    this.hex = opts.version === null || supportsHexKeys(opts.version);
  }

  on(listener: (e: ClientEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(e: ClientEvent) {
    for (const l of [...this.listeners]) l(e);
  }

  /** Windows in tmux's order. */
  get windowList(): TmuxWindow[] {
    return [...this.windows.values()].sort((a, b) => a.index - b.index);
  }

  /** Bytes from the SSH channel. Returns what is not protocol, for the host terminal to show. */
  feed(bytes: Uint8Array): Uint8Array {
    const { raw, events } = this.parser.feed(bytes);
    for (const e of events) this.handle(e);
    return raw;
  }

  private handle(e: TmuxEvent) {
    switch (e.type) {
      case "output":
        this.emit({ type: "output", paneId: e.paneId, data: e.data });
        return;
      case "response": {
        // Replies with flags 0 answer commands tmux ran itself (the `attach` that began this); ours come in order.
        if (e.flags === "0") return;
        const p = this.pending.shift();
        if (!p) return;
        p.atReply?.();
        if (e.ok) p.resolve(e.lines);
        else p.reject(new Error(e.lines.join("\n") || "tmux refused the command"));
        return;
      }
      case "layout-change": {
        const w = this.windows.get(e.windowId) ?? this.placeholder(e.windowId);
        const layout = parseLayout(e.visibleLayout || e.layout);
        if (layout) w.layout = layout;
        this.emit({ type: "change" });
        return;
      }
      case "window-add":
      case "window-close":
        if (e.type === "window-close") this.windows.delete(e.windowId);
        void this.refreshWindows();
        return;
      case "window-renamed": {
        const w = this.windows.get(e.windowId);
        if (w) w.name = e.name;
        this.emit({ type: "change" });
        return;
      }
      case "window-pane-changed": {
        const w = this.windows.get(e.windowId);
        if (w) w.activePaneId = e.paneId;
        this.emit({ type: "change" });
        return;
      }
      case "session-window-changed":
        this.setActive(e.windowId);
        return;
      case "session-changed":
        this.sessionId = e.sessionId;
        this.sessionName = e.name;
        void this.refreshWindows().then(() => {
          this.ready = true;
          this.emit({ type: "change" });
        });
        return;
      case "session-renamed":
        this.sessionName = e.name;
        this.emit({ type: "change" });
        return;
      case "exit":
        this.finish(e.reason);
        return;
      default:
        return;
    }
  }

  private placeholder(id: number): TmuxWindow {
    const w: TmuxWindow = { id, index: this.windows.size, name: "", active: false, layout: null, activePaneId: null };
    this.windows.set(id, w);
    return w;
  }

  private setActive(id: number) {
    this.activeWindowId = id;
    for (const w of this.windows.values()) w.active = w.id === id;
    this.emit({ type: "change" });
  }

  /** Ask tmux for the window list. Several asks at once become one more round after the running one. */
  async refreshWindows(): Promise<void> {
    if (this.closed) return;
    if (this.refreshing) {
      this.refreshAgain = true;
      return;
    }
    this.refreshing = true;
    try {
      do {
        this.refreshAgain = false;
        this.apply(await this.send(`list-windows -F ${quoteArg(WINDOW_FORMAT)}`));
      } while (this.refreshAgain);
    } catch {
      // The session ended while asking; `exit` has told everyone.
    } finally {
      this.refreshing = false;
    }
  }

  private apply(lines: string[]) {
    const seen = new Set<number>();
    for (const l of lines) {
      const f = l.split("|");
      if (f.length < 7 || f[0][0] !== "@") continue;
      const id = Number(f[0].slice(1));
      const w = this.windows.get(id) ?? this.placeholder(id);
      w.index = Number(f[1]);
      w.active = f[2] === "1";
      w.layout = parseLayout(f[4] || f[3]) ?? w.layout;
      w.activePaneId = f[5][0] === "%" ? Number(f[5].slice(1)) : w.activePaneId;
      w.name = f.slice(6).join("|");
      if (w.active) this.activeWindowId = id;
      seen.add(id);
    }
    for (const id of [...this.windows.keys()]) if (!seen.has(id)) this.windows.delete(id);
    this.emit({ type: "change" });
  }

  private finish(reason?: string) {
    if (this.closed) return;
    this.closed = true;
    for (const t of this.timers.values()) clearTimeout(t);
    this.timers.clear();
    const gone = this.pending.splice(0);
    for (const p of gone) p.reject(new Error("the tmux session ended"));
    this.emit({ type: "exit", reason });
  }

  /** Stop without waiting for tmux (the SSH channel dropped). */
  dispose() {
    this.finish("connection lost");
    this.listeners.clear();
  }

  /** Send a tmux command; resolves with the lines it printed. */
  send(command: string, atReply?: () => void): Promise<string[]> {
    if (this.closed) return Promise.reject(new Error("the tmux session ended"));
    if (/[\r\n]/.test(command)) return Promise.reject(new Error("a tmux command is one line"));
    return new Promise((resolve, reject) => {
      this.pending.push({ resolve, reject, atReply });
      this.opts.write(command + "\n");
    });
  }

  /** A command whose reply nobody reads. */
  private fire(command: string) {
    this.send(command).catch(() => {});
  }

  // -- the controls the interface offers ----------------------------------------------------------------------

  sendKeys(paneId: number, data: Uint8Array | string) {
    const bytes = typeof data === "string" ? encoder.encode(data) : data;
    for (const c of sendKeysCommands(paneId, bytes, this.hex)) this.fire(c);
  }

  newWindow() {
    this.fire("new-window");
  }

  /** `side`: the new pane sits beside this one (`-h`); `below`: under it (`-v`). */
  splitWindow(paneId: number | null, how: "side" | "below") {
    this.fire(`split-window ${how === "side" ? "-h" : "-v"}${paneId === null ? "" : ` -t %${paneId}`}`);
  }

  killPane(paneId: number) {
    this.fire(`kill-pane -t %${paneId}`);
  }

  killWindow(windowId: number) {
    this.fire(`kill-window -t @${windowId}`);
  }

  selectWindow(windowId: number) {
    this.setActive(windowId);
    this.fire(`select-window -t @${windowId}`);
  }

  selectPane(paneId: number) {
    this.fire(`select-pane -t %${paneId}`);
  }

  detach() {
    this.fire("detach-client");
  }

  /** The program in a pane's foreground, as tmux names it (`vim`, `zsh`). */
  async foregroundCommand(paneId: number): Promise<string> {
    const [line] = await this.send(`display-message -p -t %${paneId} '#{pane_current_command}'`);
    return (line ?? "").trim();
  }

  /** The pane's screen as text with its colours, preceded by up to `history` lines that have scrolled off. */
  capturePane(paneId: number, history = 0): Promise<string[]> {
    return this.send(`capture-pane -p -e -J -t %${paneId}${history > 0 ? ` -S -${Math.floor(history)}` : ""}`);
  }

  /**
   * Everything needed to draw a pane the way it is now: its screen (and `history` lines before it), the cursor,
   * and the full-screen program's screen if one is running. `atReply` is called as the last reply is read; output
   * arriving before it is already in the snapshot, and output after it is not.
   */
  async snapshotPane(paneId: number, history: number, atReply: () => void): Promise<PaneSnapshot> {
    const info = this.send(`display-message -p -t %${paneId} '#{cursor_x} #{cursor_y} #{alternate_on}'`);
    const normal = this.capturePane(paneId, history);
    // `-a` is the alternate screen; `-q` keeps tmux quiet when there is none.
    const alt = this.send(`capture-pane -p -e -a -q -t %${paneId}`, atReply);
    const [x, y, alternate] = ((await info)[0] ?? "").split(" ").map(Number);
    return { lines: await normal, alternateLines: alternate === 1 ? await alt : null, cursor: { x: x || 0, y: y || 0 } };
  }

  /** Tell tmux how big this client is. Waits for the resizing to settle. */
  resizeClient(cols: number, rows: number) {
    this.debounced("client", () => this.fire(`refresh-client -C ${Math.max(2, cols | 0)}x${Math.max(1, rows | 0)}`));
  }

  resizePane(paneId: number, cols: number, rows: number) {
    this.debounced(`pane${paneId}`, () => this.fire(`resize-pane -t %${paneId} -x ${Math.max(1, cols | 0)} -y ${Math.max(1, rows | 0)}`));
  }

  /** One axis only: dragging a divider changes a width or a height, not both. */
  resizePaneAxis(paneId: number, axis: "x" | "y", size: number) {
    this.debounced(`pane${paneId}${axis}`, () => this.fire(`resize-pane -t %${paneId} -${axis} ${Math.max(1, size | 0)}`));
  }

  private debounced(key: string, run: () => void) {
    clearTimeout(this.timers.get(key));
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        if (!this.closed) run();
      }, this.opts.debounceMs ?? 80),
    );
  }
}
