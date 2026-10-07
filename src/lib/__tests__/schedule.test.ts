import { describe, expect, it } from "vitest";
import { GRACE_MS, describe as describeWhen, isDue, nextRun, parseTime, problem, type Schedule, type When } from "../schedule";

const base = (when: When, o: Partial<Schedule> = {}): Schedule => ({ id: "s", runbookId: "r", params: {}, hostIds: ["h"], when, enabled: true, allowProduction: false, lastRun: 0, ...o });
const at = (y: number, mo: number, d: number, h: number, m = 0) => new Date(y, mo - 1, d, h, m, 0, 0).getTime();

describe("when a schedule is due", () => {
  it("every N minutes counts from the last start", () => {
    const s = base({ kind: "every", minutes: 30 }, { lastRun: at(2026, 10, 7, 10, 0) });
    expect(isDue(s, at(2026, 10, 7, 10, 29))).toBe(false);
    expect(isDue(s, at(2026, 10, 7, 10, 30))).toBe(true);
    expect(isDue(s, at(2026, 10, 7, 14, 0))).toBe(true);
  });

  it("daily fires once, shortly after its time, and not again", () => {
    const w: When = { kind: "daily", at: "02:30" };
    const s = base(w, { lastRun: at(2026, 10, 6, 2, 30) });
    expect(isDue(s, at(2026, 10, 7, 2, 29))).toBe(false);
    expect(isDue(s, at(2026, 10, 7, 2, 30))).toBe(true);
    expect(isDue(s, at(2026, 10, 7, 2, 35))).toBe(true);
    expect(isDue({ ...s, lastRun: at(2026, 10, 7, 2, 31) }, at(2026, 10, 7, 2, 40))).toBe(false);
  });

  it("a time missed while the app was closed is skipped, not caught up", () => {
    const s = base({ kind: "daily", at: "02:30" }, { lastRun: at(2026, 10, 6, 2, 30) });
    expect(isDue(s, at(2026, 10, 7, 2, 30) + GRACE_MS + 1000)).toBe(false);
    expect(isDue(s, at(2026, 10, 7, 9, 0))).toBe(false);
    // …and it fires at the next one.
    expect(isDue(s, at(2026, 10, 8, 2, 31))).toBe(true);
  });

  it("weekly only on its days", () => {
    // 2026-10-07 is a Wednesday (3).
    const s = base({ kind: "weekly", days: [1, 3], at: "08:00" }, { lastRun: at(2026, 10, 5, 8, 0) });
    expect(isDue(s, at(2026, 10, 7, 8, 1))).toBe(true);
    expect(isDue(s, at(2026, 10, 6, 8, 1))).toBe(false);
    expect(isDue({ ...s, when: { kind: "weekly", days: [4], at: "08:00" } }, at(2026, 10, 7, 8, 1))).toBe(false);
  });

  it("a disabled or malformed schedule never fires", () => {
    expect(isDue(base({ kind: "every", minutes: 30 }, { enabled: false }), at(2027, 1, 1, 0))).toBe(false);
    expect(isDue(base({ kind: "daily", at: "25:99" }), at(2027, 1, 1, 0))).toBe(false);
    expect(isDue(base({ kind: "every", minutes: 1 }), at(2027, 1, 1, 0))).toBe(false);
    expect(isDue(base({ kind: "weekly", days: [], at: "08:00" }), at(2027, 1, 1, 8, 1))).toBe(false);
  });

  it("works across midnight and month ends", () => {
    const s = base({ kind: "daily", at: "23:55" }, { lastRun: at(2026, 10, 31, 23, 55) });
    expect(isDue(s, at(2026, 11, 1, 0, 3))).toBe(false);
    expect(isDue(s, at(2026, 11, 1, 23, 56))).toBe(true);
  });
});

describe("what to show", () => {
  it("the next run", () => {
    const now = at(2026, 10, 7, 12, 0);
    expect(nextRun(base({ kind: "daily", at: "02:30" }), now)).toBe(at(2026, 10, 8, 2, 30));
    expect(nextRun(base({ kind: "daily", at: "13:00" }), now)).toBe(at(2026, 10, 7, 13, 0));
    expect(nextRun(base({ kind: "weekly", days: [5], at: "09:00" }), now)).toBe(at(2026, 10, 9, 9, 0));
    expect(nextRun(base({ kind: "every", minutes: 30 }, { lastRun: now - 10 * 60_000 }), now)).toBe(now + 20 * 60_000);
    expect(nextRun(base({ kind: "every", minutes: 30 }, { lastRun: now - 90 * 60_000 }), now)).toBe(now);
    expect(nextRun(base({ kind: "daily", at: "nope" }), now)).toBeNull();
  });

  it("says it in words and checks the timing", () => {
    expect(describeWhen({ kind: "every", minutes: 45 })).toBe("every 45 minutes");
    expect(describeWhen({ kind: "every", minutes: 60 })).toBe("every hour");
    expect(describeWhen({ kind: "every", minutes: 360 })).toBe("every 6 hours");
    expect(describeWhen({ kind: "daily", at: "02:30" })).toBe("every day at 02:30");
    expect(describeWhen({ kind: "weekly", days: [1, 3], at: "08:00" })).toBe("Mon, Wed at 08:00");
    expect(describeWhen({ kind: "weekly", days: [0, 1, 2, 3, 4, 5, 6], at: "08:00" })).toBe("every day at 08:00");
    expect(parseTime("2:05")).toEqual({ h: 2, m: 5 });
    expect(parseTime("24:00")).toBeNull();
    expect(problem({ kind: "every", minutes: 4 })).toBeTruthy();
    expect(problem({ kind: "every", minutes: 5 })).toBeNull();
    expect(problem({ kind: "weekly", days: [7], at: "08:00" })).toBeTruthy();
  });
});
