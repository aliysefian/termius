import { describe, expect, it } from "vitest";
import { SIDEBAR_DEFAULT, SIDEBAR_MIN, dragTo, effectiveWidth, maxWidth } from "../sidebarsize";

describe("list panel width", () => {
  it("clamps drags and snaps closed when dragged small", () => {
    expect(dragTo(350.4, 1600)).toEqual({ hide: false, width: 350 });
    expect(dragTo(150, 1600)).toEqual({ hide: false, width: SIDEBAR_MIN });
    expect(dragTo(100, 1600)).toEqual({ hide: true });
    expect(dragTo(5000, 1600)).toEqual({ hide: false, width: 720 });
  });

  it("never takes most of a small window", () => {
    expect(maxWidth(800)).toBe(480);
    expect(maxWidth(200)).toBe(SIDEBAR_MIN);
    expect(effectiveWidth(700, 800)).toBe(480);
    expect(effectiveWidth(undefined, 1600)).toBe(SIDEBAR_DEFAULT);
    expect(effectiveWidth(Number.NaN, 1600)).toBe(SIDEBAR_DEFAULT);
  });
});
