import { describe, expect, it } from "vitest";
import { DEFAULTS, rangeMs, readPrefs } from "../hostviewprefs";

describe("remembered view choices", () => {
  it("falls back to the defaults for nothing, junk or the wrong shape", () => {
    for (const raw of [null, undefined, "", "not json", "[]", "5", "null"]) expect(readPrefs(raw)).toEqual(DEFAULTS);
  });

  it("keeps good values and repairs bad ones one by one", () => {
    const p = readPrefs(JSON.stringify({ live: 5000, range: "1h", unit: "bits", ifaces: { a: "eth0", b: 5, c: "x".repeat(100) } }));
    expect(p).toEqual({ live: 5000, range: "1h", unit: "bits", ifaces: { a: "eth0" } });
    expect(readPrefs(JSON.stringify({ live: 300, range: "7d", unit: "nibbles" }))).toEqual(DEFAULTS);
    expect(readPrefs(JSON.stringify({ live: 0 })).live).toBe(0);
  });

  it("knows how far each range reaches", () => {
    expect(rangeMs("5m")).toBe(300_000);
    expect(rangeMs("1h")).toBe(3_600_000);
  });
});
