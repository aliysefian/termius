// The network store keeps real readings only: one series per interface, the latest reading per host, nothing for a
// host without counters, and a live loop that stops when asked.
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: string[] = [];
let sample: unknown = null;

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string) => {
    calls.push(cmd);
    if (cmd === "monitor_open") return "sess";
    if (cmd === "monitor_net_sample") return sample;
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));

const { netStore } = await import("$lib/stores/netstore.svelte");

const iface = (name: string, rx: number | null, tx: number | null) => ({
  name, state: "up", physical: true, link_mbps: 1000, rx_bps: rx, tx_bps: tx, rx_pps: null, tx_pps: null, rx_total: 10, tx_total: 20, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0,
});

beforeEach(() => {
  vi.stubGlobal("document", { hidden: false });
  netStore.stopLive();
  netStore.reset();
  calls.length = 0;
  sample = null;
});

describe("netStore", () => {
  it("keeps a point per interface and remembers the latest reading", () => {
    netStore.ingest("t1", { interfaces: [iface("eth0", 100, 50), iface("wg0", 5, 1)], primary: "eth0", window_secs: 1 }, "eth0", 1000, "background");
    expect(netStore.points("t1", "eth0")).toHaveLength(1);
    expect(netStore.points("t1", "wg0")[0]).toMatchObject({ rx: 5, tx: 1 });
    expect(netStore.latest["t1"].headline).toBe("eth0");
    expect(netStore.points("t1", "nope")).toEqual([]);
  });

  it("keeps an unreadable speed as no value, not zero", () => {
    netStore.ingest("t1", { interfaces: [iface("eth0", null, null)], primary: "eth0", window_secs: 1 }, null, 1000, "live");
    expect(netStore.points("t1", "eth0")[0].rx).toBeNull();
  });

  it("forgets a host and clears everything on reset", () => {
    netStore.ingest("t1", { interfaces: [iface("eth0", 1, 1)], primary: "eth0", window_secs: 1 }, null, 1, "live");
    netStore.forget("t1");
    expect(netStore.latest["t1"]).toBeUndefined();
    netStore.ingest("t2", { interfaces: [iface("eth0", 1, 1)], primary: "eth0", window_secs: 1 }, null, 1, "live");
    netStore.reset();
    expect(netStore.series).toEqual({});
  });

  it("ignores a status without readings", () => {
    netStore.ingestStatus({ id: "t1", metrics: null } as never, null);
    expect(netStore.latest["t1"]).toBeUndefined();
  });

  it("reads live over one session, says so when the host has no counters, and stops", async () => {
    const run = netStore.startLive("t1", "h1", 1);
    await vi.waitFor(() => expect(netStore.liveError).toMatch(/doesn't report/));
    expect(calls.filter((c) => c === "monitor_open")).toHaveLength(1);
    sample = { interfaces: [iface("eth0", 9, 3)], primary: "eth0", window_secs: 1, headline: "eth0" };
    await vi.waitFor(() => expect(netStore.latest["t1"]?.source).toBe("live"), { timeout: 3000 });
    netStore.stopLive();
    await run;
    expect(netStore.liveRunning).toBe(false);
    expect(calls).toContain("monitor_close");
  });
});

describe("netStore snapshots", () => {
  it("adds a check seen again only once", () => {
    const st = { id: "t1", metrics: { cpu: 1, mem: 1, disk: 1, net: { interfaces: [iface("eth0", 4, 2)], primary: "eth0", window_secs: 1 }, net_headline: "eth0" } } as never;
    netStore.ingestStatus(st, null, 5000);
    netStore.ingestStatus(st, null, 5000);
    expect(netStore.points("t1", "eth0")).toHaveLength(1);
    netStore.ingestStatus(st, null, 6000);
    expect(netStore.points("t1", "eth0")).toHaveLength(2);
  });
});
