import { describe, expect, it } from "vitest";
import { BOTTOM_MIN, RIGHT_DEFAULT, RIGHT_MIN, bottomFits, bottomHeight, bottomMax, layoutMode, rightFits, rightMax, rightWidth, switchMode } from "../docklayout";

describe("dock sizes", () => {
  it("never lets a dock take most of the window", () => {
    expect(rightMax(1000)).toBe(400);
    expect(rightMax(3000)).toBe(640);
    expect(rightMax(300)).toBe(RIGHT_MIN);
    expect(bottomMax(800)).toBe(360);
    expect(bottomMax(100)).toBe(BOTTOM_MIN);
  });

  it("makes stored sizes valid for the window", () => {
    expect(rightWidth(undefined, 1600)).toBe(RIGHT_DEFAULT);
    expect(rightWidth(NaN, 1600)).toBe(RIGHT_DEFAULT);
    expect(rightWidth(900, 1000)).toBe(400);
    expect(rightWidth(10, 1600)).toBe(RIGHT_MIN);
    expect(bottomHeight(900, 800)).toBe(360);
    expect(bottomHeight(1, 800)).toBe(BOTTOM_MIN);
  });

  it("drops docks from small windows", () => {
    expect(rightFits(800)).toBe(false);
    expect(rightFits(1200)).toBe(true);
    expect(bottomFits(480)).toBe(false);
    expect(bottomFits(900)).toBe(true);
  });
});

describe("layout mode", () => {
  it("is focus only with both docks closed", () => {
    expect(layoutMode(false, false)).toBe("focus");
    expect(layoutMode(true, false)).toBe("workspace");
    expect(layoutMode(false, true)).toBe("workspace");
  });

  it("closes everything, then restores what was open", () => {
    const off = { right: false, bottom: false };
    const first = switchMode(off, off);
    expect(first.open).toEqual({ right: true, bottom: true });
    const toFocus = switchMode({ right: true, bottom: false }, first.remembered);
    expect(toFocus.open).toEqual(off);
    expect(toFocus.remembered).toEqual({ right: true, bottom: false });
    expect(switchMode(off, toFocus.remembered).open).toEqual({ right: true, bottom: false });
  });
});
