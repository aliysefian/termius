import { describe, expect, it, vi } from "vitest";
import { Timeline } from "@/engine/timeline";
import { Player, type Scheduler } from "@/engine/player";
import type { TutorialDefinition } from "@/engine/types";

const def: TutorialDefinition = {
  id: "t",
  initial: { view: "hosts" },
  steps: [
    { type: "moveCursor", target: "a", duration: 1000, chapter: "One", note: "Go to A" },
    { type: "click", target: "a", duration: 400, effect: { panel: "dialog" } },
    { type: "type", target: "field", text: "hello", duration: 1000, chapter: "Two" },
    { type: "type", target: "term", text: "whoami", duration: 600, submit: { terminal: "t1", prompt: "$ " } },
    { type: "terminalOutput", terminal: "t1", lines: ["demo", "ok"], duration: 400 },
    { type: "progress", key: "up", from: 0, to: 100, duration: 1000 },
    { type: "toast", text: "Done", duration: 500 },
    { type: "showPanel", panel: null },
  ],
};

function manualScheduler() {
  let next = 1;
  const pending = new Map<number, (n: number) => void>();
  let now = 0;
  const s: Scheduler & { pending: typeof pending; flush(ms: number): void } = {
    pending,
    request: (cb) => {
      pending.set(next, cb);
      return next++;
    },
    cancel: (h) => void pending.delete(h),
    now: () => now,
    flush(ms) {
      now += ms;
      const cbs = [...pending.values()];
      pending.clear();
      cbs.forEach((cb) => cb(now));
    },
  };
  return s;
}

describe("Timeline", () => {
  const tl = new Timeline(def);

  it("compiles absolute times and chapters", () => {
    expect(tl.duration).toBe(1000 + 400 + 1000 + 600 + 400 + 1000 + 500);
    expect(tl.chapters.map((c) => [c.title, c.start])).toEqual([
      ["One", 0],
      ["Two", 1400],
    ]);
    expect(tl.chapters[1]!.end).toBe(tl.duration);
  });

  it("derives state from time alone, so seeking is deterministic", () => {
    const forward = [0, 500, 1399, 1400, 1900, 2400, 2800, 3300, 4000].map((t) => tl.stateAt(t));
    const backward = [4000, 3300, 2800, 2400, 1900, 1400, 1399, 500, 0].map((t) => tl.stateAt(t)).reverse();
    expect(backward).toEqual(forward);
  });

  it("applies click effects at the click and types progressively", () => {
    expect(tl.stateAt(1399).panel).toBeNull();
    expect(tl.stateAt(1400).panel).toBe("dialog");
    const mid = tl.stateAt(1400 + 600).fields.field!;
    expect("hello".startsWith(mid)).toBe(true);
    expect(mid.length).toBeGreaterThan(0);
    expect(mid.length).toBeLessThan(5);
    expect(tl.stateAt(2400).fields.field).toBe("hello");
  });

  it("commits submitted commands and reveals output", () => {
    const after = tl.stateAt(3000);
    expect(after.terminals.t1).toEqual(["$ whoami"]);
    expect(after.fields.term).toBe("");
    expect(tl.stateAt(3200).terminals.t1).toEqual(["$ whoami", "demo"]);
    expect(tl.stateAt(3400).terminals.t1).toEqual(["$ whoami", "demo", "ok"]);
  });

  it("animates progress and clears the toast afterwards", () => {
    expect(tl.stateAt(3900).progress.up).toBe(50);
    expect(tl.stateAt(4500).toast?.text).toBe("Done");
    expect(tl.stateAt(tl.duration).toast).toBeNull();
    expect(tl.stateAt(tl.duration).progress.up).toBe(100);
  });

  it("keyAt only changes when the render would", () => {
    expect(tl.keyAt(1500)).toBe(tl.keyAt(1510));
    expect(tl.keyAt(1500)).not.toBe(tl.keyAt(1700));
  });

  it("tracks the caption", () => {
    expect(tl.caption(10)).toBe("Go to A");
  });

  it("moves the cursor between resolved targets and presses on click", () => {
    const resolve = (t: string) => (t === "a" ? { x: 0.5, y: 0.5 } : null);
    const mid = tl.cursorAt(500, resolve);
    expect(mid.x).toBeGreaterThan(0.5);
    expect(mid.x).toBeLessThan(0.9);
    const arrived = tl.cursorAt(1000, resolve);
    expect(arrived.x).toBeCloseTo(0.5);
    expect(arrived.y).toBeCloseTo(0.5);
    const click = tl.cursorAt(1050, resolve);
    expect(click.pressed).toBe(true);
    expect(click.clickIndex).toBe(0);
  });

  it("lerps from the previous target, not from the origin", () => {
    const two = new Timeline({
      id: "x",
      initial: {},
      steps: [
        { type: "moveCursor", target: "a", duration: 100 },
        { type: "moveCursor", target: "b", duration: 1000 },
      ],
    });
    const pos: Record<string, { x: number; y: number }> = { a: { x: 0.2, y: 0.2 }, b: { x: 0.8, y: 0.8 } };
    const f = two.cursorAt(100, (t) => pos[t] ?? null);
    expect(f.x).toBeCloseTo(0.2);
    expect(two.cursorAt(1100, (t) => pos[t] ?? null).x).toBeCloseTo(0.8);
  });
});

