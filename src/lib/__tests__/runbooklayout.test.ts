import { describe, expect, it } from "vitest";
import { CENTER_MIN, clamp, EXEC, LIBRARY, layoutFor, OUTPUT, outputHeight } from "../runbooklayout";

const both = { libraryOpen: true, execOpen: true };

describe("layoutFor", () => {
  it("docks both panels on a wide window at their default widths", () => {
    expect(layoutFor({ ...both, width: 1800 })).toMatchObject({ libraryDocked: true, execDocked: true, libraryWidth: LIBRARY.def, execWidth: EXEC.def, libraryDrawer: false, execDrawer: false });
  });
  it("makes the execution panel a drawer first, then the library", () => {
    const mid = layoutFor({ ...both, width: 900 });
    expect(mid).toMatchObject({ libraryDocked: true, execDocked: false, execDrawer: true, libraryDrawer: false });
    const narrow = layoutFor({ ...both, width: 600 });
    expect(narrow).toMatchObject({ libraryDocked: false, execDocked: false, libraryDrawer: true, execDrawer: true });
  });
  it("leaves a closed panel closed", () => {
    expect(layoutFor({ width: 500, libraryOpen: false, execOpen: false })).toMatchObject({ libraryDrawer: false, execDrawer: false });
  });
  it("always leaves the editor its minimum beside docked panels", () => {
    for (let w = 400; w <= 2400; w += 41) {
      const l = layoutFor({ ...both, width: w, libraryWidth: LIBRARY.max, execWidth: EXEC.max });
      const center = w - (l.libraryDocked ? l.libraryWidth : 0) - (l.execDocked ? l.execWidth : 0);
      if (l.libraryDocked || l.execDocked) expect(center).toBeGreaterThanOrEqual(CENTER_MIN);
    }
  });
  it("keeps stored widths in range", () => {
    expect(clamp(10, LIBRARY)).toBe(LIBRARY.min);
    expect(clamp(5000, EXEC)).toBe(EXEC.max);
    expect(clamp(undefined, EXEC)).toBe(EXEC.def);
    expect(clamp(NaN, LIBRARY)).toBe(LIBRARY.def);
  });
});

describe("outputHeight", () => {
  it("is bounded below and leaves the editor its height", () => {
    expect(outputHeight(undefined, 900)).toBe(OUTPUT.def);
    expect(outputHeight(20, 900)).toBe(OUTPUT.min);
    expect(outputHeight(5000, 900)).toBe(900 - 220);
    expect(outputHeight(300, 300)).toBe(OUTPUT.min);
  });
});
