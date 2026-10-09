import { describe, expect, it } from "vitest";
// The app's type setup has no Node types; vitest runs on Node, so these built-ins are there at run time.
// @ts-expect-error Node built-in
import { execFileSync } from "node:child_process";
// @ts-expect-error Node built-in
import { platform } from "node:os";
import { METRICS_SCRIPT, formatUptime, parseMetricsOutput } from "../hostmetrics";

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
      failedUnits: null,
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
      failedUnits: null,
    });
  });

  it("reads failed systemd units, tells none from not available, and ignores odd names", () => {
    const base = "CPU=1\nMEM=2\nLOAD=0 0 0\nDISK=3\nUPTIME=4\n";
    expect(parseMetricsOutput(base + "SYSTEMD=1\nFAILED=nginx.service,cron.service\n").failedUnits).toEqual(["nginx.service", "cron.service"]);
    expect(parseMetricsOutput(base + "SYSTEMD=1\nFAILED=\n").failedUnits).toEqual([]);
    expect(parseMetricsOutput(base + "SYSTEMD=\nFAILED=\n").failedUnits).toBeNull();
    expect(parseMetricsOutput(base).failedUnits).toBeNull();
    expect(parseMetricsOutput(base + "SYSTEMD=1\nFAILED=a.service,$(rm -rf /),b b,c@x.service\n").failedUnits).toEqual(["a.service", "c@x.service"]);
  });

  // Runs the real script with the system shell (Linux only: it samples /proc for a second).
  it.runIf(platform() === "linux")("the script itself prints every field, whatever this machine runs", () => {
    const out = execFileSync("sh", ["-c", METRICS_SCRIPT], { encoding: "utf8", timeout: 20_000 });
    const keys = out.split("\n").map((l: string) => l.split("=")[0]).filter(Boolean);
    expect(keys).toEqual(["CPU", "MEM", "LOAD", "DISK", "UPTIME", "SYSTEMD", "FAILED"]);
    const m = parseMetricsOutput(out);
    expect(m.cpuPct).not.toBeNull();
    expect(m.uptimeSecs).toBeGreaterThan(0);
    // null on a machine without systemd, an array (maybe empty) on one with it.
    expect(m.failedUnits === null || Array.isArray(m.failedUnits)).toBe(true);
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