describe("Player", () => {
  const tl = new Timeline(def);

  it("plays through the scheduler at the chosen speed and ends", () => {
    const s = manualScheduler();
    const p = new Player(tl, { scheduler: s, speed: 2 });
    p.play();
    s.flush(100);
    expect(p.time).toBe(200);
    for (let i = 0; i < 100; i++) s.flush(100);
    expect(p.status).toBe("ended");
    expect(p.time).toBe(tl.duration);
    expect(s.pending.size).toBe(0);
  });

  it("pauses and resumes without losing the position", () => {
    const s = manualScheduler();
    const p = new Player(tl, { scheduler: s });
    p.play();
    s.flush(80);
    p.pause();
    const t = p.time;
    expect(s.pending.size).toBe(0);
    s.flush(500);
    expect(p.time).toBe(t);
    p.play();
    s.flush(50);
    expect(p.time).toBe(t + 50);
  });

  it("restart replays from zero and seek clamps", () => {
    const s = manualScheduler();
    const p = new Player(tl, { scheduler: s });
    p.seek(99999);
    expect(p.time).toBe(tl.duration);
    p.restart();
    expect(p.time).toBe(0);
    expect(p.status).toBe("playing");
    p.seek(-5);
    expect(p.time).toBe(0);
  });

  it("leaves no pending frame after destroy", () => {
    const s = manualScheduler();
    const p = new Player(tl, { scheduler: s });
    p.play();
    expect(s.pending.size).toBe(1);
    p.destroy();
    expect(s.pending.size).toBe(0);
    s.flush(100);
    expect(p.time).toBe(0);
  });

  it("steps and chapters navigate", () => {
    const p = new Player(tl, { scheduler: manualScheduler() });
    p.nextStep();
    expect(p.time).toBe(1000);
    p.nextChapter();
    expect(p.time).toBe(1400);
    p.prevChapter();
    expect(p.time).toBe(0);
  });

  it("loops a range with a hold", () => {
    const s = manualScheduler();
    const p = new Player(tl, { scheduler: s, range: [1400, 2400], loop: { hold: 200 } });
    expect(p.time).toBe(1400);
    p.play();
    p.advance(1000);
    expect(p.time).toBe(2400);
    expect(p.status).toBe("playing");
    p.advance(250);
    expect(p.time).toBe(1400);
  });

  it("notifies subscribers and frame listeners", () => {
    const p = new Player(tl, { scheduler: manualScheduler() });
    const sub = vi.fn();
    const frame = vi.fn();
    p.subscribe(sub);
    p.onFrame(frame);
    p.seek(500);
    expect(sub).toHaveBeenCalledTimes(1);
    expect(frame).toHaveBeenCalledTimes(1);
    expect(p.getSnapshot().time).toBe(500);
  });
});
