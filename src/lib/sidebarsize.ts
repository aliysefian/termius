// Width rules for the resizable list panel (VS Code-style).
export const SIDEBAR_DEFAULT = 288;
export const SIDEBAR_MIN = 200;
/** Dragging narrower than this closes the panel instead. */
export const SIDEBAR_SNAP = 120;
export const SIDEBAR_MAX = 720;

/** Widest the panel may be in a window this wide: never most of it. */
export function maxWidth(windowWidth: number): number {
  return Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, Math.floor(windowWidth * 0.6)));
}

export type DragResult = { hide: true } | { hide: false; width: number };

/** Where a drag to `raw` pixels lands. */
export function dragTo(raw: number, windowWidth: number): DragResult {
  if (raw < SIDEBAR_SNAP) return { hide: true };
  return { hide: false, width: Math.round(Math.min(maxWidth(windowWidth), Math.max(SIDEBAR_MIN, raw))) };
}

/** A stored width, made valid for the current window. */
export function effectiveWidth(stored: number | undefined, windowWidth: number): number {
  const w = Number.isFinite(stored) ? (stored as number) : SIDEBAR_DEFAULT;
  return Math.min(maxWidth(windowWidth), Math.max(SIDEBAR_MIN, w));
}
