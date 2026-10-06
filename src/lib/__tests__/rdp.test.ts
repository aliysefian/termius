import { describe, expect, it } from "vitest";
import { decodeMessage, fingerprintLines, formatFingerprint, remotePoint, usableSize, wheelUnits } from "../rdp";

const u16 = (n: number) => [n & 0xff, n >> 8];
const bytes = (...parts: (number | number[])[]) => Uint8Array.from(parts.flat());

describe("decodeMessage", () => {
  it("reads a frame", () => {
    const m = decodeMessage(bytes(0, u16(5), u16(6), u16(2), u16(1), [1, 2, 3, 255, 4, 5, 6, 255]));
    expect(m).toMatchObject({ kind: "frame", x: 5, y: 6, width: 2, height: 1 });
    expect(m?.kind === "frame" && Array.from(m.rgba)).toEqual([1, 2, 3, 255, 4, 5, 6, 255]);
  });
  it("refuses a frame whose pixels don't match its size", () => {
    expect(decodeMessage(bytes(0, u16(0), u16(0), u16(2), u16(2), [1, 2, 3, 4]))).toBeNull();
    expect(decodeMessage(bytes(0, u16(0), u16(0), u16(0), u16(1)))).toBeNull();
    expect(decodeMessage(bytes(0, 1, 2, 3))).toBeNull();
  });
  it("reads sizes, and refuses zero", () => {
    expect(decodeMessage(bytes(1, u16(1024), u16(700)))).toEqual({ kind: "size", width: 1024, height: 700 });
    expect(decodeMessage(bytes(1, u16(0), u16(700)))).toBeNull();
    expect(decodeMessage(bytes(1, u16(10)))).toBeNull();
  });
  it("reads cursors", () => {
    expect(decodeMessage(bytes(2))).toEqual({ kind: "cursor-default" });
    expect(decodeMessage(bytes(3))).toEqual({ kind: "cursor-hidden" });
    const c = decodeMessage(bytes(4, u16(1), u16(2), u16(1), u16(1), [9, 8, 7, 6]));
    expect(c).toMatchObject({ kind: "cursor", hotX: 1, hotY: 2, width: 1, height: 1 });
  });
  it("reads clipboard text in UTF-8", () => {
    expect(decodeMessage(Uint8Array.from([5, ...new TextEncoder().encode("héllo 東京")]))).toEqual({ kind: "clipboard", text: "héllo 東京" });
    expect(decodeMessage(bytes(5))).toEqual({ kind: "clipboard", text: "" });
  });
  it("tells a clean end from a failure", () => {
    expect(decodeMessage(bytes(6))).toEqual({ kind: "ended", error: null });
    expect(decodeMessage(Uint8Array.from([6, ...new TextEncoder().encode("boom")]))).toEqual({ kind: "ended", error: "boom" });
  });
  it("ignores nonsense", () => {
    expect(decodeMessage(new Uint8Array())).toBeNull();
    expect(decodeMessage(bytes(99, 1, 2))).toBeNull();
  });
  it("copes with a message that is a view into a larger buffer", () => {
    const whole = new Uint8Array([7, 7, ...bytes(1, u16(640), u16(480))]);
    expect(decodeMessage(whole.subarray(2))).toEqual({ kind: "size", width: 640, height: 480 });
  });
});

describe("remotePoint", () => {
  const box = { left: 100, top: 50, width: 512, height: 350 };
  it("scales the pointer to the remote screen", () => {
    expect(remotePoint(100, 50, box, { width: 1024, height: 700 })).toEqual({ x: 0, y: 0 });
    expect(remotePoint(356, 225, box, { width: 1024, height: 700 })).toEqual({ x: 512, y: 350 });
  });
  it("keeps the pointer on the screen when it leaves the picture", () => {
    expect(remotePoint(0, 0, box, { width: 1024, height: 700 })).toEqual({ x: 0, y: 0 });
    expect(remotePoint(9999, 9999, box, { width: 1024, height: 700 })).toEqual({ x: 1023, y: 699 });
  });
  it("survives an empty box", () => {
    expect(remotePoint(5, 5, { left: 0, top: 0, width: 0, height: 0 }, { width: 10, height: 10 })).toEqual({ x: 0, y: 0 });
  });
});

describe("wheelUnits", () => {
  it("flips the direction and scales a notch to about 120", () => {
    expect(wheelUnits(100, 0)).toBe(-120);
    expect(wheelUnits(-100, 0)).toBe(120);
    expect(wheelUnits(3, 1)).toBe(-120);
    expect(wheelUnits(1, 2)).toBe(-360);
  });
  it("always moves at least a step, and never overflows", () => {
    expect(wheelUnits(0.1, 0)).toBe(-1);
    expect(wheelUnits(-0.1, 0)).toBe(1);
    expect(wheelUnits(0, 0)).toBe(0);
    expect(wheelUnits(Number.NaN, 0)).toBe(0);
    expect(wheelUnits(1e9, 0)).toBe(-32768);
    expect(wheelUnits(-1e9, 0)).toBe(32767);
  });
});

describe("sizes and fingerprints", () => {
  it("keeps sizes inside the server's limits and even", () => {
    expect(usableSize(1, 1)).toEqual({ width: 200, height: 200 });
    expect(usableSize(1025, 701)).toEqual({ width: 1024, height: 700 });
    expect(usableSize(99999, 99999)).toEqual({ width: 8192, height: 8192 });
  });
  it("shows a fingerprint in pairs", () => {
    expect(formatFingerprint("c82ec0f2")).toBe("c8:2e:c0:f2");
    expect(formatFingerprint("")).toBe("");
    expect(fingerprintLines("00".repeat(32)).split("\n")).toHaveLength(4);
    expect(fingerprintLines("aabbcc")).toBe("aa:bb:cc");
  });
});
