import { describe, expect, it } from "vitest";
import { DEFAULTS, PANEL_DEFAULT, PANEL_MAX, PANEL_MIN, TOOLBAR_EVERY_MS, clampWidth, metricsHost, readPrefs, wantedInterval } from "../terminalmetrics";

describe("remembered terminal choices", () => {
  it("falls back to the defaults for nothing, junk or the wrong shape", () => {
    for (const raw of [null, undefined, "", "nope", "[]", "5", "null"]) expect(readPrefs(raw)).toEqual(DEFAULTS);
  });

  it("keeps good values and repairs bad ones one by one", () => {
    const p = readPrefs(JSON.stringify({ side: "left", width: 400, live: 5000, hosts: { a: { enabled: true, panel: true }, b: { enabled: true, panel: false }, c: { enabled: false, panel: false }, d: "x", e: { enabled: "yes" } } }));
    expect(p).toEqual({ side: "left", width: 400, live: 5000, hosts: { a: { enabled: true, panel: true }, b: { enabled: true, panel: false } } });
    expect(readPrefs(JSON.stringify({ side: "top", width: "wide", live: 77 }))).toEqual(DEFAULTS);
  });

  it("never lets a stored open panel belong to a host that is off", () => {
    expect(readPrefs(JSON.stringify({ hosts: { a: { enabled: false, panel: true } } })).hosts.a).toEqual({ enabled: true, panel: true });
  });
});

describe("panel width", () => {
  it("stays between the limits and under 40% of a small window", () => {
    expect(clampWidth(10)).toBe(PANEL_MIN);
    expect(clampWidth(5000)).toBe(PANEL_MAX);
    expect(clampWidth(300, 600)).toBe(240); // 40% of a 600 px window
    expect(clampWidth(300, 400)).toBe(PANEL_MIN); // never under the minimum, even in a tiny window
    expect(clampWidth(500, 1000)).toBe(400);
    expect(clampWidth(NaN)).toBe(PANEL_DEFAULT);
  });
});

describe("which hosts can show metrics", () => {
  const hosts = new Map([
    ["ssh", { data: { protocol: "ssh" } }],
    ["plain", { data: {} }],
    ["tel", { data: { protocol: "telnet" } }],
    ["rdp", { data: { protocol: "rdp" } }],
  ]) as never;
  it("is a saved SSH host and nothing else", () => {
    expect(metricsHost({ kind: "host", hostId: "ssh" }, hosts)).toBe("ssh");
    expect(metricsHost({ kind: "host", hostId: "plain" }, hosts)).toBe("plain");
    expect(metricsHost({ kind: "host", hostId: "tel" }, hosts)).toBeNull();
    expect(metricsHost({ kind: "host", hostId: "rdp" }, hosts)).toBeNull();
    expect(metricsHost({ kind: "local" }, hosts)).toBeNull();
    expect(metricsHost({ kind: "telnet", host: "x", port: 23 }, hosts)).toBeNull();
    expect(metricsHost(undefined, hosts)).toBeNull();
  });
});

describe("how often a host is read", () => {
  it("never reads a host that is off", () => {
    expect(wantedInterval(undefined, true, 2000)).toBeNull();
    expect(wantedInterval({ enabled: false, panel: false }, true, 2000)).toBeNull();
  });

  it("reads slowly for the toolbar and at the live rate only while the panel is on screen", () => {
    expect(wantedInterval({ enabled: true, panel: false }, true, 1000)).toBe(TOOLBAR_EVERY_MS);
    expect(wantedInterval({ enabled: true, panel: true }, true, 1000)).toBe(1000);
    expect(wantedInterval({ enabled: true, panel: true }, false, 1000)).toBe(TOOLBAR_EVERY_MS);
    expect(wantedInterval({ enabled: true, panel: true }, true, 0)).toBe(TOOLBAR_EVERY_MS);
  });
});
