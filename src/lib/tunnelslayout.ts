// Width of the Tunnels page's details panel: remembered, but never more than a share of the page, so the list
// keeps room for its columns.
export const DETAIL_MIN = 320;
export const DETAIL_MAX = 640;
export const DETAIL_DEFAULT = 440;

/** A stored (or dragged) width made valid for a page this wide: at most 45% of it. */
export function detailWidth(stored: number | undefined, pageWidth: number): number {
  const w = Number.isFinite(stored) ? (stored as number) : DETAIL_DEFAULT;
  const hi = Math.max(DETAIL_MIN, Math.min(DETAIL_MAX, Math.floor(pageWidth * 0.45)));
  return Math.round(Math.min(hi, Math.max(DETAIL_MIN, w)));
}
