import { describe, expect, it } from "vitest";
import { KEEP_MAX, KEEP_MS, RANGES, addPoint, bucketPeaks, formatRate, formatTotal, headlineInterface, nearest, niceMax, paths, peak, rangeAvailable, rateText, sortInterfaces, sparkline, ticks, updatedAgo, utilization, type NetInterface, type NetPoint } from "../netdata";

const iface = (name: string, extra: Partial<NetInterface> = {}): NetInterface => ({ name, state: "up", physical: true, link_mbps: null, rx_bps: 0, tx_bps: 0, rx_pps: 0, tx_pps: 0, rx_total: 0, tx_total: 0, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, ...extra });

describe("speed units", () => {
  it("scales bytes per second automatically and says which unit", () => {
    expect(rateText(0)).toBe("0 B/s");
    expect(rateText(512)).toBe("512 B/s");
    expect(rateText(1500)).toBe("1.50 KB/s");
    expect(rateText(12_800_000)).toBe("12.8 MB/s");
    expect(rateText(3_420_000)).toBe("3.42 MB/s");
    expect(rateText(250_000_000)).toBe("250 MB/s");
    expect(rateText(2_300_000_000)).toBe("2.30 GB/s");
  });

  it("shows bits when asked, which is eight times the bytes", () => {
    expect(rateText(1_250_000, "bits")).toBe("10.0 Mbit/s");
    expect(rateText(125_000_000, "bits")).toBe("1.00 Gbit/s");
    expect(formatRate(100, "bits")).toEqual({ value: "800", unit: "bit/s" });
  });

  it("shows no number, not a zero, when there is no reading", () => {
    expect(rateText(null)).toBe("–");
    expect(formatRate(null)).toEqual({ value: "–", unit: "B/s" });
    expect(formatTotal(null)).toBe("–");
    expect(formatRate(NaN).value).toBe("–");
  });

  it("formats totals in powers of 1000", () => {
    expect([formatTotal(0), formatTotal(999), formatTotal(24_800_000_000), formatTotal(8_700_000_000), formatTotal(1_500_000)]).toEqual(["0 B", "999 B", "24.8 GB", "8.70 GB", "1.50 MB"]);
  });

  it("works out how full a link is, only when its speed is known", () => {
    expect(utilization(12_500_000, 1000)).toBe(10);
    expect(utilization(125_000_000, 100)).toBe(100);
    expect(utilization(1, null)).toBeNull();
    expect(utilization(null, 1000)).toBeNull();
  });
});

describe("interfaces", () => {
  it("finds the named headline interface and never guesses one", () => {
    const r = { interfaces: [iface("eth0"), iface("wg0")], primary: "eth0", window_secs: 1 };
    expect(headlineInterface(r, "wg0")?.name).toBe("wg0");
    expect(headlineInterface(r, null)).toBeNull();
    expect(headlineInterface(r, "zzz")).toBeNull();
    expect(headlineInterface(null, "eth0")).toBeNull();
  });

  it("lists the main interface first, then real devices by traffic", () => {
    const list = [iface("docker0", { physical: false, rx_bps: 9e9 }), iface("ens19", { rx_bps: 10 }), iface("eth0", { rx_bps: 5 }), iface("ens18", { rx_bps: 500 })];
    expect(sortInterfaces(list, "eth0").map((i) => i.name)).toEqual(["eth0", "ens18", "ens19", "docker0"]);
  });
});

