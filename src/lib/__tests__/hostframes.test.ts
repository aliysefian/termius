import { describe, expect, it } from "vitest";
import { TOTAL, addFrame, backfill, fitWindow, frameOf, ifacePick, peak, pickInterface, within, type Frame } from "../hostframes";
import type { DiskIo, HostSample } from "../hostsample";

const disk = (over: Partial<DiskIo>): DiskIo => ({ name: "sda", model: null, size: null, rotational: null, virtual_device: false, read_bps: 100, write_bps: 50, read_iops: null, write_iops: null, busy_pct: 10, await_ms: null, read_total: 0, write_total: 0, ...over });
const base = (over: Partial<HostSample> = {}): HostSample => ({
  host_id: "h",
  taken_at_ms: 1000,
  platform: { kind: "linux" },
  baseline: { kind: "measured", elapsed_secs: 2 },
  uptime_secs: 1,
  cpu: { total_pct: 30, iowait_pct: null, steal_pct: null, cores: [], logical: 2, freq_mhz: null },
  memory: { total: 100, used: 60, available: 40, free: 10, buffers: 0, cached: 30, shared: 0, swap_total: 0, swap_used: 0, committed: null, slab: null, used_pct: 60, available_exact: true },
  load: { m1: 1.5, m5: 1, m15: 1 },
  processes: null,
  filesystems: [],
  disks: [],
  temperatures: [],
  gpus: [],
  notes: [],
  network: { interfaces: [{ name: "eth0", kind: "physical", state: "up", master: null, mtu: null, link_mbps: null, rx_bps: 10, tx_bps: 5, rx_pps: null, tx_pps: null, rx_bytes: 0, tx_bytes: 0, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, counted: true }], default_iface: "eth0", total: { rx_bps: 10, tx_bps: 5, from: ["eth0"] } },
  capabilities: { proc_stat: true, meminfo: true, loadavg: true, net_dev: true, diskstats: true, sys_block: true, sys_net: true, filesystems: true, cpufreq: false, sensors: false, amd_gpu: false, nvidia_smi: false, tools: [] },
  info: { hostname: null, os_name: null, kernel: null, arch: null, cpu_model: null, sockets: null, physical_cores: null, logical_cpus: null, max_mhz: null, base_mhz: null, virtualization: null, hypervisor: null, caches: [] },
  ...over,
});
const fr = (t: number, rx: number | null = 1): Frame => ({ t, cpu: null, mem: null, load1: null, rx, tx: null, diskRead: null, diskWrite: null, diskBusy: null, temp: null, gpu: null, gpus: {}, ifaces: {} });

describe("frameOf", () => {
  it("keeps what was measured and nothing else", () => {
    const f = frameOf(base({ disks: [disk({}), disk({ name: "dm-0", virtual_device: true, read_bps: 9999, busy_pct: 99 }), disk({ name: "sdb", read_bps: 20, write_bps: 10, busy_pct: 40 })] }));
    expect(f).toMatchObject({ t: 1000, cpu: 30, mem: 60, load1: 1.5, rx: 10, tx: 5, diskRead: 120, diskWrite: 60, diskBusy: 40 });
    expect(f.ifaces).toEqual({ eth0: [10, 5] });
  });

  it("leaves a first reading's rates null, and a disk sum with a missing disk unknown", () => {
    const first = frameOf(base({ cpu: { total_pct: null, iowait_pct: null, steal_pct: null, cores: [], logical: 2, freq_mhz: null }, network: { interfaces: [], default_iface: null, total: null } }));
    expect([first.cpu, first.rx, first.tx]).toEqual([null, null, null]);
    expect(frameOf(base({ disks: [disk({}), disk({ name: "sdb", read_bps: null })] })).diskRead).toBeNull();
    expect(frameOf(base()).diskRead).toBeNull();
  });
});

