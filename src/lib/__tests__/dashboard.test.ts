import { describe, expect, it } from "vitest";
import {
  HIGH_FROM,
  LEVEL_FILL,
  MODERATE_FROM,
  activeTunnels,
  activityRows,
  byLabel,
  clampPct,
  countOnline,
  elapsedShort,
  hostStatus,
  releaseChannel,
  usageLevel,
  vaultHealth,
} from "../dashboard";
import type { ConnectionLogEntry } from "../stores/connectionlog.svelte";

describe("the build", () => {
  it("is STABLE unless the version carries a pre-release tag", () => {
    expect(releaseChannel("0.36.2")).toBe("STABLE");
    expect(releaseChannel("0.37.0-beta.1")).toBe("BETA");
    expect(releaseChannel("1.0.0-rc.2+build5")).toBe("RC");
    expect(releaseChannel("1.0.0+build5")).toBe("STABLE");
  });
});

describe("vault health", () => {
  const ok = { unlocked: true, conflicts: 0, rollbacks: 0 };
  it("is Healthy only when nothing is wrong", () => {
    expect(vaultHealth(ok)).toMatchObject({ label: "Healthy", tone: "success" });
  });
  it("says what is wrong, a rolled-back folder before conflicts", () => {
    expect(vaultHealth({ ...ok, conflicts: 2 })).toMatchObject({ label: "Conflicts", tone: "warning" });
    expect(vaultHealth({ ...ok, conflicts: 2, rollbacks: 1 })).toMatchObject({ label: "Rolled back", tone: "danger" });
    expect(vaultHealth({ ...ok, conflicts: 1 }).detail).toContain("1 item was");
    expect(vaultHealth({ ...ok, rollbacks: 3 }).detail).toContain("3 records are");
  });
  it("never calls a locked vault healthy", () => {
    expect(vaultHealth({ ...ok, unlocked: false }).label).toBe("Locked");
  });
});

describe("host status", () => {
  it("is online or offline only when a probe said so", () => {
    expect(hostStatus({ state: "up", latency_ms: 4, banner: null })).toBe("online");
    expect(hostStatus({ state: "down", reason: "refused" })).toBe("offline");
    expect(hostStatus({ state: "via_jump" })).toBe("unknown");
    expect(hostStatus(undefined)).toBe("unknown");
  });
  it("counts the online ones", () => {
    expect(countOnline(["online", "offline", "unknown", "online"])).toBe(2);
    expect(countOnline([])).toBe(0);
  });
  it("sorts names the way a person counts", () => {
    const names = ["web-10", "web-2", "Db-1", "web-1"].map((label) => ({ label }));
    expect(names.sort(byLabel).map((n) => n.label)).toEqual(["Db-1", "web-1", "web-2", "web-10"]);
  });
});

describe("usage bars", () => {
  it("is green, then yellow, then orange, switching at the thresholds", () => {
    expect(usageLevel(0)).toBe("low");
    expect(usageLevel(MODERATE_FROM - 1)).toBe("low");
    expect(usageLevel(MODERATE_FROM)).toBe("moderate");
    expect(usageLevel(HIGH_FROM - 1)).toBe("moderate");
    expect(usageLevel(HIGH_FROM)).toBe("high");
    expect(usageLevel(100)).toBe("high");
    expect(LEVEL_FILL).toEqual({ low: "bg-success", moderate: "bg-usage-moderate", high: "bg-usage-high" });
  });
  it("keeps nonsense inside the bar", () => {
    expect(clampPct(-5)).toBe(0);
    expect(clampPct(250)).toBe(100);
    expect(clampPct(Number.NaN)).toBe(0);
    expect(clampPct(41.6)).toBe(42);
    expect(usageLevel(Number.NaN)).toBe("low");
  });
});

describe("tunnels", () => {
  it("counts the active ones, not the starting, stopped or failed", () => {
    expect(activeTunnels({ a: { state: "active", port: 80 }, b: { state: "starting" }, c: { state: "stopped" }, d: { state: "error", message: "x" }, e: { state: "active", port: 81 } })).toBe(2);
    expect(activeTunnels({})).toBe(0);
  });
});

describe("recent activity", () => {
  const NOW = 1_000_000_000;
  const entry = (id: string, label: string, kind: ConnectionLogEntry["kind"], agoMs: number, reason: ConnectionLogEntry["reason"] = null): ConnectionLogEntry => ({
    id, hostId: null, label, kind, startedAt: NOW - agoMs, endedAt: null, exitCode: null, reason,
  });

  it("writes the elapsed time compactly", () => {
    expect(elapsedShort(0)).toBe("0s");
    expect(elapsedShort(-5000)).toBe("0s");
    expect(elapsedShort(59_999)).toBe("59s");
    expect(elapsedShort(60_000)).toBe("1m");
    expect(elapsedShort(59 * 60_000)).toBe("59m");
    expect(elapsedShort(3_600_000)).toBe("1h");
    expect(elapsedShort(23 * 3_600_000)).toBe("23h");
    expect(elapsedShort(24 * 3_600_000)).toBe("1d");
  });
  it("names what each kind of session opened", () => {
    const rows = activityRows([entry("1", "web-01", "host", 5000), entry("2", "web-01", "files", 120_000), entry("3", "workshop-pc", "rdp", 7_200_000), entry("4", "10.0.0.9", "adhoc", 0), entry("5", "ttyUSB0", "serial", 0)], NOW);
    expect(rows.map((r) => [r.host, r.what, r.ago])).toEqual([
      ["web-01", ">_ Terminal", "5s"],
      ["web-01", "Files", "2m"],
      ["workshop-pc", "RDP", "2h"],
      ["10.0.0.9", ">_ Terminal", "0s"],
      ["ttyUSB0", "Serial", "0s"],
    ]);
  });
  it("shows only the newest few, and marks a connection that failed", () => {
    const many = Array.from({ length: 20 }, (_, i) => entry(String(i), `h${i}`, "host", i * 1000, i === 1 ? "failed" : null));
    const rows = activityRows(many, NOW, 3);
    expect(rows.map((r) => r.id)).toEqual(["0", "1", "2"]);
    expect(rows.map((r) => r.failed)).toEqual([false, true, false]);
    expect(activityRows([], NOW)).toEqual([]);
  });
  it("falls back to the raw kind for one a newer version wrote", () => {
    const odd = { ...entry("1", "x", "host", 0), kind: "quic" } as unknown as ConnectionLogEntry;
    expect(activityRows([odd], NOW)[0].what).toBe("quic");
  });
});
