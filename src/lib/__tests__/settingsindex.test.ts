import { describe, expect, it } from "vitest";
import panel from "../components/SettingsPanel.svelte?raw";
import { SETTING_ENTRIES } from "../settingsindex";
import { fuzzyScore } from "../fuzzy";

// The keyword strings the Settings page filters sections by, read from its source.
const keywordStrings = [...panel.matchAll(/visible\("[a-z]+", ("[^"]+"|[A-Z_]+)\)/g)].map((m) => m[1]);
const constants = Object.fromEntries([...panel.matchAll(/const ([A-Z_]+) =\s*"([^"]+)"/g)].map((m) => [m[1], m[2]]));
const sections = keywordStrings.map((k) => (k.startsWith('"') ? k.slice(1, -1) : (constants[k] ?? ""))).map((s) => s.toLowerCase());

describe("settings found from the palette", () => {
  it("reads the page's own keyword lists", () => {
    expect(sections.length).toBeGreaterThan(10);
    expect(sections.every((s) => s.length > 5)).toBe(true);
  });

  it.each(SETTING_ENTRIES.map((e) => [e.label, e.query]))("%s: its search word finds a section", (_label, query) => {
    expect(sections.some((s) => s.includes(query.toLowerCase())), `"${query}" matches no section of the Settings page`).toBe(true);
  });

  it("has no two entries searching for the same thing", () => {
    const q = SETTING_ENTRIES.map((e) => e.query);
    expect(new Set(q).size).toBe(q.length);
  });

  it("is found by typing roughly what it is called", () => {
    const find = (typed: string) => SETTING_ENTRIES.filter((e) => fuzzyScore(typed, `Setting: ${e.label} ${e.words ?? ""}`) !== null).map((e) => e.query);
    expect(find("autolock")).toContain("auto-lock");
    expect(find("font")).toContain("font");
    expect(find("quiet hours")).toContain("alerts");
    expect(find("screen reader")).toContain("screen reader");
  });
});
