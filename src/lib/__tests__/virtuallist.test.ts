import { describe, expect, it } from "vitest";
import { offsets, rowAt, windowOf } from "../virtuallist";

describe("windowing a long list", () => {
  const heights = [30, 46, 46, 30, 46, 46, 46, 30, 46, 46]; // groups and hosts mixed
  const starts = offsets(heights);

  it("adds the heights up", () => {
    expect(starts[0]).toBe(0);
    expect(starts[1]).toBe(30);
    expect(starts.at(-1)).toBe(heights.reduce((a, b) => a + b, 0));
    expect(offsets([])).toEqual([0]);
  });

  it("finds the row at a position, including the edges and beyond", () => {
    expect(rowAt(starts, 0)).toBe(0);
    expect(rowAt(starts, 29)).toBe(0);
    expect(rowAt(starts, 30)).toBe(1);
    expect(rowAt(starts, 75)).toBe(1);
    expect(rowAt(starts, 76)).toBe(2);
    expect(rowAt(starts, 122)).toBe(3);
    expect(rowAt(starts, 10_000)).toBe(heights.length - 1);
    expect(rowAt(starts, -50)).toBe(0);
    expect(rowAt([0], 5)).toBe(0);
  });

  it("draws the rows in view plus a margin, and says where the first one sits", () => {
    const w = windowOf(starts, 100, 100, 0);
    expect(starts[w.start]).toBeLessThanOrEqual(100);
    expect(starts[w.end]).toBeGreaterThanOrEqual(200);
    expect(w.top).toBe(starts[w.start]);
    expect(w.total).toBe(starts.at(-1));
    const wide = windowOf(starts, 100, 100, 100);
    expect(wide.start).toBeLessThanOrEqual(w.start);
    expect(wide.end).toBeGreaterThanOrEqual(w.end);
  });

  it("is the whole list when it all fits, and the last rows when scrolled past the end", () => {
    expect(windowOf(starts, 0, 10_000)).toMatchObject({ start: 0, end: heights.length });
    const end = windowOf(starts, 100_000, 200, 0);
    expect(end.end).toBe(heights.length);
    expect(end.start).toBeLessThan(end.end);
    expect(windowOf([0], 0, 100)).toEqual({ start: 0, end: 0, top: 0, total: 0 });
  });

  it("draws a bounded number of rows however long the list is", () => {
    const big = offsets(Array.from({ length: 100_000 }, (_, i) => (i % 8 === 0 ? 30 : 46)));
    for (const y of [0, 123_456, 2_000_000, 4_599_000]) {
      const w = windowOf(big, y, 900);
      expect(w.end - w.start).toBeLessThan(60);
      expect(w.start).toBeLessThanOrEqual(rowAt(big, y));
      expect(w.end).toBeGreaterThan(rowAt(big, y + 899));
    }
  });
});
