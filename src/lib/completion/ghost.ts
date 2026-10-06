// Where and what to draw for the faint suggestion after the cursor, kept free
// of the DOM so it can be tested: given measurements, it says what fits.

/** East Asian wide and fullwidth characters, and emoji, take two terminal cells. */
export function charWidth(cp: number): 1 | 2 {
  return (cp >= 0x1100 && cp <= 0x115f) ||
    (cp >= 0x2e80 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe6f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1f64f) ||
    (cp >= 0x1f900 && cp <= 0x1f9ff) ||
    (cp >= 0x20000 && cp <= 0x3fffd)
    ? 2
    : 1;
}

export function displayWidth(s: string): number {
  let w = 0;
  for (const ch of s) w += charWidth(ch.codePointAt(0)!);
  return w;
}

/** The longest start of `s` that fits in `cols` cells. */
export function fitToColumns(s: string, cols: number): string {
  let w = 0;
  let out = "";
  for (const ch of s) {
    const cw = charWidth(ch.codePointAt(0)!);
    if (w + cw > cols) break;
    w += cw;
    out += ch;
  }
  return out;
}

/** One word of a suggestion: any spaces it starts with, then up to the next space. */
export function nextWord(s: string): string {
  return /^\s*\S+/.exec(s)?.[0] ?? s;
}

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface GhostInput {
  /** The terminal's screen element (`.xterm-screen`), and the box the overlay is positioned in. */
  screen: Rect;
  root: Rect;
  cols: number;
  rows: number;
  cursorX: number;
  cursorY: number;
  suggestion: string;
}

export interface Ghost {
  left: number;
  top: number;
  cellWidth: number;
  cellHeight: number;
  /** What fits on the cursor's row; never more than that. */
  text: string;
  /** Cells the text takes, so the box is exactly as wide as the text. */
  cellsWide: number;
}

export interface Cell {
  left: number;
  top: number;
  cellWidth: number;
  cellHeight: number;
}

/** The cursor's cell in the overlay's coordinates, or null for measurements that make no sense. */
export function cursorCell(m: Omit<GhostInput, "suggestion">): Cell | null {
  if (m.cols <= 0 || m.rows <= 0 || m.screen.width <= 0 || m.screen.height <= 0) return null;
  if (m.cursorX < 0 || m.cursorX >= m.cols || m.cursorY < 0 || m.cursorY >= m.rows) return null;
  const cellWidth = m.screen.width / m.cols;
  const cellHeight = m.screen.height / m.rows;
  return {
    left: m.screen.left - m.root.left + m.cursorX * cellWidth,
    top: m.screen.top - m.root.top + m.cursorY * cellHeight,
    cellWidth,
    cellHeight,
  };
}

/** Null when there's nothing to draw: no room left on the row, or measurements that make no sense. */
export function ghostBox(m: GhostInput): Ghost | null {
  const cell = cursorCell(m);
  if (!cell) return null;
  // The suggestion stays on the cursor's row, so it can't cover the line below it.
  const text = fitToColumns(m.suggestion, m.cols - m.cursorX);
  if (!text) return null;
  return { ...cell, text, cellsWide: displayWidth(text) };
}