describe("addFrame", () => {
  it("adds in order, ignores the same reading twice, and places a late one correctly", () => {
    let l: Frame[] = [];
    l = addFrame(l, fr(1000));
    l = addFrame(l, fr(3000));
    expect(addFrame(l, fr(3000))).toBe(l);
    l = addFrame(l, fr(2000));
    expect(l.map((f) => f.t)).toEqual([1000, 2000, 3000]);
  });

  it("drops what is past the window or the cap", () => {
    let l: Frame[] = [];
    for (const t of [0, 1000, 5000, 9000]) l = addFrame(l, fr(t), 6000);
    expect(l.map((f) => f.t)).toEqual([5000, 9000]);
    let m: Frame[] = [];
    for (let t = 0; t < 10; t++) m = addFrame(m, fr(t * 10), 1e9, 4);
    expect(m.map((f) => f.t)).toEqual([60, 70, 80, 90]);
  });
});

describe("backfill", () => {
  it("puts the engine's kept history before what the window saw, without repeating a moment", () => {
    const pts = [{ t: 100, cpu: 1, mem: 2, disk_used: null, load1: null, rx_bps: 5, tx_bps: 6, disk_busy: null, temp_cpu: null, gpu_util: null }, { t: 500, cpu: 1, mem: 2, disk_used: null, load1: null, rx_bps: 5, tx_bps: 6, disk_busy: null, temp_cpu: null, gpu_util: null }];
    const l = backfill([fr(300), fr(400)], pts);
    expect(l.map((f) => f.t)).toEqual([100, 300, 400]);
    expect(l[0].ifaces).toEqual({});
    expect(backfill([fr(50)], pts)).toHaveLength(1);
    expect(backfill([], pts).map((f) => f.t)).toEqual([100, 500]);
  });
});

describe("series helpers", () => {
  it("finds a peak only among measured values", () => {
    expect(peak([fr(1, null), fr(2, 5), fr(3, 9), fr(4, null)], (f) => f.rx)).toBe(9);
    expect(peak([fr(1, null)], (f) => f.rx)).toBeNull();
    expect(peak([], (f) => f.rx)).toBeNull();
  });

  it("cuts a window and reads one interface", () => {
    expect(within([fr(1), fr(50), fr(95), fr(120)], 100, 60).map((f) => f.t)).toEqual([50, 95]);
    const f = { ...fr(1), ifaces: { eth0: [3, 4] as [number, number] } };
    expect(ifacePick("eth0", "rx")(f)).toBe(3);
    expect(ifacePick("eth0", "tx")(f)).toBe(4);
    expect(ifacePick("wg0", "rx")(f)).toBeNull();
  });
});

describe("pickInterface", () => {
  it("honours a saved choice that still exists, else falls back sensibly", () => {
    expect(pickInterface("eth1", ["eth0", "eth1"], "eth0", true)).toBe("eth1");
    expect(pickInterface("gone", ["eth0"], "eth0", true)).toBe(TOTAL);
    expect(pickInterface(null, ["eth0"], "eth0", false)).toBe("eth0");
    expect(pickInterface(TOTAL, ["eth0"], null, false)).toBe("eth0");
    expect(pickInterface(null, [], null, false)).toBe(TOTAL);
  });
});

describe("fitWindow", () => {
  it("covers what there is, not the whole range, until there is enough", () => {
    expect(fitWindow([], 100_000, 900_000)).toBe(60_000);
    expect(fitWindow([fr(95_000)], 100_000, 900_000)).toBe(60_000);
    expect(fitWindow([fr(0), fr(300_000)], 400_000, 900_000)).toBe(405_000);
    expect(fitWindow([fr(0)], 2_000_000, 900_000)).toBe(60_000); // old frames outside the range don't count
    expect(fitWindow([fr(1_000_000)], 1_890_000, 900_000)).toBe(895_000);
    expect(fitWindow([fr(1_000_000)], 1_899_000, 900_000)).toBe(900_000);
  });
});
