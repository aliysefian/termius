import { describe, expect, it } from "vitest";
import { GAP_MS, HISTORY_MS, addSample, areaOf, chartSegments, clock, fromDetail, fromSummary, nearestIndex, niceMax, pathOf, stepsFor, ticks, timeLabel, type Box, type Sample } from "../hosthistory";
import type { HostDetail } from "../hostdetail";

const s = (t: number, cpu: number | null = 10, mem: number | null = 20, rx: number | null = null, tx: number | null = null): Sample => ({ t, cpu, mem, rx, tx });
const MIN = 60_000;

describe("keeping fifteen minutes", () => {
  it("adds in order and returns a new array each time", () => {
    const a: Sample[] = [];
    const b = addSample(a, s(1_000_000));
    const c = addSample(b, s(1_005_000));
    expect(a).toEqual([]);
    expect(b).toHaveLength(1);
    expect(c.map((x) => x.t)).toEqual([1_000_000, 1_005_000]);
    expect(c).not.toBe(b);
  });
  it("drops what is older than the window, but never the newest", () => {
    let l: Sample[] = [];
    for (let i = 0; i <= 40; i++) l = addSample(l, s(i * MIN));
    const newest = 40 * MIN;
    expect(l[l.length - 1].t).toBe(newest);
    expect(l[0].t).toBeGreaterThanOrEqual(newest - HISTORY_MS);
    expect(l.length).toBe(16);
    // A single very old sample then a new one: the old one goes.
    expect(addSample([s(0)], s(HISTORY_MS * 3)).map((x) => x.t)).toEqual([HISTORY_MS * 3]);
  });
  it("caps the count", () => {
    let l: Sample[] = [];
    for (let i = 0; i < 50; i++) l = addSample(l, s(1_000_000 + i * 1000), HISTORY_MS, 10);
    expect(l).toHaveLength(10);
    expect(l[9].t).toBe(1_000_000 + 49 * 1000);
  });
  it("merges two readings of the same instant, keeping what either knew", () => {
    const l = addSample([s(5000, 30, 40, null, null)], s(5100, null, null, 900, 100));
    expect(l).toEqual([{ t: 5100, cpu: 30, mem: 40, rx: 900, tx: 100 }]);
  });
  it("starts over when the clock steps backwards", () => {
    expect(addSample([s(9000), s(10_000)], s(2000)).map((x) => x.t)).toEqual([2000]);
  });
  it("turns a summary or a detail reading into a sample", () => {
    expect(fromSummary({ cpuPct: 12, memPct: 34, diskPct: 50, load: null, uptimeSecs: 9, failedUnits: null }, 7)).toEqual({ t: 7, cpu: 12, mem: 34, rx: null, tx: null });
    const d = {
      cpuPct: 5,
      memPct: 6,
      rates: [
        { name: "lo", rxBps: 999, txBps: 999, rxBytes: 0, txBytes: 0 },
        { name: "eth0", rxBps: 100, txBps: 20, rxBytes: 0, txBytes: 0 },
        { name: "eth1", rxBps: 1, txBps: 2, rxBytes: 0, txBytes: 0 },
      ],
    } as HostDetail;
    expect(fromDetail(d, 8)).toEqual({ t: 8, cpu: 5, mem: 6, rx: 101, tx: 22 });
    expect(fromDetail({ ...d, rates: [] }, 9)).toEqual({ t: 9, cpu: 5, mem: 6, rx: null, tx: null });
  });
});

