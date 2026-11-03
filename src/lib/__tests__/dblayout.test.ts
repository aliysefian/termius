import { describe, expect, it } from "vitest";
import { CENTER_MIN, clampWidth, EXPLORER_DEFAULT, EXPLORER_MAX, EXPLORER_MIN, INSPECTOR_DEFAULT, layoutFor } from "../dblayout";

const base = { explorerOpen: true, inspectorOpen: true };

describe("layoutFor", () => {
  it("docks both panels on a wide window", () => {
    const l = layoutFor({ ...base, width: 1900 });
    expect(l).toMatchObject({ explorerDocked: true, inspectorDocked: true, explorerDrawer: false, inspectorDrawer: false, explorerWidth: EXPLORER_DEFAULT, inspectorWidth: INSPECTOR_DEFAULT });
  });

  it("makes the inspector a drawer first when the editor would get too narrow", () => {
    const l = layoutFor({ ...base, width: 1000 });
    expect(l.explorerDocked).toBe(true);
    expect(l.inspectorDocked).toBe(false);
    expect(l.inspectorDrawer).toBe(true);
    expect(1000 - l.explorerWidth).toBeGreaterThanOrEqual(CENTER_MIN);
  });

  it("makes the explorer a drawer too on a narrow window", () => {
    const l = layoutFor({ ...base, width: 700 });
    expect(l).toMatchObject({ explorerDocked: false, inspectorDocked: false, explorerDrawer: true, inspectorDrawer: true });
  });

  it("leaves a closed panel closed, not a drawer", () => {
    const l = layoutFor({ width: 700, explorerOpen: false, inspectorOpen: false });
    expect(l).toMatchObject({ explorerDrawer: false, inspectorDrawer: false, explorerDocked: false, inspectorDocked: false });
  });

  it("keeps the editor at its minimum or more whenever panels are docked", () => {
    for (let width = 500; width <= 2400; width += 37) {
      const l = layoutFor({ ...base, width, explorerWidth: 420, inspectorWidth: 460 });
      const center = width - (l.explorerDocked ? l.explorerWidth : 0) - (l.inspectorDocked ? l.inspectorWidth : 0);
      if (l.explorerDocked || l.inspectorDocked) expect(center).toBeGreaterThanOrEqual(CENTER_MIN);
    }
  });
});

describe("clampWidth", () => {
  it("keeps widths in range and falls back for junk", () => {
    expect(clampWidth(10, EXPLORER_MIN, EXPLORER_MAX, EXPLORER_DEFAULT)).toBe(EXPLORER_MIN);
    expect(clampWidth(9999, EXPLORER_MIN, EXPLORER_MAX, EXPLORER_DEFAULT)).toBe(EXPLORER_MAX);
    expect(clampWidth(undefined, EXPLORER_MIN, EXPLORER_MAX, EXPLORER_DEFAULT)).toBe(EXPLORER_DEFAULT);
    expect(clampWidth(NaN, EXPLORER_MIN, EXPLORER_MAX, EXPLORER_DEFAULT)).toBe(EXPLORER_DEFAULT);
  });
});
