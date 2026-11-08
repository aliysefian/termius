import { describe, expect, it } from "vitest";
import { UNAVAILABLE, baselineNote, busiest, bytesText, figure, hasRates, isStale, memText, pctText, rateText, type HostSample, type LatestSample } from "../hostsample";

const sample = (over: Partial<HostSample> = {}): HostSample => ({
  host_id: "h1",
  taken_at_ms: 0,
  platform: { kind: "linux" },
  baseline: { kind: "measured", elapsed_secs: 2 },
  uptime_secs: 100,
  cpu: { total_pct: 24.8, iowait_pct: 0, steal_pct: 0, cores: [], logical: 4, freq_mhz: null },
  memory: null,
  load: null,
  processes: null,
  filesystems: [],
  disks: [],
  network: null,
  temperatures: [],
  gpus: [],
  notes: [],
  capabilities: { proc_stat: true, meminfo: true, loadavg: true, net_dev: true, diskstats: true, sys_block: true, sys_net: true, filesystems: true, cpufreq: false, sensors: false, amd_gpu: false, nvidia_smi: false, tools: [] },
  info: { hostname: null, os_name: null, kernel: null, arch: null, cpu_model: null, sockets: null, physical_cores: null, logical_cpus: null, max_mhz: null, base_mhz: null, virtualization: null, hypervisor: null, caches: [] },
  ...over,
});

const latest = (id: string, over: Partial<HostSample>, age_ms = 1000): LatestSample => ({ sample: sample({ host_id: id, ...over }), age_ms });

describe("showing unmeasured figures", () => {
  it("never turns a missing figure into a zero", () => {
    expect(pctText(null)).toBe(UNAVAILABLE);
    expect(pctText(undefined)).toBe(UNAVAILABLE);
    expect(pctText(NaN)).toBe(UNAVAILABLE);
    expect(rateText(null)).toBe(UNAVAILABLE);
    expect(rateText(-5)).toBe(UNAVAILABLE);
    expect(bytesText(null)).toBe(UNAVAILABLE);
    expect(memText(null)).toBe(UNAVAILABLE);
    // A real zero is a real zero.
    expect(pctText(0)).toBe("0.0%");
    expect(rateText(0)).toBe("0 B/s");
  });

  it("writes speeds with units that cannot be mistaken for each other", () => {
    expect(rateText(12_800_000)).toBe("12.8 MB/s");
    expect(rateText(3_420_000)).toBe("3.42 MB/s");
    expect(rateText(512)).toBe("512 B/s");
    expect(rateText(1_500)).toBe("1.50 KB/s");
    expect(rateText(2_500_000_000)).toBe("2.50 GB/s");
    // The same speed in bits: eight times the number, a lowercase-b unit.
    expect(rateText(12_800_000, "bits")).toBe("102 Mbps");
    expect(rateText(125_000, "bits")).toBe("1.00 Mbps");
    expect(rateText(0, "bits")).toBe("0 bps");
  });

  it("writes percentages, sizes and memory", () => {
    expect(pctText(24.8)).toBe("24.8%");
    expect(pctText(100)).toBe("100%");
    expect(pctText(99.97)).toBe("100%");
    expect(bytesText(24_800_000_000)).toBe("24.8 GB");
    expect(memText(16 * 1024 ** 3)).toBe("16.0 GiB");
    expect(memText(512)).toBe("512 B");
  });
});

describe("why a rate is missing", () => {
  it("says so for every baseline but a measured one", () => {
    expect(baselineNote({ kind: "measured", elapsed_secs: 2 })).toBeNull();
    expect(baselineNote({ kind: "first" })).toMatch(/First reading/);
    expect(baselineNote({ kind: "rebooted" })).toMatch(/restarted/);
    expect(baselineNote({ kind: "stale", gap_secs: 1200 })).toMatch(/20 min/);
    expect(baselineNote({ kind: "too_close" })).toMatch(/too soon/);
    expect(hasRates(sample())).toBe(true);
    expect(hasRates(sample({ baseline: { kind: "first" } }))).toBe(false);
  });
});

describe("staleness", () => {
  it("is a few intervals, bounded", () => {
    expect(isStale(5_000, 2_000)).toBe(false);
    expect(isStale(11_000, 2_000)).toBe(true); // 3 x 2 s is under the 10 s floor
    expect(isStale(80_000, 30_000)).toBe(false);
    expect(isStale(95_000, 30_000)).toBe(true);
    expect(isStale(130_000, 600_000)).toBe(true); // never more than 2 minutes
  });
});

describe("the busiest hosts", () => {
  const net = (rx: number, tx: number) => ({ interfaces: [], default_iface: null, total: { rx_bps: rx, tx_bps: tx, from: ["eth0"] } });
  const disk = (busy: number | null, virt = false) => ({ name: "x", model: null, size: null, rotational: null, virtual_device: virt, read_bps: null, write_bps: null, read_iops: null, write_iops: null, busy_pct: busy, await_ms: null, read_total: 0, write_total: 0 });

  it("ranks by one metric at a time, highest first, and leaves out hosts with no figure for it", () => {
    const all = [
      latest("a", { cpu: { total_pct: 40, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null } }),
      latest("b", { cpu: { total_pct: 92, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null } }),
      latest("c", { cpu: { total_pct: null, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null } }), // first reading: no rate yet
      latest("d", { cpu: null, platform: { kind: "unsupported", os: "Darwin" } }),
    ];
    expect(busiest(all, "cpu").map((r) => [r.hostId, r.value])).toEqual([["b", 92], ["a", 40]]);
  });

  it("ranks network by total speed and disk by the busiest real disk, never adding unlike things", () => {
    const all = [latest("a", { network: net(1_000_000, 500_000), disks: [disk(10), disk(99, true)] }), latest("b", { network: net(100, 100), disks: [disk(60)] })];
    expect(busiest(all, "net").map((r) => r.hostId)).toEqual(["a", "b"]);
    expect(figure(all[0].sample, "net")).toBe(1_500_000);
    expect(busiest(all, "disk").map((r) => [r.hostId, r.value])).toEqual([["b", 60], ["a", 10]]);
  });

  it("marks old readings stale without hiding them, and honours the limit", () => {
    const all = [latest("a", { cpu: { total_pct: 50, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null } }, 500_000), latest("b", { cpu: { total_pct: 10, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null } }, 1_000)];
    const r = busiest(all, "cpu", 30_000, 1);
    expect(r).toHaveLength(1);
    expect(r[0]).toMatchObject({ hostId: "a", stale: true });
  });
});