describe("drawing a series", () => {
  const box: Box = { width: 100, height: 50, from: 0, to: 100_000, max: 100 };
  it("maps time to x and value to y, 0 at the bottom", () => {
    const seg = chartSegments([s(0, 0), s(50_000, 50), s(100_000, 100)], (x) => x.cpu, box);
    expect(seg).toHaveLength(1);
    expect(seg[0].map((p) => [p.x, p.y])).toEqual([[0, 50], [50, 25], [100, 0]]);
    expect(seg[0].map((p) => p.i)).toEqual([0, 1, 2]);
  });
  it("breaks the line where a value is missing, and where time has a hole", () => {
    const a = chartSegments([s(0, 1), s(10_000, null), s(20_000, 3)], (x) => x.cpu, box);
    expect(a.map((r) => r.length)).toEqual([1, 1]);
    const wide: Box = { ...box, to: 10 * MIN };
    const b = chartSegments([s(0, 1), s(30_000, 2), s(30_000 + GAP_MS + 1, 3), s(30_000 + GAP_MS + 31_000, 4)], (x) => x.cpu, wide);
    expect(b.map((r) => r.length)).toEqual([2, 2]);
    const c = chartSegments([s(0, 1), s(GAP_MS, 2)], (x) => x.cpu, { ...wide });
    expect(c.map((r) => r.length), "exactly the limit is not a hole").toEqual([2]);
  });
  it("leaves out what is outside the time range and clamps what is outside the scale", () => {
    const seg = chartSegments([s(-5000, 10), s(1000, 500), s(200_000, 10)], (x) => x.cpu, box);
    expect(seg).toHaveLength(1);
    expect(seg[0]).toHaveLength(1);
    expect(seg[0][0].y).toBe(0);
    expect(chartSegments([s(1000, -50)], (x) => x.cpu, box)[0][0].y).toBe(50);
  });
  it("draws nothing for an empty or degenerate box", () => {
    expect(chartSegments([], (x) => x.cpu, box)).toEqual([]);
    expect(chartSegments([s(1)], (x) => x.cpu, { ...box, to: 0 })).toEqual([]);
    expect(chartSegments([s(1)], (x) => x.cpu, { ...box, max: 0 })).toEqual([]);
  });
  it("makes paths, and a visible dot of a lone reading", () => {
    const pts = [{ x: 0, y: 10, i: 0 }, { x: 5.55, y: 20, i: 1 }];
    expect(pathOf(pts)).toBe("M0.0 10.0 L5.5 20.0".replace("5.5", "5.5"));
    expect(pathOf([{ x: 3, y: 4, i: 0 }])).toBe("M3.0 4.0 L3.0 4.0");
    expect(pathOf([])).toBe("");
    expect(areaOf(pts, 50)).toBe("M0.0 10.0 L5.5 20.0 L5.5 50 L0.0 50 Z");
    expect(areaOf([pts[0]], 50)).toBe("");
  });
});

describe("axes", () => {
  it("rounds a maximum up to 1, 2 or 5 times a power of ten, never below the floor", () => {
    expect(niceMax(0, 1000)).toBe(1000);
    expect(niceMax(950, 1000)).toBe(1000);
    expect(niceMax(1001, 1000)).toBe(2000);
    expect(niceMax(2300)).toBe(5000);
    expect(niceMax(4200)).toBe(5000);
    expect(niceMax(7_300_000)).toBe(10_000_000);
    expect(niceMax(3)).toBe(5);
    expect(niceMax(NaN, 10)).toBe(10);
    expect(niceMax(Infinity, 10)).toBe(10);
  });
  it("divides every such maximum into round steps, never into 1.25s", () => {
    expect(ticks(5_000_000, stepsFor(5_000_000))).toEqual([0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000]);
    expect(ticks(2000, stepsFor(2000))).toEqual([0, 500, 1000, 1500, 2000]);
    expect(ticks(1000, stepsFor(1000))).toEqual([0, 200, 400, 600, 800, 1000]);
    expect(ticks(10_000, stepsFor(10_000))).toEqual([0, 2000, 4000, 6000, 8000, 10_000]);
  });
  it("spaces ticks evenly from zero", () => {
    expect(ticks(100)).toEqual([0, 25, 50, 75, 100]);
    expect(ticks(1000, 2)).toEqual([0, 500, 1000]);
  });
  it("labels the time axis", () => {
    expect(timeLabel(0)).toBe("now");
    expect(timeLabel(15)).toBe("−15 min");
    expect(clock(Date.UTC(2026, 9, 5, 12, 30, 15))).toMatch(/\d{1,2}[:.]\d{2}[:.]\d{2}/);
  });
});

describe("finding the sample under the pointer", () => {
  const l = [s(0), s(10), s(30), s(100)];
  it("picks the nearest in time", () => {
    expect(nearestIndex(l, 0)).toBe(0);
    expect(nearestIndex(l, 4)).toBe(0);
    expect(nearestIndex(l, 6)).toBe(1);
    expect(nearestIndex(l, 21)).toBe(2);
    expect(nearestIndex(l, 20)).toBe(1);
    expect(nearestIndex(l, 99)).toBe(3);
    expect(nearestIndex(l, 5000)).toBe(3);
    expect(nearestIndex(l, -50)).toBe(0);
  });
  it("copes with none and with one", () => {
    expect(nearestIndex([], 5)).toBe(-1);
    expect(nearestIndex([s(7)], 1000)).toBe(0);
  });
});
