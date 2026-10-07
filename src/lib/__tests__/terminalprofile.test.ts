import { describe, expect, it } from "vitest";
import { cleanProfile, effectiveLook } from "../terminalprofile";

const prefs = { themeId: "default", fontSize: 14, scrollback: 5000 };
const known = (id: string) => ["default", "dracula"].includes(id);

describe("a host's terminal profile", () => {
  it("saves nothing when nothing is set, and keeps numbers in range", () => {
    expect(cleanProfile(undefined)).toBeUndefined();
    expect(cleanProfile({})).toBeUndefined();
    expect(cleanProfile({ theme: "" })).toBeUndefined();
    expect(cleanProfile({ theme: "dracula", font_size: 99, scrollback: 5 })).toEqual({ theme: "dracula", font_size: 32, scrollback: 100 });
    expect(cleanProfile({ font_size: Number.NaN })).toBeUndefined();
  });
  it("follows the settings for what is not set", () => {
    expect(effectiveLook(prefs, undefined, 14, known)).toEqual({ themeId: "default", fontSize: 14, scrollback: 5000 });
    expect(effectiveLook(prefs, { theme: "dracula" }, 14, known)).toEqual({ themeId: "dracula", fontSize: 14, scrollback: 5000 });
  });
  it("a host's size and scrollback win, and zoom still moves its size", () => {
    expect(effectiveLook(prefs, { font_size: 18, scrollback: 200 }, 14, known)).toEqual({ themeId: "default", fontSize: 18, scrollback: 200 });
    expect(effectiveLook({ ...prefs, fontSize: 16 }, { font_size: 18 }, 14, known).fontSize).toBe(20);
    expect(effectiveLook({ ...prefs, fontSize: 40 }, { font_size: 18 }, 14, known).fontSize).toBe(32);
  });
  it("a theme that no longer exists falls back to the settings", () => {
    expect(effectiveLook(prefs, { theme: "deleted-custom" }, 14, known).themeId).toBe("default");
  });
});
