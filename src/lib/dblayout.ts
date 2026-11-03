// How the Databases workspace divides its width: the explorer on the left, the editor and results in the middle,
// the inspector on the right. Panels dock beside the editor when there is room to keep it usable, and become
// drawers over it when there isn't. Pure, so the rules are tested.
export const EXPLORER_MIN = 220;
export const EXPLORER_MAX = 420;
export const EXPLORER_DEFAULT = 288;
export const INSPECTOR_MIN = 260;
export const INSPECTOR_MAX = 460;
export const INSPECTOR_DEFAULT = 312;
/** The editor and results never get narrower than this because of a docked panel. */
export const CENTER_MIN = 520;

export const clampWidth = (w: number | undefined, min: number, max: number, def: number) => Math.round(Math.min(max, Math.max(min, Number.isFinite(w) ? (w as number) : def)));

export interface LayoutInput {
  /** The workspace's own width. */
  width: number;
  explorerOpen: boolean;
  inspectorOpen: boolean;
  explorerWidth?: number;
  inspectorWidth?: number;
}

export interface Layout {
  explorerWidth: number;
  inspectorWidth: number;
  /** Beside the editor. */
  explorerDocked: boolean;
  inspectorDocked: boolean;
  /** Open, but over the editor because there isn't room beside it. */
  explorerDrawer: boolean;
  inspectorDrawer: boolean;
}

export function layoutFor(i: LayoutInput): Layout {
  const explorerWidth = clampWidth(i.explorerWidth, EXPLORER_MIN, EXPLORER_MAX, EXPLORER_DEFAULT);
  const inspectorWidth = clampWidth(i.inspectorWidth, INSPECTOR_MIN, INSPECTOR_MAX, INSPECTOR_DEFAULT);
  let explorerDocked = i.explorerOpen;
  let inspectorDocked = i.inspectorOpen;
  // The inspector gives way first, then the explorer.
  const fits = () => i.width - (explorerDocked ? explorerWidth : 0) - (inspectorDocked ? inspectorWidth : 0) >= CENTER_MIN;
  if (!fits()) inspectorDocked = false;
  if (!fits()) explorerDocked = false;
  return {
    explorerWidth,
    inspectorWidth,
    explorerDocked,
    inspectorDocked,
    explorerDrawer: i.explorerOpen && !explorerDocked,
    inspectorDrawer: i.inspectorOpen && !inspectorDocked,
  };
}
