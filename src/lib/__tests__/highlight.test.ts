import { describe, expect, it } from "vitest";
import { MAX_LINE, PRESETS, matchLine, problem, sanitize, triggered, withId, type HighlightRule } from "../highlight";

const rule = (o: Partial<HighlightRule>): HighlightRule => ({ id: "x", name: "", pattern: "err", regex: false, matchCase: false, color: "#112233", enabled: true, notify: false, ...o });

describe("highlight rules", () => {
  it("finds every match of plain text, ignoring case unless asked", () => {
    expect(matchLine([rule({})], "Err and err").map((m) => [m.from, m.to])).toEqual([[0, 3], [8, 11]]);
    expect(matchLine([rule({ matchCase: true })], "Err and err")).toHaveLength(1);
  });
  it("plain text is not a pattern", () => {
    expect(matchLine([rule({ pattern: "a.c" })], "abc a.c")).toHaveLength(1);
    expect(matchLine([rule({ pattern: "a.c", regex: true })], "abc a.c")).toHaveLength(2);
  });
  it("an earlier rule wins where two overlap, disabled ones are skipped", () => {
    const a = rule({ id: "a", pattern: "error" });
    const b = rule({ id: "b", pattern: "rro" });
    const m = matchLine([a, b], "an error");
    expect(m).toHaveLength(1);
    expect(m[0].rule.id).toBe("a");
    expect(matchLine([rule({ enabled: false })], "err")).toEqual([]);
  });
  it("refuses patterns that match nothing in particular or don't compile, and bad colours", () => {
    expect(problem(rule({ pattern: "a*", regex: true }))).toBeTruthy();
    expect(problem(rule({ pattern: "(", regex: true }))).toBeTruthy();
    expect(problem(rule({ pattern: "" }))).toBeTruthy();
    expect(problem(rule({ color: "red" }))).toBeTruthy();
    expect(problem(rule({}))).toBeNull();
    expect(matchLine([rule({ pattern: "(", regex: true })], "(")).toEqual([]);
  });
  it("does not scan huge lines", () => {
    expect(matchLine([rule({})], "err".repeat(MAX_LINE))).toEqual([]);
  });
  it("only rules that ask for notices trigger them", () => {
    const n = rule({ id: "n", pattern: "panic", notify: true });
    expect(triggered([rule({}), n], "err panic panic").map((r) => r.id)).toEqual(["n"]);
    expect(triggered([rule({})], "err")).toEqual([]);
  });
  it("the presets all compile and find what they are for", () => {
    const rules = PRESETS.map(withId);
    for (const r of rules) expect(problem(r)).toBeNull();
    expect(matchLine(rules, "connection refused from 10.0.0.12").map((m) => m.rule.name).sort()).toEqual(["Errors", "IP addresses"]);
  });
  it("keeps only well-formed saved rules", () => {
    const out = sanitize([{ pattern: "a", color: "#abcdef" }, { pattern: 3 }, null, { pattern: "b", color: "nope" }]);
    expect(out.map((r) => r.pattern)).toEqual(["a", "b"]);
    expect(out[1].color).toBe("#444444");
    expect(sanitize("x")).toEqual([]);
  });
});
