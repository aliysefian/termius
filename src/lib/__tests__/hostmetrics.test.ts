import { describe, expect, it } from "vitest";
import { formatUptime, parseMetricsOutput } from "../hostmetrics";

describe("parseMetricsOutput", () => {
  it("reads real output captured from running the script locally (Linux, /proc)", () => {
    // Captured by actually running METRICS_SCRIPT with `sh` and `dash` on
    // this machine, not hand-written — the shell logic is real.
    const real = "CPU=51\nMEM=36\nLOAD=4.29 5.50 4.48\nDISK=55\nUPTIME=592914\n";
    expect(parseMetricsOutput(real)).toEqual({
      cpuPct: 51,
      memPct: 36,
      diskPct: 55,
      load: "4.29 5.50 4.48",
      uptimeSecs: 592914,
    });
  });

  it("treats an empty field (the BSD/macOS CPU/MEM fallback) as null, not zero", () => {
    const bsd = "CPU=\nMEM=\nLOAD=0.12 0.08 0.05\nDISK=42\nUPTIME=86400\n";
    const m = parseMetricsOutput(bsd);
    expect(m.cpuPct).toBeNull();
    expect(m.memPct).toBeNull();
    expect(m.diskPct).toBe(42);
    expect(m.load).toBe("0.12 0.08 0.05");
  });

  it("handles CRLF line endings and trailing blank lines", () => {
    const text = "CPU=10\r\nMEM=20\r\nLOAD=0.1 0.1 0.1\r\nDISK=30\r\nUPTIME=100\r\n\r\n";
    expect(parseMetricsOutput(text).cpuPct).toBe(10);
    expect(parseMetricsOutput(text).uptimeSecs).toBe(100);
  });

  it("ignores stray output a shell's own startup files might print", () => {
    const noisy = "Welcome to Ubuntu\nCPU=5\nMEM=10\nLOAD=0.0 0.0 0.0\nDISK=1\nUPTIME=5\n";
    expect(parseMetricsOutput(noisy).cpuPct).toBe(5);
  });

  it("returns all-null fields for unrecognisable output", () => {
    expect(parseMetricsOutput("garbage, not key=value lines")).toEqual({
      cpuPct: null,
      memPct: null,
      diskPct: null,
      load: null,
      uptimeSecs: null,
    });
  });
});

describe("formatUptime", () => {
  it("picks the two most meaningful units", () => {
    expect(formatUptime(45)).toBe("0m");
    expect(formatUptime(90)).toBe("1m");
    expect(formatUptime(3700)).toBe("1h 1m");
    expect(formatUptime(90000)).toBe("1d 1h");
    expect(formatUptime(592914)).toBe("6d 20h");
  });
});
