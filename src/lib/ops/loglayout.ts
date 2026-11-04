// Drawing only the rows in view when there may be tens of thousands: where each row starts, and which rows a scroll
// position shows. Rows are one line high, except wrapped ones (a few lines) and the one opened for its details.
import type { Row } from "./logrows";

export const ROW_H = 24;
export const WRAP_LINE_H = 18;
/** A wrapped message is drawn over at most this many lines; the details show all of it. */
export const MAX_WRAP_LINES = 6;

export interface LayoutOptions {
  /** Wrap long messages onto more lines. */
  wrap: boolean;
  /** Characters of the message column that fit on a line (from its width); used when wrapping. */
  charsPerLine: number;
  /** The id of the row whose details are open. */
  open: string | null;
  /** Height of the open details. */
  detailH: number;
}

export function rowHeight(r: Row, o: LayoutOptions): number {
  let h = ROW_H;
  if (o.wrap && o.charsPerLine > 8) {
    const lines = Math.min(MAX_WRAP_LINES, Math.max(1, Math.ceil(r.message.length / o.charsPerLine)));
    h = lines === 1 ? ROW_H : 6 + lines * WRAP_LINE_H;
  }
  return r.id === o.open ? h + o.detailH : h;
}

/** `offsets[i]` is where row i starts; the last entry is the total height. */
export function buildOffsets(rows: Row[], o: LayoutOptions): Float64Array {
  const out = new Float64Array(rows.length + 1);
  let y = 0;
  for (let i = 0; i < rows.length; i++) {
    out[i] = y;
    y += rowHeight(rows[i], o);
  }
  out[rows.length] = y;
  return out;
}

/** The rows to draw for a view `height` tall scrolled to `top`: [start, end), with a few extra on each side. */
export function visibleRows(offsets: Float64Array, top: number, height: number, overscan = 8): { start: number; end: number } {
  const n = offsets.length - 1;
  if (n <= 0) return { start: 0, end: 0 };
  // The last row that starts at or before `top`.
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (offsets[mid] <= top) lo = mid;
    else hi = mid - 1;
  }
  let end = lo;
  while (end < n && offsets[end] < top + height) end++;
  return { start: Math.max(0, lo - overscan), end: Math.min(n, end + overscan) };
}

/** The index of the row with `id`, or -1. */
export const indexOfRow = (rows: Row[], id: string | null): number => (id === null ? -1 : rows.findIndex((r) => r.id === id));

/** How many characters of a monospace message fit across `widthPx` at `charPx` each. */
export const charsFor = (widthPx: number, charPx = 7.3): number => Math.max(0, Math.floor(widthPx / charPx));
