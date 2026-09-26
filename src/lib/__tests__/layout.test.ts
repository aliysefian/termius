import { describe, expect, it } from "vitest";
import { MIN_RATIO, grid, layoutRects, leaf, paneIds, remove, setRatio, split, type LayoutNode } from "$lib/layout";

describe("pane layout", () => {
  it("splits, lists and removes panes", () => {
    let t: LayoutNode = leaf("a");
    t = split(t, "a", "b", "row"); // a | b
    t = split(t, "b", "c", "column"); // a | (b / c)
    expect(paneIds(t)).toEqual(["a", "b", "c"]);

    const r = layoutRects(t);
    expect(r.panes.get("a")).toEqual({ x: 0, y: 0, w: 50, h: 100 });
    expect(r.panes.get("b")).toEqual({ x: 50, y: 0, w: 50, h: 50 });
    expect(r.panes.get("c")).toEqual({ x: 50, y: 50, w: 50, h: 50 });
    expect(r.dividers).toHaveLength(2);

    // Closing b lets c take the whole right half.
    const t2 = remove(t, "b")!;
    expect(paneIds(t2)).toEqual(["a", "c"]);
    expect(layoutRects(t2).panes.get("c")).toEqual({ x: 50, y: 0, w: 50, h: 100 });
    expect(remove(leaf("x"), "x")).toBeNull();
    expect(remove(t, "missing")).toEqual(t);
  });

  it("resizes with clamping and leaves other splits alone", () => {
    let t: LayoutNode = split(leaf("a"), "a", "b", "row");
    const id = t.type === "split" ? t.id : "";
    t = setRatio(t, id, 0.7);
    expect(layoutRects(t).panes.get("a")!.w).toBeCloseTo(70);
    t = setRatio(t, id, 0.01);
    expect(t.type === "split" && t.ratio).toBe(MIN_RATIO);
    expect(setRatio(t, "other", 0.9)).toEqual(t);
  });
});

describe("grid", () => {
  it("tiles panes evenly in rows", () => {
    const r4 = layoutRects(grid(["a", "b", "c", "d"])).panes;
    expect(r4.get("a")).toEqual({ x: 0, y: 0, w: 50, h: 50 });
    expect(r4.get("d")).toEqual({ x: 50, y: 50, w: 50, h: 50 });

    const r5 = layoutRects(grid(["a", "b", "c", "d", "e"])).panes;
    // Three across the top, two across the bottom, every row full width.
    for (const id of ["a", "b", "c"]) expect(r5.get(id)!.h).toBeCloseTo(50);
    expect(r5.get("a")!.w).toBeCloseTo(100 / 3);
    expect(r5.get("d")!.w).toBeCloseTo(50);
    expect(r5.get("e")).toEqual({ x: 50, y: 50, w: 50, h: 50 });

    expect(paneIds(grid(["x"]))).toEqual(["x"]);
    expect(paneIds(grid(["1", "2", "3", "4", "5", "6"]))).toEqual(["1", "2", "3", "4", "5", "6"]);
  });
});
