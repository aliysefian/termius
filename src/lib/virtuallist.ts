// A long list drawn a window at a time. Rows can differ in height (a group's row is shorter than a host's), so the
// window is found from running totals rather than from one row height.

/** Where each row starts, and the total at the end: one more entry than there are rows. */
export function offsets(heights: number[]): number[] {
  const out = new Array<number>(heights.length + 1);
  out[0] = 0;
  for (let i = 0; i < heights.length; i++) out[i + 1] = out[i] + heights[i];
  return out;
}

/** The index of the row that contains `y` (the last row if `y` is past the end), by binary search. */
export function rowAt(starts: number[], y: number): number {
  const n = starts.length - 1;
  if (n <= 0) return 0;
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

export interface Window {
  /** Rows start..end (end excluded) are drawn. */
  start: number;
  end: number;
  /** How far down the first drawn row is. */
  top: number;
  /** The height of the whole list. */
  total: number;
}

/** The rows to draw for a view `viewport` tall scrolled to `scrollTop`, with `overscanPx` more drawn on each side. */
export function windowOf(starts: number[], scrollTop: number, viewport: number, overscanPx = 300): Window {
  const n = starts.length - 1;
  const total = starts[n] ?? 0;
  if (n <= 0) return { start: 0, end: 0, top: 0, total: 0 };
  const from = Math.max(0, scrollTop - overscanPx);
  const to = Math.min(total, scrollTop + viewport + overscanPx);
  const start = rowAt(starts, from);
  let end = rowAt(starts, Math.max(from, to - 1)) + 1;
  end = Math.min(n, Math.max(end, start + 1));
  return { start, end, top: starts[start], total };
}
