import { describe, expect, it } from "vitest";
import { Timeline } from "@/engine/timeline";
import { getTutorial, loadTutorial, tutorialIds, tutorials } from "@/tutorials/registry";
import { targetId } from "@/engine/state";

describe("tutorial library", () => {
  for (const id of tutorialIds) {
    describe(id, () => {
      it("compiles into a timeline whose chapters match the written steps", async () => {
        const def = await loadTutorial(id);
        const tl = new Timeline(def);
        expect(tl.duration).toBeGreaterThan(5000);
        const meta = getTutorial(id);
        if (meta) {
          expect(tl.chapters.map((c) => c.title)).toHaveLength(meta.steps.length);
          // Written titles are the chapter titles, in order.
          expect(tl.chapters.map((c) => c.title)).toEqual(meta.steps.map((s) => s.title));
          expect(Math.abs(tl.duration / 1000 - meta.durationSec)).toBeLessThanOrEqual(3);
          expect(tl.duration).toBeGreaterThanOrEqual(30_000);
          expect(tl.duration).toBeLessThanOrEqual(90_000);
        } else {
          expect(tl.duration).toBeLessThanOrEqual(20_000);
        }
      });

      it("replays deterministically at every step boundary", async () => {
        const tl = new Timeline(await loadTutorial(id));
        for (const item of tl.items) {
          const a = tl.stateAt(item.end);
          tl.stateAt(0);
          tl.stateAt(tl.duration);
          expect(tl.stateAt(item.end)).toEqual(a);
        }
      });

      it("only targets ids and fixtures that are fictional", async () => {
        const def = await loadTutorial(id);
        const text = JSON.stringify(def);
        expect(text).not.toMatch(/\b(10|172\.(1[6-9]|2\d|3[01])|192\.168)\.\d+\.\d+/);
        for (const s of def.steps) if ("target" in s && s.target) expect(targetId(s.target)).toMatch(/^[\w.-]+$/);
      });
    });
  }

  it("has a unique id per tutorial and resolvable related links", () => {
    const ids = tutorials.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const t of tutorials) for (const r of t.related) expect(ids).toContain(r);
  });
});
