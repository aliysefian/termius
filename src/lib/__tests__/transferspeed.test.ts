import { describe, expect, it } from "vitest";
import { formatEta, nextSample } from "$lib/transferspeed";

describe("nextSample", () => {
  it("the first event only starts the clock", () => {
    expect(nextSample(undefined, 1000, 5000)).toEqual({ at: 5000, bytes: 1000, bps: 0 });
  });

  it("measures bytes per second over the interval", () => {
    const a = nextSample(undefined, 0, 0);
    expect(nextSample(a, 1_000_000, 1000).bps).toBe(1_000_000);
  });

  it("ignores events closer together than the minimum window", () => {
    const a = nextSample(undefined, 0, 0);
    expect(nextSample(a, 500, 100)).toBe(a);
  });

  it("smooths a change of speed rather than jumping to it", () => {
    let s = nextSample(undefined, 0, 0);
    s = nextSample(s, 1000, 1000); // 1000 B/s
    s = nextSample(s, 11_000, 2000); // 10000 B/s now
    expect(s.bps).toBeGreaterThan(1000);
    expect(s.bps).toBeLessThan(10_000);
  });

  it("starts over when the byte count goes backwards (a retry)", () => {
    const s = nextSample({ at: 0, bytes: 9000, bps: 500 }, 100, 1000);
    expect(s).toEqual({ at: 1000, bytes: 100, bps: 0 });
  });
});

describe("formatEta", () => {
  it("is empty without a speed", () => {
    expect(formatEta(1000, 0)).toBe("");
  });
  it.each([
    [10_000, 1000, "10s"],
    [185_000, 1000, "3m 05s"],
    [3_720_000, 1000, "1h 02m"],
    [10, 1000, "1s"],
  ])("%i bytes at %i B/s is %s", (rem, bps, text) => {
    expect(formatEta(rem, bps)).toBe(text);
  });
});
