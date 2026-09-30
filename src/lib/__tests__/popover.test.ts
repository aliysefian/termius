import { describe, expect, it } from "vitest";
import { EDGE, allGroupPaths, matchesQuery, placeDropdown, placeMenu } from "../popover";

const vp = { width: 1000, height: 800 };

describe("context menu placement", () => {
  it("opens at the pointer when it fits", () => {
    expect(placeMenu(100, 100, 200, 300, vp)).toEqual({ left: 100, top: 100 });
  });

  it("flips up near the bottom so every item stays visible", () => {
    const p = placeMenu(100, 750, 200, 300, vp);
    expect(p.top).toBe(450);
    expect(p.top + 300).toBeLessThanOrEqual(vp.height);
  });

  it("flips left near the right edge", () => {
    expect(placeMenu(950, 100, 200, 300, vp).left).toBe(750);
  });

  it("stays inside when neither direction fits", () => {
    const p = placeMenu(500, 300, 200, 700, vp);
    expect(p.top).toBeGreaterThanOrEqual(EDGE);
    expect(p.top + 700).toBeLessThanOrEqual(vp.height - EDGE);
  });

  it("scrolls only when taller than the window", () => {
    expect(placeMenu(10, 10, 200, 900, vp)).toEqual({ left: 10, top: EDGE, maxHeight: vp.height - EDGE * 2 });
  });
});

describe("dropdown placement", () => {
  const field = { left: 50, top: 100, bottom: 130, width: 300 };

  it("drops below the field when there is room", () => {
    expect(placeDropdown(field, 200, vp)).toMatchObject({ top: 134, maxHeight: 200, left: 50, width: 300 });
  });

  it("opens upward near the bottom of the window", () => {
    const low = { left: 50, top: 700, bottom: 730, width: 300 };
    const p = placeDropdown(low, 280, vp);
    expect(p.top).toBeUndefined();
    expect(p.bottom).toBe(vp.height - 700 + 4);
    expect(p.maxHeight).toBe(280);
  });

  it("shrinks to the larger side when it fits neither", () => {
    // 380px free below, 370px above: below wins and the list scrolls.
    const p = placeDropdown({ left: 0, top: 380, bottom: 410, width: 100 }, 2000, vp);
    expect(p.top).toBe(414);
    expect(p.maxHeight).toBe(800 - 410 - 4 - EDGE);
    const high = placeDropdown({ left: 0, top: 500, bottom: 530, width: 100 }, 2000, vp);
    expect(high.bottom).toBe(304);
    expect(high.maxHeight).toBe(500 - 4 - EDGE);
  });
});

describe("group choices", () => {
  it("includes parents, dedupes and trims", () => {
    expect(allGroupPaths(["Prod/DB", " Prod / Web ", "", undefined, "Dev", "prod/DB"])).toEqual([
      "Dev",
      "Prod",
      "prod",
      "Prod/DB",
      "prod/DB",
      "Prod/Web",
    ]);
  });

  it("matches every word in any field", () => {
    expect(matchesQuery("web eu", "web-1", "10.0.0.1", "eu-west")).toBe(true);
    expect(matchesQuery("web us", "web-1", "eu-west")).toBe(false);
    expect(matchesQuery("  ", "anything")).toBe(true);
  });
});
