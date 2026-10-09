import { describe, expect, it } from "vitest";
import { GRACE_MS, describe as describeWhen, isDue, nextRun, parseTime, problem, retryPlan, scheduleProblem, validZone, type Schedule, type When } from "../schedule";

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

const utc = (y: number, mo: number, d: number, h: number, m = 0) => Date.UTC(y, mo - 1, d, h, m);

describe("clock times in a time zone", () => {
  it("reads a daily time on the clock of the chosen zone, including half-hour zones", () => {
    const daily = (at: string, tz: string) => base({ kind: "daily", at }, { tz, lastRun: 0 });
    // 08:00 in Kolkata (UTC+5:30) is 02:30 UTC.
    expect(nextRun(daily("08:00", "Asia/Kolkata"), utc(2026, 10, 7, 0, 0))).toBe(utc(2026, 10, 7, 2, 30));
    // 09:00 in New York is 13:00 UTC in October (EDT) and 14:00 UTC in January (EST).
    expect(nextRun(daily("09:00", "America/New_York"), utc(2026, 10, 7, 0, 0))).toBe(utc(2026, 10, 7, 13, 0));
    expect(nextRun(daily("09:00", "America/New_York"), utc(2027, 1, 15, 0, 0))).toBe(utc(2027, 1, 15, 14, 0));
  });

  it("keeps the wall-clock time across a daylight-saving change", () => {
    const s = base({ kind: "daily", at: "09:00" }, { tz: "America/New_York", lastRun: 0 });
    // Clocks go forward on 2026-03-08 and back on 2026-11-01.
    expect(nextRun(s, utc(2026, 3, 7, 15, 0))).toBe(utc(2026, 3, 8, 13, 0));
    expect(nextRun(s, utc(2026, 3, 8, 14, 0))).toBe(utc(2026, 3, 9, 13, 0));
    expect(nextRun(s, utc(2026, 10, 31, 15, 0))).toBe(utc(2026, 11, 1, 14, 0));
  });

  it("a time that doesn't exist that day still fires once, just after the gap", () => {
    // Berlin skips 02:00-03:00 on 2026-03-29, so 02:30 isn't a real time then.
    const s = base({ kind: "daily", at: "02:30" }, { tz: "Europe/Berlin", lastRun: utc(2026, 3, 28, 1, 30) });
    const next = nextRun(s, utc(2026, 3, 28, 12, 0))!;
    expect(next).toBeGreaterThanOrEqual(utc(2026, 3, 29, 1, 0));
    expect(next).toBeLessThan(utc(2026, 3, 29, 2, 30));
    expect(isDue(s, next + 60_000)).toBe(true);
    expect(isDue({ ...s, lastRun: next }, next + 120_000)).toBe(false);
  });

  it("weekly days are the zone's days, not this computer's", () => {
    // 2026-10-07 23:30 UTC is already Thursday (4) 05:00 in Kolkata.
    const s = base({ kind: "weekly", days: [4], at: "05:00" }, { tz: "Asia/Kolkata", lastRun: utc(2026, 10, 1, 0, 0) });
    expect(isDue(s, utc(2026, 10, 7, 23, 31))).toBe(true);
    expect(isDue({ ...s, tz: "UTC" }, utc(2026, 10, 7, 23, 31))).toBe(false);
  });

  it("knows which zones exist and refuses made-up ones", () => {
    expect(validZone("Europe/Berlin")).toBe(true);
    expect(validZone("Mars/Olympus_Mons")).toBe(false);
    expect(scheduleProblem(base({ kind: "daily", at: "02:30" }, { tz: "Mars/Olympus_Mons" }))).toMatch(/not a time zone/);
    expect(isDue(base({ kind: "daily", at: "02:30" }, { tz: "Mars/Olympus_Mons", lastRun: 0 }), utc(2026, 10, 7, 2, 31))).toBe(false);
  });
});

describe("one-time schedules and missed times", () => {
  const once = (at: number, o: Partial<Schedule> = {}) => base({ kind: "once", at }, { lastRun: at - 3600_000, ...o });

  it("fires once, at its time, and then not again", () => {
    const t = utc(2026, 10, 9, 12, 0);
    expect(isDue(once(t), t - 1)).toBe(false);
    expect(isDue(once(t), t)).toBe(true);
    expect(isDue(once(t, { lastRun: t + 1000 }), t + 2000)).toBe(false);
    expect(nextRun(once(t), t - 60_000)).toBe(t);
    expect(nextRun(once(t, { lastRun: t + 1 }), t + 5)).toBeNull();
    expect(nextRun(once(t, { enabled: false }), t - 60_000)).toBeNull();
  });

  it("a one-time job missed while closed is skipped unless it asked to run once", () => {
    const t = utc(2026, 10, 9, 12, 0);
    const late = t + GRACE_MS + 60_000;
    expect(isDue(once(t), late)).toBe(false);
    expect(isDue(once(t, { missed: "run_once" }), late)).toBe(true);
    expect(isDue(once(t, { missed: "run_once" }), t + 3 * 86_400_000)).toBe(true);
  });

  it("a daily job can ask for the time it missed, once, however many it missed", () => {
    const s = base({ kind: "daily", at: "02:30" }, { tz: "UTC", lastRun: utc(2026, 10, 1, 2, 30) });
    const later = utc(2026, 10, 5, 9, 0);
    expect(isDue(s, later)).toBe(false);
    expect(isDue({ ...s, missed: "run_once" }, later)).toBe(true);
    expect(isDue({ ...s, missed: "run_once", lastRun: utc(2026, 10, 5, 2, 30) }, later)).toBe(false);
  });

  it("describes and validates it", () => {
    expect(describeWhen({ kind: "once", at: utc(2026, 10, 9, 12, 0) })).toMatch(/^once, on /);
    expect(describeWhen({ kind: "daily", at: "02:30" }, "Europe/Berlin")).toBe("every day at 02:30 (Europe/Berlin)");
    expect(problem({ kind: "once", at: NaN })).toBeTruthy();
    expect(problem({ kind: "once", at: utc(2026, 10, 9, 12, 0) })).toBeNull();
  });
});

describe("trying failed hosts again", () => {
  const s = base({ kind: "daily", at: "02:30" }, { retries: 2, retryMinutes: 10 });
  it("retries only the failed hosts, a few times, and not when asked not to", () => {
    expect(retryPlan(s, 0, ["a", "b"], 1000)).toEqual({ hostIds: ["a", "b"], at: 1000 + 10 * 60_000 });
    expect(retryPlan(s, 1, ["b"], 5)).toEqual({ hostIds: ["b"], at: 5 + 10 * 60_000 });
    expect(retryPlan(s, 2, ["b"], 5)).toBeNull();
    expect(retryPlan(s, 0, [], 5)).toBeNull();
    expect(retryPlan(base({ kind: "daily", at: "02:30" }), 0, ["a"], 5)).toBeNull();
    expect(retryPlan({ ...s, retries: 99 }, 3, ["a"], 5)).toBeNull();
    expect(retryPlan({ ...s, retryMinutes: undefined }, 0, ["a"], 0)?.at).toBe(15 * 60_000);
  });
  it("refuses silly retry settings", () => {
    expect(scheduleProblem({ ...s, retries: 4 })).toMatch(/Retries/);
    expect(scheduleProblem({ ...s, retryMinutes: 0 })).toMatch(/Minutes between retries/);
    expect(scheduleProblem(s)).toBeNull();
  });
});
