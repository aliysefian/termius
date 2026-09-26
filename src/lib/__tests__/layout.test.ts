import { describe, expect, it } from "vitest";
import { MIN_RATIO, layoutRects, leaf, paneIds, remove, setRatio, split, type LayoutNode } from "$lib/layout";

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
