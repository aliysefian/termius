// What has been typed at the shell prompt, read from the terminal, or null when
// it can't be known well enough to suggest anything.
//
// The trustworthy source is shell integration: the shell marks where the prompt
// ends (OSC 133 B), so the typed line is whatever is on screen from there to
// the cursor. Without it, the keys we followed (`LineTracker`) are all there
// is, and the answer says so (`reliable: false`).
//
// Nothing here knows about xterm.js beyond the small shapes below, so it runs
// against the real terminal in the app and the headless one in tests.
import type { Osc133, Pos } from "$lib/shellintegration";

export interface BufferLineLike {
  isWrapped: boolean;
  translateToString(trimRight?: boolean, startColumn?: number, endColumn?: number): string;
  /** A cell with something written in it has characters; one nothing was written to has none. */
  getCell(x: number): { getChars(): string } | undefined;
}

export interface TermLike {
  cols: number;
  buffer: {
    active: {
      type: "normal" | "alternate";
      cursorX: number;
      cursorY: number;
      baseY: number;
      getLine(y: number): BufferLineLike | undefined;
    };
  };
  modes: { mouseTrackingMode: "none" | "x10" | "vt200" | "drag" | "any" };
}

/** Where the shell says things stand, from the OSC 133 marks. */
export class InputWatch {
  #seen = false;
  #state: "prompt" | "input" | "running" = "prompt";
  #start: Pos | null = null;
  #cols = 0;

  /** The shell has sent at least one mark, so the marks can be trusted over guessing. */
  get integrated(): boolean {
    return this.#seen;
  }

  /** `input` while the person is typing a command at the prompt. */
  get state() {
    return this.#state;
  }

  /** Where the typed line starts, and the width the terminal had then. */
  get inputStart(): { pos: Pos; cols: number } | null {
    return this.#state === "input" && this.#start ? { pos: this.#start, cols: this.#cols } : null;
  }

  feed(mark: Osc133, cursor: Pos, cols: number) {
    this.#seen = true;
    switch (mark.kind) {
      case "prompt":
        this.#state = "prompt";
        this.#start = null;
        break;
      case "command":
        this.#state = "input";
        this.#start = cursor;
        this.#cols = cols;
        break;
      case "output":
        this.#state = "running";
        this.#start = null;
        break;
      case "end":
        this.#state = "prompt";
        this.#start = null;
        break;
    }
  }

  /** A new connection or a cleared screen: forget everything. */
  reset() {
    this.#seen = false;
    this.#state = "prompt";
    this.#start = null;
  }
}

export interface ShellLine {
  /** What has been typed, up to the cursor. */
  text: string;
  /** Nothing but blanks to the right of the cursor, so a suggestion can follow it. */
  cursorAtEnd: boolean;
  /** Read from the screen between the shell's own marks, rather than guessed from keystrokes. */
  reliable: boolean;
}

/**
 * Whether `row` goes on into the next row. xterm marks a row as wrapped only when the text ran off the
 * edge by itself; zsh and fish place each row of a long line with the cursor, so those rows aren't
 * marked, but the row before is still full to the last column.
 */
function continues(term: TermLike, row: BufferLineLike | undefined, next: BufferLineLike | undefined): boolean {
  if (!row || !next) return false;
  return next.isWrapped || (row.getCell(term.cols - 1)?.getChars() ?? "") !== "";
}

/** A prompt that is asking for a secret, when only the screen can tell us. */
const SECRET_PROMPT = /pass(word|phrase)|passcode|\bpin\b|token|secret|otp|verification code/i;

/**
 * @param typed what `LineTracker` believes was typed on this line, or null once it lost track
 */
export function readLine(term: TermLike, watch: InputWatch, typed: string | null): ShellLine | null {
  const buf = term.buffer.active;
  // A full-screen program or one that tracks the mouse owns the keyboard.
  if (buf.type === "alternate") return null;
  if (term.modes.mouseTrackingMode !== "none") return null;

  const cursorAbs = buf.baseY + buf.cursorY;
  const row = buf.getLine(cursorAbs);
  if (!row) return null;
  // Blanks after the cursor mean the cursor is at the end; anything else (a right-hand prompt,
  // the shell's own grey suggestion) means there is no room for ours.
  const cursorAtEnd = row.translateToString(true, buf.cursorX).trim() === "";

  if (watch.integrated) {
    const start = watch.inputStart;
    // Not at a prompt: a command is running, or the shell hasn't shown one yet.
    if (!start) return null;
    // The window was resized since the prompt: the saved position may have moved with the reflow.
    if (start.cols !== term.cols) return null;
    if (cursorAbs < start.pos.line || (cursorAbs === start.pos.line && buf.cursorX < start.pos.col)) return null;
    let text = "";
    for (let y = start.pos.line; y <= cursorAbs; y++) {
      const line = buf.getLine(y);
      if (!line) return null;
      // Rows after the first must be the same line running on; anything else is a multi-line command
      // with continuation prompts, whose text we won't try to separate from the prompts.
      if (y > start.pos.line && !continues(term, buf.getLine(y - 1), line)) return null;
      text += line.translateToString(false, y === start.pos.line ? start.pos.col : 0, y === cursorAbs ? buf.cursorX : undefined);
    }
    return { text, cursorAtEnd, reliable: true };
  }

  // No shell integration: only the keys we followed.
  if (typed === null) return null;
  // The whole logical line up to the cursor: this row and the rows it wraps from.
  let first = cursorAbs;
  while (first > 0 && continues(term, buf.getLine(first - 1), buf.getLine(first))) first--;
  let logical = "";
  for (let y = first; y <= cursorAbs; y++) {
    const line = buf.getLine(y);
    if (!line) return null;
    logical += line.translateToString(false, 0, y === cursorAbs ? buf.cursorX : undefined);
  }
  // What's on screen has to end with what we think was typed, or the two disagree.
  if (!logical.endsWith(typed)) return null;
  if (SECRET_PROMPT.test(logical.slice(0, logical.length - typed.length))) return null;
  return { text: typed, cursorAtEnd, reliable: false };
}
