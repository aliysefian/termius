import { describe, expect, it } from "vitest";
import { blockAt, formatDuration, nextMatching, summary, trim, visibleBlocks, type Block } from "../commandblocks";

const b = (id: number, start: number, end: number, exit: number | null = 0, pinned = false): Block => ({ id, command: `cmd${id}`, exit, durationMs: 1500, pinned, start, end });

describe("command blocks", () => {
  const list = [b(1, 0, 4), b(2, 5, 9, 1), b(3, 10, 12), b(4, 13, 20, 127, true)];
  it("formats durations", () => {
    expect(formatDuration(40)).toBe("40 ms");
    expect(formatDuration(1500)).toBe("1.5 s");
    expect(formatDuration(42000)).toBe("42 s");
    expect(formatDuration(125000)).toBe("2 min 5 s");
    expect(formatDuration(3_900_000)).toBe("1 h 5 min");
  });
  it("tells the outcome", () => {
    expect(summary(b(2, 0, 1, 1))).toContain("failed (exit 1)");
    expect(summary(b(1, 0, 1))).toContain("succeeded");
    expect(summary(b(1, 0, 1, null))).toContain("finished");
  });
  it("finds the block at a line", () => {
    expect(blockAt(list, 7)?.id).toBe(2);
    expect(blockAt(list, 12)?.id).toBe(3);
    expect(blockAt(list, 99)).toBeNull();
  });
  it("jumps to the previous or next failed or pinned block", () => {
    expect(nextMatching(list, 14, -1)?.id).toBe(4);
    expect(nextMatching(list, 13, -1)?.id).toBe(2);
    expect(nextMatching(list, 14, 1)).toBeNull();
    expect(nextMatching(list, 0, 1)?.id).toBe(2);
    expect(nextMatching(list, 9, 1)?.id).toBe(4);
    expect(nextMatching(list, 13, -1, "pinned")).toBeNull();
    expect(nextMatching(list, 30, -1, "pinned")?.id).toBe(4);
    expect(nextMatching(list, 0, 1, "pinned")?.id).toBe(4);
  });
  it("shows only blocks that touch the screen", () => {
    expect(visibleBlocks(list, 8, 4).map((x) => x.id)).toEqual([2, 3]);
  });
  it("trims the oldest, never a pinned one", () => {
    const many = [b(1, 0, 1, 0, true), b(2, 2, 3), b(3, 4, 5), b(4, 6, 7)];
    expect(trim(many, 2).map((x) => x.id)).toEqual([1, 4]);
    expect(trim(many, 10)).toBe(many);
  });
});