describe("readings over time", () => {
  const p = (t: number, rx: number | null, tx: number | null = 0): NetPoint => ({ t, rx, tx });

  it("keeps an hour at most, and merges a repeat of the same instant", () => {
    let l: NetPoint[] = [];
    l = addPoint(l, p(0, 1));
    l = addPoint(l, p(100, 2, 9));
    expect(l).toEqual([{ t: 100, rx: 2, tx: 9 }]);
    l = addPoint(l, p(KEEP_MS + 5000, 3));
    expect(l.map((x) => x.t)).toEqual([KEEP_MS + 5000]);
    let big: NetPoint[] = [];
    for (let i = 0; i < KEEP_MAX + 50; i++) big = addPoint(big, p(i * 500, 1), Infinity);
    expect(big).toHaveLength(KEEP_MAX);
  });

  it("starts over when the clock steps back", () => {
    expect(addPoint([p(1000, 1), p(2000, 1)], p(500, 7))).toEqual([p(500, 7)]);
  });

  it("answers a range from memory, or from kept history, or says what to turn on", () => {
    const by = Object.fromEntries(RANGES.map((r) => [r.id, r]));
    expect(rangeAvailable(by["1h"], "off").ok).toBe(true);
    expect(rangeAvailable(by["6h"], "off")).toMatchObject({ ok: false });
    expect(rangeAvailable(by["6h"], "off").why).toContain("Keep history");
    expect(rangeAvailable(by["24h"], "day").ok).toBe(true);
    expect(rangeAvailable(by["7d"], "day").ok).toBe(false);
    expect(rangeAvailable(by["7d"], "week").ok).toBe(true);
  });

  it("reduces to columns by their peak, so a burst is never averaged away, and leaves empty columns empty", () => {
    const pts = [p(0, 10), p(1, 900), p(2, 20), p(30, 5)];
    const cols = bucketPeaks(pts, 0, 40, 4);
    expect(cols.map((c) => c.rx)).toEqual([900, null, null, 5]);
    expect(bucketPeaks(pts, 0, 40, 0)).toEqual([]);
    expect(bucketPeaks(pts, 5, 5, 4)).toEqual([]);
    expect(peak(pts, 0, 40)).toEqual({ rx: 900, tx: 0 });
    expect(peak([], 0, 1)).toEqual({ rx: null, tx: null });
  });
});

describe("chart geometry", () => {
  it("picks a tidy top and evenly spaced ticks", () => {
    expect([niceMax(0), niceMax(1200), niceMax(3_400_000), niceMax(13_000_000), niceMax(70_000_000)]).toEqual([1000, 2000, 5_000_000, 20_000_000, 100_000_000]);
    expect(ticks(100, 4)).toEqual([0, 25, 50, 75, 100]);
  });

  it("draws a gap where there is no reading", () => {
    const box = { width: 100, height: 50, from: 0, to: 100, max: 100 };
    const cols = [{ t: 10, rx: 50, tx: null }, { t: 20, rx: 100, tx: null }, { t: 50, rx: null, tx: null }, { t: 80, rx: 0, tx: null }, { t: 90, rx: 25, tx: null }];
    const { line, area } = paths(cols, (c) => c.rx, box);
    expect(line.match(/M/g)).toHaveLength(2);
    expect(line).toContain("M10.0,25.0 L20.0,0.0");
    expect(area.match(/Z/g)).toHaveLength(2);
    expect(paths(cols, (c) => c.tx, box)).toEqual({ line: "", area: "" });
    // A value above the top is drawn at the top, not outside the chart.
    expect(paths([{ t: 10, rx: 500, tx: null }, { t: 20, rx: 500, tx: null }], (c) => c.rx, box).line).toBe("M10.0,0.0 L20.0,0.0");
  });

  it("finds the column nearest the pointer, skipping empty ones", () => {
    const cols = [{ t: 10, rx: 1, tx: null }, { t: 20, rx: null, tx: null }, { t: 30, rx: 3, tx: null }];
    expect(nearest(cols, 19)?.t).toBe(10);
    expect(nearest(cols, 26)?.t).toBe(30);
    expect(nearest([{ t: 1, rx: null, tx: null }], 1)).toBeNull();
  });

  it("draws a small sparkline from the latest readings only", () => {
    const pts = Array.from({ length: 50 }, (_, i) => ({ t: i, rx: i, tx: null }));
    expect(sparkline(pts, (x) => x.rx, 60, 20, 10).split(" ")).toHaveLength(10);
    expect(sparkline([{ t: 1, rx: 1, tx: 1 }], (x) => x.rx, 60, 20)).toBe("");
  });

  it("says how long ago", () => {
    expect([updatedAgo(0, 1000), updatedAgo(0, 12_000), updatedAgo(0, 180_000)]).toEqual(["1 s ago", "12 s ago", "3 min ago"]);
  });
});
