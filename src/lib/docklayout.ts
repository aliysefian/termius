// Size rules for the right and bottom docks around the terminal. The terminal comes first: a dock never takes
// more than a share of the window, and on a small window it is left out of the layout (its state is kept).
export const RIGHT_DEFAULT = 320;
export const RIGHT_MIN = 240;
export const RIGHT_MAX = 640;
export const BOTTOM_DEFAULT = 260;
export const BOTTOM_MIN = 120;
export const BOTTOM_MAX = 600;

/** The terminal never gets less than this much width because of the right dock. */
export const TERMINAL_MIN_WIDTH = 520;
/** The activity bar plus the gutters around the workbench. */
const CHROME_WIDTH = 68 + 12;
/** Below this window height a bottom dock would leave too few terminal rows. */
export const BOTTOM_MIN_WINDOW = 520;

/** Widest the right dock may be: at most 40% of the window. */
export function rightMax(windowWidth: number): number {
  return Math.max(RIGHT_MIN, Math.min(RIGHT_MAX, Math.floor(windowWidth * 0.4)));
}

/** Tallest the bottom dock may be: at most 45% of the window. */
export function bottomMax(windowHeight: number): number {
  return Math.max(BOTTOM_MIN, Math.min(BOTTOM_MAX, Math.floor(windowHeight * 0.45)));
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The most the right dock can take while the terminal keeps its minimum, with `leftUsed` px taken by the list panel. */
function roomRight(windowWidth: number, leftUsed: number): number {
  return windowWidth - leftUsed - CHROME_WIDTH - TERMINAL_MIN_WIDTH;
}

/** A stored (or dragged) width made valid for this window and for what the list panel already takes. */
export function rightWidth(stored: number | undefined, windowWidth: number, leftUsed = 0): number {
  const w = Number.isFinite(stored) ? (stored as number) : RIGHT_DEFAULT;
  const hi = Math.min(rightMax(windowWidth), Math.max(RIGHT_MIN, roomRight(windowWidth, leftUsed)));
  return Math.round(clamp(w, RIGHT_MIN, hi));
}

/** A stored (or dragged) height made valid for this window. */
export function bottomHeight(stored: number | undefined, windowHeight: number): number {
  const h = Number.isFinite(stored) ? (stored as number) : BOTTOM_DEFAULT;
  return Math.round(clamp(h, BOTTOM_MIN, bottomMax(windowHeight)));
}

/** Whether the right dock can show without squeezing the terminal below its minimum. */
export const rightFits = (windowWidth: number, leftUsed = 0) => roomRight(windowWidth, leftUsed) >= RIGHT_MIN;
export const bottomFits = (windowHeight: number) => windowHeight >= BOTTOM_MIN_WINDOW;

/** Which parts of the workbench are showing: the list panel, the right dock and the bottom dock. */
export interface Visibility {
  left: boolean;
  right: boolean;
  bottom: boolean;
}

export const ALL_HIDDEN: Visibility = { left: false, right: false, bottom: false };

/** Focus mode hides every panel. Leaving it brings back exactly what was showing before, not everything. */
export function toggleFocus(focused: boolean, now: Visibility, saved: Visibility): { focused: boolean; show: Visibility; saved: Visibility } {
  if (focused) return { focused: false, show: saved, saved };
  return { focused: true, show: ALL_HIDDEN, saved: now };
}
