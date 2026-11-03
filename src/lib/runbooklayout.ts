// How the Runbooks workspace divides its space: the library on the left, the editor in the middle, the execution panel on the
// right, and the output console along the bottom. Side panels dock beside the editor when it keeps room to be used, and become
// drawers over it when it wouldn't. The execution panel gives way first, then the library. Pure, so the rules are tested.
export const LIBRARY = { min: 240, max: 360, def: 280 };
export const EXEC = { min: 300, max: 420, def: 340 };
/** The editor is never narrower than this because of a docked panel. */
export const CENTER_MIN = 480;
export const OUTPUT = { min: 120, def: 280 };
/** The editor above the console keeps at least this much height. */
export const EDITOR_MIN_HEIGHT = 220;

export const clamp = (w: number | undefined, r: { min: number; max: number; def: number }) => Math.round(Math.min(r.max, Math.max(r.min, Number.isFinite(w) ? (w as number) : r.def)));

export interface LayoutInput {
  width: number;
  libraryOpen: boolean;
  execOpen: boolean;
  libraryWidth?: number;
  execWidth?: number;
}

export interface Layout {
  libraryWidth: number;
  execWidth: number;
  libraryDocked: boolean;
  execDocked: boolean;
  /** Wanted open, but over the editor because there is no room beside it. */
  libraryDrawer: boolean;
  execDrawer: boolean;
}

export function layoutFor(i: LayoutInput): Layout {
  const libraryWidth = clamp(i.libraryWidth, LIBRARY);
  const execWidth = clamp(i.execWidth, EXEC);
  let libraryDocked = i.libraryOpen;
  let execDocked = i.execOpen;
  const fits = () => i.width - (libraryDocked ? libraryWidth : 0) - (execDocked ? execWidth : 0) >= CENTER_MIN;
  if (!fits()) execDocked = false;
  if (!fits()) libraryDocked = false;
  return { libraryWidth, execWidth, libraryDocked, execDocked, libraryDrawer: i.libraryOpen && !libraryDocked, execDrawer: i.execOpen && !execDocked };
}

/** The console's height for a workspace this tall: at least [`OUTPUT.min`], and leaving the editor its minimum. */
export function outputHeight(stored: number | undefined, workspaceHeight: number): number {
  const max = Math.max(OUTPUT.min, workspaceHeight - EDITOR_MIN_HEIGHT);
  const h = Number.isFinite(stored) ? (stored as number) : OUTPUT.def;
  return Math.round(Math.min(max, Math.max(OUTPUT.min, h)));
}
