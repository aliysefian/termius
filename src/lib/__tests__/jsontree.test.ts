import { describe, expect, it } from "vitest";
import { jsonMatches, jsonPreview } from "../jsontree";

describe("json tree helpers", () => {
  it("previews the size of a branch", () => {
    expect([jsonPreview({}), jsonPreview([]), jsonPreview({ a: 1, b: 2 }), jsonPreview([1, 2, 3]), jsonPreview(7)]).toEqual(["{}", "[]", "{2}", "[3]", "7"]);
  });

  it("finds a key or a value anywhere below a node, ignoring case", () => {
    const v = { Config: { Env: ["LOG_LEVEL=info"], User: "root" }, Id: "abc123" };
    expect(jsonMatches(v, null, "log_level")).toBe(true);
    expect(jsonMatches(v, null, "ROOT")).toBe(true);
    expect(jsonMatches(v, "Config", "config")).toBe(true);
    expect(jsonMatches(v.Config, "Config", "abc123")).toBe(false);
    expect(jsonMatches(v, null, "")).toBe(true);
    expect(jsonMatches(null, "k", "null")).toBe(true);
  });
});
