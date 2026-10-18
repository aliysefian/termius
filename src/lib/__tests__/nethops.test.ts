import { describe, expect, it } from "vitest";
import { formatCountdown, formatMs, formatUptime, hopHealth, localUrl, shortFingerprint, SLOW_MS } from "$lib/nethops";

describe("hopHealth", () => {
  it("is green when the hop answers promptly", () => {
    expect(hopHealth({ total_ms: 24, segment_ms: 24 }, 0, true)).toBe("up");
    expect(hopHealth({ total_ms: SLOW_MS, segment_ms: 0 }, 0, true)).toBe("up");
  });
  it("is amber when it answers slowly", () => {
    expect(hopHealth({ total_ms: SLOW_MS + 1, segment_ms: 10 }, 0, true)).toBe("slow");
  });
  it("is amber for one missed probe and red for repeated ones", () => {
    expect(hopHealth({ total_ms: null, segment_ms: null }, 1, true)).toBe("slow");
    expect(hopHealth({ total_ms: null, segment_ms: null }, 2, true)).toBe("down");
  });
  it("is red for everything once the session has ended, and grey before the first measurement", () => {
    expect(hopHealth({ total_ms: 5, segment_ms: 5 }, 0, false)).toBe("down");
    expect(hopHealth(undefined, 0, true)).toBe("unknown");
  });
});

describe("formatting", () => {
  it("shows a missing number as a dash", () => {
    expect(formatMs(null)).toBe("–");
    expect(formatMs(undefined)).toBe("–");
    expect(formatMs(24)).toBe("24 ms");
  });
  it.each([
    [0, "0s"],
    [45, "45s"],
    [185, "3m 05s"],
    [7800, "2h 10m"],
    [90_000, "1d 1h"],
    [-5, "0s"],
  ])("uptime %i is %s", (s, text) => expect(formatUptime(s)).toBe(text));
  it.each([
    [272, "4:32"],
    [300, "5:00"],
    [59.2, "1:00"],
    [0, "0:00"],
  ])("a countdown of %f seconds reads %s", (s, text) => expect(formatCountdown(s)).toBe(text));
  it("shortens a long fingerprint and leaves a short one", () => {
    expect(shortFingerprint("SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG")).toBe("SHA256:abcdefgh…BCDEFG");
    expect(shortFingerprint("SHA256:abc")).toBe("SHA256:abc");
    expect(shortFingerprint(null)).toBe("–");
  });
  it("makes a browser address", () => {
    expect(localUrl(8000)).toBe("http://localhost:8000");
  });
});
