import { describe, expect, it } from "vitest";
import { entryOutcome, formatDuration, outcomeParts } from "../activitytext";

const at = (secs: number) => ({ startedAt: 1_000_000, endedAt: 1_000_000 + secs * 1000 });

describe("connection log outcome", () => {
  it("separates every part with a spaced dot", () => {
    expect(entryOutcome({ ...at(1541), exitCode: null, reason: "dropped" })).toBe("· 25m 41s · dropped");
    expect(entryOutcome({ ...at(3), exitCode: null, reason: "failed" })).toBe("· 3s · failed");
    expect(entryOutcome({ ...at(1084), exitCode: 0, reason: "exited" })).toBe("· 18m 4s · exit 0");
  });

  it("leaves out reasons that say nothing", () => {
    expect(entryOutcome({ ...at(3720), exitCode: null, reason: "closed" })).toBe("· 1h 2m");
  });

  it("says connected while the session lasts", () => {
    expect(entryOutcome({ startedAt: 1, endedAt: null, exitCode: null, reason: null })).toBe("· connected");
  });

  it("formats durations", () => {
    expect(formatDuration(59_000)).toBe("59s");
    expect(formatDuration(61_000)).toBe("1m 1s");
    expect(formatDuration(3_660_000)).toBe("1h 1m");
  });

  it("lets the host card use its own shorter durations", () => {
    const minutes = (ms: number) => `${Math.round(ms / 60000)}m`;
    expect(outcomeParts({ ...at(1084), exitCode: 0, reason: "exited" }, minutes).join(" · ")).toBe("18m · exit 0");
    expect(outcomeParts({ startedAt: 1, endedAt: null, exitCode: null, reason: null })).toEqual([]);
  });
});
