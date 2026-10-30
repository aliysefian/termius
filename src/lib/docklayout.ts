// Size rules for the right and bottom docks around the terminal. The terminal comes first: a dock never takes
// more than a share of the window, and on a small window it is left out of the layout (its state is kept).
export const RIGHT_DEFAULT = 320;
export const RIGHT_MIN = 240;
export const RIGHT_MAX = 640;
export const BOTTOM_DEFAULT = 220;
export const BOTTOM_MIN = 120;
export const BOTTOM_MAX = 600;

/** Below this window width a right dock would squeeze the terminal too much to be worth showing. */
export const RIGHT_MIN_WINDOW = 820;
/** Below this window height a bottom dock would leave too few terminal rows. */
export const BOTTOM_MIN_WINDOW = 520;

export type LayoutMode = "focus" | "workspace";

/** Widest the right dock may be: at most 40% of the window. */
export function rightMax(windowWidth: number): number {
  return Math.max(RIGHT_MIN, Math.min(RIGHT_MAX, Math.floor(windowWidth * 0.4)));
}

/** Tallest the bottom dock may be: at most 45% of the window. */
export function bottomMax(windowHeight: number): number {
  return Math.max(BOTTOM_MIN, Math.min(BOTTOM_MAX, Math.floor(windowHeight * 0.45)));
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** A stored (or dragged) width made valid for this window. */
export function rightWidth(stored: number | undefined, windowWidth: number): number {
  const w = Number.isFinite(stored) ? (stored as number) : RIGHT_DEFAULT;
  return Math.round(clamp(w, RIGHT_MIN, rightMax(windowWidth)));
}

/** A stored (or dragged) height made valid for this window. */
export function bottomHeight(stored: number | undefined, windowHeight: number): number {
  const h = Number.isFinite(stored) ? (stored as number) : BOTTOM_DEFAULT;
  return Math.round(clamp(h, BOTTOM_MIN, bottomMax(windowHeight)));
}

export const rightFits = (windowWidth: number) => windowWidth >= RIGHT_MIN_WINDOW;
export const bottomFits = (windowHeight: number) => windowHeight >= BOTTOM_MIN_WINDOW;

/** Shell focus is "no dock open"; any open dock is the workspace. */
export function layoutMode(rightOpen: boolean, bottomOpen: boolean): LayoutMode {
  return rightOpen || bottomOpen ? "workspace" : "focus";
}

export interface DockState {
  right: boolean;
  bottom: boolean;
}

/** Where the mode switch lands: out of focus it restores what was open before (both docks the first time). */
export function switchMode(current: DockState, remembered: DockState): { open: DockState; remembered: DockState } {
  if (layoutMode(current.right, current.bottom) === "workspace") {
    return { open: { right: false, bottom: false }, remembered: current };
  }
  const back = remembered.right || remembered.bottom ? remembered : { right: true, bottom: true };
  return { open: back, remembered: back };
}
