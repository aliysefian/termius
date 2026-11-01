import { describe, expect, it } from "vitest";
import { INTER_KNOCK_MAX, INTER_KNOCK_MIN, POST_KNOCK_MAX, delayProblem, emptyKnocking, formatSequence, knockProblems, parseSequence } from "../portknock";

describe("knock sequence text", () => {
  it("defaults to TCP and accepts :udp, /udp and mixed separators", () => {
    expect(parseSequence("7000, 8000:udp, 9000")).toEqual({
      ok: true,
      steps: [{ port: 7000, protocol: "tcp" }, { port: 8000, protocol: "udp" }, { port: 9000, protocol: "tcp" }],
    });
    const r = parseSequence("7000:TCP 8000/udp;9000");
    expect(r.ok && r.steps.map((s) => s.protocol)).toEqual(["tcp", "udp", "tcp"]);
  });

  it("says what is wrong", () => {
    for (const [text, needle] of [["", "at least one"], [" , ", "at least one"], ["0", "out of range"], ["65536", "out of range"], ["-5", "isn't a port"], ["http", "isn't a port"], ["7000:icmp", "tcp or udp"], ["7000:", "tcp or udp"], ["70.5", "isn't a port"]] as const) {
      const r = parseSequence(text);
      expect(r.ok, text).toBe(false);
      expect(!r.ok && r.error, text).toContain(needle);
    }
    expect(parseSequence(Array(17).fill("7000").join(",")).ok).toBe(false);
    expect(parseSequence(Array(16).fill("7000").join(",")).ok).toBe(true);
  });

  it("formats back to the compact form and round-trips", () => {
    const r = parseSequence("7000, 8000:udp,9000/tcp");
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(formatSequence(r.steps)).toBe("7000, 8000:udp, 9000");
      expect(parseSequence(formatSequence(r.steps))).toEqual(r);
    }
  });
});

describe("delays and the whole form", () => {
  it("keeps the delays inside the backend's limits", () => {
    expect(delayProblem(150, INTER_KNOCK_MIN, INTER_KNOCK_MAX, "x")).toBeNull();
    expect(delayProblem(INTER_KNOCK_MIN - 1, INTER_KNOCK_MIN, INTER_KNOCK_MAX, "x")).toContain("10 to 5000");
    expect(delayProblem(INTER_KNOCK_MAX + 1, INTER_KNOCK_MIN, INTER_KNOCK_MAX, "x")).toContain("10 to 5000");
    expect(delayProblem(0, 0, POST_KNOCK_MAX, "x")).toBeNull();
    expect(delayProblem(10_001, 0, POST_KNOCK_MAX, "x")).toContain("0 to 10000");
    expect(delayProblem("", 0, POST_KNOCK_MAX, "x")).toContain("whole number");
    expect(delayProblem(1.5, 0, POST_KNOCK_MAX, "x")).toContain("whole number");
  });

  it("reports nothing while knocking is off, and everything wrong once it is on", () => {
    expect(knockProblems("nonsense", 1, 99999, false)).toEqual([]);
    expect(knockProblems("7000, 8000:udp", 150, 500, true)).toEqual([]);
    expect(knockProblems("", 5, 20000, true)).toHaveLength(3);
  });

  it("has the defaults of the spec", () => {
    expect(emptyKnocking()).toEqual({ enabled: false, sequence: [], inter_knock_delay_ms: 150, post_knock_delay_ms: 500 });
  });
});
