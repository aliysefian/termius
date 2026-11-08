import { describe, expect, it } from "vitest";
import { describeCron, parseCron, validateCron } from "../admin/cronparse";

describe("checking a schedule", () => {
  it("accepts what cron accepts", () => {
    for (const e of ["* * * * *", "*/5 * * * *", "0 2 * * *", "30 9 1,15 * *", "0 0 * jan-mar mon-fri", "0 22 * * 1-5", "5/10 * * * *", "0 0 * * 7", "@reboot", "@DAILY", "0 8-18/2 * * *", "  15   3  * *  sun "]) expect(validateCron(e), e).toBeNull();
  });

  it("says what is wrong with what it does not accept", () => {
    expect(validateCron("")).toMatch(/Enter a schedule/);
    expect(validateCron("* * * *")).toMatch(/five fields.*has 4/);
    expect(validateCron("60 * * * *")).toMatch(/minute must be from 0 to 59/);
    expect(validateCron("* 24 * * *")).toMatch(/hour must be from 0 to 23/);
    expect(validateCron("* * 0 * *")).toMatch(/day of month must be from 1 to 31/);
    expect(validateCron("* * * 13 *")).toMatch(/month must be from 1 to 12/);
    expect(validateCron("* * * * 8")).toMatch(/day of week must be from 0 to 7/);
    expect(validateCron("*/0 * * * *")).toMatch(/step/);
    expect(validateCron("5-2 * * * *")).toMatch(/backwards/);
    expect(validateCron("a * * * *")).toMatch(/not a valid minute/);
    expect(validateCron("1,,2 * * * *")).toMatch(/empty item/);
    expect(validateCron("@sometimes")).toMatch(/not a schedule macro/);
    expect(validateCron("*/5/2 * * * *")).toMatch(/more than one step/);
  });

  it("reads names, and Sunday as 0 or 7", () => {
    const p = parseCron("0 0 * jan,mar sun,7");
    expect(p.ok && p.fields[3]).toEqual([1, 3]);
    expect(p.ok && p.fields[4]).toEqual([0]);
    const q = parseCron("0 0 * * mon-fri");
    expect(q.ok && q.fields[4]).toEqual([1, 2, 3, 4, 5]);
  });
});

describe("saying a schedule in words", () => {
  const cases: [string, string][] = [
    ["* * * * *", "Every minute"],
    ["*/5 * * * *", "Every 5 minutes"],
    ["0 * * * *", "At the start of every hour"],
    ["15 * * * *", "At minute 15 of every hour"],
    ["30 2 * * *", "At 02:30"],
    ["0 9 * * 1-5", "At 09:00, on Monday to Friday"],
    ["0 0 1 * *", "At 00:00, on the 1st"],
    ["0 0 1,15 * *", "At 00:00, on the 1st and 15th"],
    ["0 0 * * 0", "At 00:00, on Sunday"],
    ["0 0 * * 7", "At 00:00, on Sunday"],
    ["30 6 * jan-mar *", "At 06:30 in January to March"],
    ["0 8,20 * * *", "At 08:00 and 20:00"],
    ["0 */6 * * *", "At minute 0, every 6 hours"],
    ["0 0 13 * 5", "At 00:00, on the 13th or on Friday"],
    ["@reboot", "At every start-up of the host"],
    ["@daily", "Once a day, at 00:00"],
    ["@hourly", "Once an hour, at minute 0"],
  ];
  for (const [e, want] of cases) it(`${e}`, () => expect(describeCron(e)).toBe(want));

  it("says why instead when it is not a schedule", () => {
    expect(describeCron("61 * * * *")).toMatch(/minute must be/);
  });
});
