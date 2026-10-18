import { describe, expect, it } from "vitest";
import { diffLines, hunks, MAX_D, splitRows } from "$lib/diff";

const kinds = (a: string, b: string) => diffLines(a, b).lines.map((l) => (l.kind === "same" ? " " : l.kind === "add" ? "+" : "-") + l.text);

describe("diffLines", () => {
  it("identical text has no changes", () => {
    const d = diffLines("a\nb\nc\n", "a\nb\nc\n");
    expect(d.added + d.removed).toBe(0);
    expect(d.lines.every((l) => l.kind === "same")).toBe(true);
  });

  it("finds an inserted, a removed and a replaced line", () => {
    expect(kinds("a\nc\n", "a\nb\nc\n")).toEqual([" a", "+b", " c", " "]);
    expect(kinds("a\nb\nc", "a\nc")).toEqual([" a", "-b", " c"]);
    expect(kinds("listen 80;\nroot /a;", "listen 8080;\nroot /a;")).toEqual(["-listen 80;", "+listen 8080;", " root /a;"]);
  });

  it("counts and numbers the lines on each side", () => {
    const d = diffLines("one\ntwo\nthree", "one\n2\nthree\nfour");
    expect(d.added).toBe(2);
    expect(d.removed).toBe(1);
    expect(d.lines.map((l) => [l.oldNo, l.newNo])).toEqual([
      [1, 1],
      [2, undefined],
      [undefined, 2],
      [3, 3],
      [undefined, 4],
    ]);
  });

  it("handles an empty side, and CRLF the same as LF", () => {
    expect(diffLines("", "a\nb").added).toBe(2);
    expect(diffLines("a\nb", "").removed).toBe(2);
    expect(diffLines("", "").lines).toEqual([]);
    expect(diffLines("a\r\nb\r\n", "a\nb\n").added + diffLines("a\r\nb\r\n", "a\nb\n").removed).toBe(0);
  });

  it("the old text plus the additions minus the removals is always the new text", () => {
    const a = Array.from({ length: 300 }, (_, i) => `line ${i}`);
    const b = a.filter((_, i) => i % 7 !== 0).map((l, i) => (i % 13 === 0 ? l + " changed" : l));
    b.splice(40, 0, "new one", "new two");
    const d = diffLines(a.join("\n"), b.join("\n"));
    expect(d.lines.filter((l) => l.kind !== "add").map((l) => l.text)).toEqual(a);
    expect(d.lines.filter((l) => l.kind !== "del").map((l) => l.text)).toEqual(b);
    expect(d.tooDifferent).toBe(false);
  });

  it("is quick on a large file with a few changes", () => {
    const a = Array.from({ length: 50_000 }, (_, i) => `row ${i} ${"x".repeat(30)}`);
    const b = [...a];
    b[100] = "changed";
    b.splice(30_000, 5);
    b.push("added at the end");
    const t = performance.now();
    const d = diffLines(a.join("\n"), b.join("\n"));
    expect(performance.now() - t).toBeLessThan(1500);
    expect(d.removed).toBe(6);
    expect(d.added).toBe(2);
  });

  it("gives up on files that differ in too many places instead of freezing", () => {
    const a = Array.from({ length: 6000 }, (_, i) => `a${i}`).join("\n");
    const b = Array.from({ length: 6000 }, (_, i) => `b${i}`).join("\n");
    const t = performance.now();
    const d = diffLines(a, b);
    expect(performance.now() - t).toBeLessThan(3000);
    expect(d.tooDifferent).toBe(true);
    expect(d.removed).toBe(6000);
    expect(d.added).toBe(6000);
    expect(MAX_D).toBeLessThan(12000);
  });
});

describe("hunks", () => {
  it("keeps context around changes and folds the rest", () => {
    const a = Array.from({ length: 40 }, (_, i) => `l${i}`);
    const b = [...a];
    b[20] = "CHANGED";
    const h = hunks(diffLines(a.join("\n"), b.join("\n")).lines, 2);
    expect(h.map((x) => x.type)).toEqual(["fold", "lines", "fold"]);
    expect(h[0]).toEqual({ type: "fold", count: 18 });
    const mid = h[1];
    expect(mid.type === "lines" && mid.lines.map((l) => l.text)).toEqual(["l18", "l19", "l20", "CHANGED", "l21", "l22"]);
    expect(h[2]).toEqual({ type: "fold", count: 17 });
  });

  it("is one block of lines when everything is near a change", () => {
    expect(hunks(diffLines("a\nb\nc", "a\nB\nc").lines, 3)).toHaveLength(1);
  });
});

describe("splitRows", () => {
  it("puts a replacement beside what it replaced and pads the shorter side", () => {
    const rows = splitRows(diffLines("a\nb\nc\nd", "a\nB1\nB2\nd").lines);
    expect(rows.map((r) => [r.left?.text, r.right?.text])).toEqual([
      ["a", "a"],
      ["b", "B1"],
      ["c", "B2"],
      ["d", "d"],
    ]);
    const rows2 = splitRows(diffLines("a\nb\nc", "a").lines);
    expect(rows2.map((r) => [r.left?.text, r.right?.text])).toEqual([
      ["a", "a"],
      ["b", undefined],
      ["c", undefined],
    ]);
  });
});
