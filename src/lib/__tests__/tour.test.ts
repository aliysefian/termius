import { describe, expect, it } from "vitest";
import { STEPS, around, placeCard } from "../tour";
import { LOCALES } from "../i18n/index.svelte";
import { en } from "../i18n/en";

const view = { width: 1200, height: 800 };
const card = { width: 340, height: 200 };

describe("the tour's steps", () => {
  it("have unique ids, and every text exists in every language", () => {
    expect(new Set(STEPS.map((s) => s.id)).size).toBe(STEPS.length);
    for (const s of STEPS) {
      expect(s.title in en && s.body in en, s.id).toBe(true);
      for (const l of LOCALES) expect(l.messages[s.title] ?? en[s.title], `${l.code}.${s.title}`).toBeTruthy();
    }
  });

  it("start with the sidebar and finish with a card that points at nothing", () => {
    expect(STEPS[0].id).toBe("rail");
    expect(STEPS.at(-1)!.targets).toEqual([]);
  });
});

describe("placing the card", () => {
  it("goes to the right of a thing on the left", () => {
    const p = placeCard({ left: 0, top: 100, width: 68, height: 400 }, card, view);
    expect(p.side).toBe("right");
    expect(p.left).toBe(68 + 16);
    expect(p.top).toBe(100);
  });

  it("goes below a thing with no room on its right, and above one at the bottom", () => {
    expect(placeCard({ left: 900, top: 20, width: 280, height: 40 }, card, view).side).toBe("below");
    const above = placeCard({ left: 900, top: 740, width: 280, height: 40 }, card, view);
    expect(above.side).toBe("above");
    expect(above.top).toBe(740 - 16 - 200);
  });

  it("stays inside the window, however tall or low the thing is", () => {
    for (const t of [{ left: 0, top: 700, width: 68, height: 400 }, { left: 1100, top: 700, width: 100, height: 90 }, { left: -50, top: -50, width: 60, height: 60 }]) {
      const p = placeCard(t, card, view);
      expect(p.left).toBeGreaterThanOrEqual(12);
      expect(p.top).toBeGreaterThanOrEqual(12);
      expect(p.left + card.width).toBeLessThanOrEqual(view.width - 12);
      expect(p.top + card.height).toBeLessThanOrEqual(view.height - 12);
    }
  });

  it("is in the middle when there is nothing to point at, and in a tiny window", () => {
    expect(placeCard(null, card, view)).toEqual({ left: 430, top: 300, side: "center" });
    const tiny = placeCard({ left: 10, top: 10, width: 50, height: 50 }, card, { width: 300, height: 250 });
    expect(tiny.left).toBeGreaterThanOrEqual(12);
    expect(tiny.top).toBeGreaterThanOrEqual(12);
  });

  it("makes the spotlight a little larger than what it lights", () => {
    expect(around({ left: 10, top: 20, width: 100, height: 50 })).toEqual({ left: 4, top: 14, width: 112, height: 62 });
  });
});
