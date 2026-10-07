// What to do with a hyperlink a program put in the terminal (OSC 8): the text shown can say one thing and the
// address another, so the address is checked, and shown when it differs from the text.

export type LinkVerdict = { ok: true; confirm: boolean; address: string } | { ok: false; reason: string };

/** Schemes worth opening from a terminal. file: and the like are not: they point at this computer, not the host. */
const ALLOWED = new Set(["http:", "https:", "mailto:"]);

export function judgeLink(uri: string, shownText: string): LinkVerdict {
  if (uri.length > 2000 || /[\x00-\x1f\x7f]/.test(uri)) return { ok: false, reason: "That link isn't usable." };
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return { ok: false, reason: "That link isn't usable." };
  }
  if (!ALLOWED.has(url.protocol)) return { ok: false, reason: `Links that open ${url.protocol} aren't followed from a terminal.` };
  // A link whose text is its own address says what it is; anything else may be hiding where it goes.
  const same = shownText.trim() === uri || shownText.trim() === uri.replace(/\/$/, "") || shownText.trim() === url.href;
  return { ok: true, confirm: !same, address: url.href };
}
