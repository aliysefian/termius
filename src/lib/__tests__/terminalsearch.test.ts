import { describe, expect, it } from "vitest";
import { MAX_HITS, compile, registered, registerTerminal, search, type Source } from "../terminalsearch";

const src = (paneId: string, lines: string[], label = paneId): Source => ({ paneId, label, lines: () => lines, reveal: () => {} });

describe("searching every terminal", () => {
  const a = src("a", ["$ ls", "error: disk full", "ok", "ERROR again"], "web-01");
  const b = src("b", ["nothing here", "an Error in b"], "db-01");

  it("finds lines in every terminal, newest first within one, and says where", () => {
    const r = search([a, b], "error");
    expect(r.error).toBeNull();
    expect(r.hits.map((h) => `${h.label}:${h.line}`)).toEqual(["web-01:3", "web-01:1", "db-01:1"]);
    const h = r.hits[2];
    expect(h.text.slice(h.from, h.to)).toBe("Error");
  });

  it("can match case and read a pattern, and plain text is not a pattern", () => {
    expect(search([a, b], "ERROR", { matchCase: true }).hits).toHaveLength(1);
    expect(search([a], "d.sk").hits).toHaveLength(0);
    expect(search([a], "d.sk", { regex: true }).hits).toHaveLength(1);
    expect(search([a], "$ ls").hits).toHaveLength(1);
  });

  it("an empty query finds nothing, and a bad pattern is a message, not a crash", () => {
    expect(search([a], "")).toEqual({ hits: [], truncated: false, error: null });
    expect(search([a], "(", { regex: true }).error).toBeTruthy();
    expect(compile("x".repeat(400)) instanceof Error).toBe(true);
    expect(compile("(", { regex: true }) instanceof Error).toBe(true);
  });

  it("stops at a limit and says so; a match that is empty does not loop", () => {
    const big = src("big", Array.from({ length: MAX_HITS + 50 }, (_, i) => `line ${i} hit`));
    const r = search([big], "hit");
    expect(r.hits).toHaveLength(MAX_HITS);
    expect(r.truncated).toBe(true);
    expect(search([a], "x*", { regex: true }).hits).toEqual([]);
  });

  it("cuts a very long line down around the match", () => {
    const long = src("l", ["a".repeat(500) + "NEEDLE" + "b".repeat(500)]);
    const h = search([long], "needle").hits[0];
    expect(h.text.length).toBeLessThan(260);
    expect(h.text.slice(h.from, h.to)).toBe("NEEDLE");
  });

  it("knows the open terminals as they come and go", () => {
    const off = registerTerminal(a);
    expect(registered().map((s) => s.paneId)).toContain("a");
    off();
    expect(registered().map((s) => s.paneId)).not.toContain("a");
    // A terminal that was replaced can't remove its successor.
    const first = registerTerminal(src("p", ["1"]));
    const second = registerTerminal(src("p", ["2"]));
    first();
    expect(registered().find((s) => s.paneId === "p")?.lines()).toEqual(["2"]);
    second();
  });
});
