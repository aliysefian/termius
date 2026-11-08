import { describe, expect, it } from "vitest";
import { countedNote, loadPerCpu, mainFilesystem, memoryBar, sortedInterfaces, unsupportedReason, usageTone } from "../hostview";
import type { Filesystem, HostSample, Memory, NetIface } from "../hostsample";

const fs = (mount: string, used_pct: number): Filesystem => ({ device: "/dev/x", mount, fstype: "ext4", total: 100, used: used_pct, available: 100 - used_pct, used_pct });
const iface = (name: string, kind: NetIface["kind"], over: Partial<NetIface> = {}): NetIface => ({ name, kind, state: "up", master: null, mtu: null, link_mbps: null, rx_bps: null, tx_bps: null, rx_pps: null, tx_pps: null, rx_bytes: 0, tx_bytes: 0, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, counted: false, ...over });

describe("the overview's filesystem", () => {
  it("is the root one, else the fullest, else none", () => {
    expect(mainFilesystem([fs("/home", 90), fs("/", 40)])?.mount).toBe("/");
    expect(mainFilesystem([fs("/data", 30), fs("/home", 90)])?.mount).toBe("/home");
    expect(mainFilesystem([])).toBeNull();
  });
});

describe("the memory bar", () => {
  const mem = (over: Partial<Memory>): Memory => ({ total: 1000, used: 400, available: 600, free: 100, buffers: 0, cached: 0, shared: 0, swap_total: 0, swap_used: 0, committed: null, slab: null, used_pct: 40, available_exact: true, ...over });

  it("adds up to the whole and agrees with the percentage", () => {
    const b = memoryBar(mem({}));
    expect(b).toEqual({ used: 0.4, reclaimable: 0.5, free: 0.1 });
    expect(b.used + b.reclaimable + b.free).toBeCloseTo(1);
  });

  it("never lets free exceed what is available", () => {
    const b = memoryBar(mem({ used: 500, available: 500, free: 700 }));
    expect(b.free).toBe(0.5);
    expect(b.reclaimable).toBe(0);
    expect(b.used + b.reclaimable + b.free).toBeCloseTo(1);
  });
});

describe("network wording", () => {
  it("says why an interface is or is not in the total", () => {
    expect(countedNote(iface("eth0", "physical", { counted: true }))).toMatch(/Counted/);
    expect(countedNote(iface("br0", "bridge"))).toMatch(/real interface/);
    expect(countedNote(iface("eth1", "physical", { master: "bond0" }))).toMatch(/part of bond0/);
    expect(countedNote(iface("x0", "other"))).toBe("Not in the total.");
  });

  it("lists real traffic before plumbing, with numbers in natural order", () => {
    const names = sortedInterfaces([iface("docker0", "bridge"), iface("wg0", "tunnel"), iface("eth10", "physical", { counted: true }), iface("eth2", "physical", { counted: true }), iface("veth1", "container")]).map((i) => i.name);
    expect(names).toEqual(["eth2", "eth10", "docker0", "wg0", "veth1"]);
  });
});

describe("small decisions", () => {
  it("compares load with the number of CPUs", () => {
    expect(loadPerCpu(6, 4)).toBe(1.5);
    expect(loadPerCpu(1, 0)).toBeNull();
  });

  it("grades use without inventing a grade for nothing", () => {
    expect(usageTone(null)).toBe("none");
    expect(usageTone(50)).toBe("ok");
    expect(usageTone(75)).toBe("warn");
    expect(usageTone(95)).toBe("bad");
  });

  it("explains an unsupported system and nothing else", () => {
    const s = { platform: { kind: "unsupported", os: "Darwin" } } as HostSample;
    expect(unsupportedReason(s)).toMatch(/Darwin/);
    expect(unsupportedReason({ platform: { kind: "linux" } } as HostSample)).toBeNull();
  });
});
