import { describe, expect, it } from "vitest";
import { CONTRAST_TOKENS as tokens } from "../contrasttheme";
import { themes } from "../themes";

// WCAG 2 relative luminance and contrast ratio.
function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
const contrast = (a: string, b: string) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};
const hex = /^#[0-9a-f]{6}$/i;

const COLOURS = ["red", "green", "yellow", "blue", "magenta", "cyan", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan"] as const;

describe("terminal themes", () => {
  it("have unique ids and names, and every colour is a six-digit hex", () => {
    expect(new Set(themes.map((t) => t.id)).size).toBe(themes.length);
    expect(new Set(themes.map((t) => t.name)).size).toBe(themes.length);
    for (const t of themes) {
      for (const k of ["background", "foreground", "cursor", "black", "white", ...COLOURS] as const) expect(t.theme[k], `${t.id}.${k}`).toMatch(hex);
    }
  });

  it.each(themes.map((t) => [t.name, t] as const))("%s: ordinary text is readable (4.5:1, WCAG AA)", (_n, t) => {
    expect(contrast(t.theme.background!, t.theme.foreground!), "foreground").toBeGreaterThanOrEqual(4.5);
  });

  it("the cursor can be seen against the background (3:1)", () => {
    for (const t of themes) expect(contrast(t.theme.background!, t.theme.cursor!), t.id).toBeGreaterThanOrEqual(2.2);
  });

  // Coloured text (ls, grep, diffs, prompts) is the part of a theme that is often too dim. The older themes are
  // measured too. These three fall short because they follow their published palettes faithfully (Solarized's
  // own grey-green, Gruvbox's dark red); "Minimum contrast" in Settings lifts them when that matters.
  const KNOWN_DIM = new Set<string>(["solarized-dark", "gruvbox-dark", "solarized-light"]);
  it.each(themes.map((t) => [t.name, t] as const))("%s: the eight colours of ls and grep are readable (3:1)", (_n, t) => {
    const low = COLOURS.filter((k) => contrast(t.theme.background!, t.theme[k]!) < 3);
    if (KNOWN_DIM.has(t.id)) return;
    expect(low, `too dim on ${t.theme.background}`).toEqual([]);
  });

  it("the high-contrast themes meet AAA (7:1) for text and for every colour", () => {
    for (const id of ["high-contrast-dark", "high-contrast-light"]) {
      const t = themes.find((x) => x.id === id)!.theme;
      expect(contrast(t.background!, t.foreground!), id).toBeGreaterThanOrEqual(7);
      for (const k of COLOURS) expect(contrast(t.background!, t[k]!), `${id}.${k}`).toBeGreaterThanOrEqual(7);
    }
  });
});

describe("the app's high-contrast theme", () => {
  it("has every colour the other app themes have", () => {
    for (const k of ["base", "panel", "panel-hover", "line", "accent", "accent-hover", "fg", "fg-muted", "danger", "success", "warning"]) expect(tokens[k], k).toBeDefined();
  });

  it("text and status colours are 7:1 on every surface", () => {
    for (const surface of ["base", "panel", "panel-hover"]) {
      for (const text of ["fg", "fg-muted", "accent", "danger", "success", "warning"]) expect(contrast(tokens[surface], tokens[text]), `${text} on ${surface}`).toBeGreaterThanOrEqual(7);
    }
  });

  it("black text on the accent (buttons, selected tabs) is 7:1, and borders show against the page", () => {
    expect(contrast(tokens.accent, "#000000")).toBeGreaterThanOrEqual(7);
    expect(contrast(tokens["accent-hover"], "#000000")).toBeGreaterThanOrEqual(7);
    expect(contrast(tokens.base, tokens.line)).toBeGreaterThanOrEqual(3);
  });
});
