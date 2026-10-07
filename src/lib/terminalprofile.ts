// A host's own terminal look over the settings: a theme, a text size, a scrollback length. Anything not set
// follows the settings, and zooming still works on top of a host's size.
import type { TerminalProfile } from "./types";

export const FONT_MIN = 8;
export const FONT_MAX = 32;
export const SCROLLBACK_MIN = 100;
export const SCROLLBACK_MAX = 100_000;

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n)));

/** What to save: nothing at all when no field is set, numbers kept in range. */
export function cleanProfile(p: TerminalProfile | undefined): TerminalProfile | undefined {
  if (!p) return undefined;
  const out: TerminalProfile = {};
  if (p.theme) out.theme = p.theme;
  if (typeof p.font_size === "number" && Number.isFinite(p.font_size)) out.font_size = clamp(p.font_size, FONT_MIN, FONT_MAX);
  if (typeof p.scrollback === "number" && Number.isFinite(p.scrollback)) out.scrollback = clamp(p.scrollback, SCROLLBACK_MIN, SCROLLBACK_MAX);
  return Object.keys(out).length ? out : undefined;
}

export interface Look {
  themeId: string;
  fontSize: number;
  scrollback: number;
}

/**
 * The look for a pane. `zoom` is how far the user has zoomed from the default size (the settings' size minus
 * `defaultSize`); it is added to a host's own size so Ctrl+scroll keeps working there.
 */
export function effectiveLook(prefs: { themeId: string; fontSize: number; scrollback: number }, profile: TerminalProfile | undefined, defaultSize: number, knownTheme: (id: string) => boolean): Look {
  const p = cleanProfile(profile);
  const zoom = prefs.fontSize - defaultSize;
  return {
    themeId: p?.theme && knownTheme(p.theme) ? p.theme : prefs.themeId,
    fontSize: p?.font_size !== undefined ? clamp(p.font_size + zoom, FONT_MIN, FONT_MAX) : prefs.fontSize,
    scrollback: p?.scrollback ?? prefs.scrollback,
  };
}
