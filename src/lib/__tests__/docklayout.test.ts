import { describe, expect, it } from "vitest";
import { BOTTOM_MIN, RIGHT_DEFAULT, RIGHT_MIN, bottomFits, bottomHeight, bottomMax, rightFits, rightMax, rightWidth, toggleFocus } from "../docklayout";

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

  it("drops docks from small windows, counting what the list panel already takes", () => {
    expect(rightFits(800)).toBe(false);
    expect(rightFits(1200)).toBe(true);
    expect(rightFits(1000, 288)).toBe(false);
    expect(rightFits(1000, 0)).toBe(true);
    expect(rightFits(1600, 288)).toBe(true);
    expect(rightWidth(640, 1100, 288)).toBe(RIGHT_MIN);
    expect(rightWidth(320, 1600, 288)).toBe(320);
    expect(bottomFits(480)).toBe(false);
    expect(bottomFits(900)).toBe(true);
  });
});

describe("shell focus", () => {
  const none = { left: false, right: false, bottom: false };

  it("hides every panel and remembers what was showing", () => {
    const now = { left: true, right: false, bottom: true };
    const r = toggleFocus(false, now, { left: true, right: true, bottom: true });
    expect(r).toEqual({ focused: true, show: none, saved: now });
  });

  it("restores exactly what was showing, not everything", () => {
    const saved = { left: true, right: false, bottom: true };
    expect(toggleFocus(true, none, saved)).toEqual({ focused: false, show: saved, saved });
  });
});
