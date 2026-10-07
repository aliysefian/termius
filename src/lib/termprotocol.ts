// Terminal protocol helpers for things xterm.js leaves to the embedder.
// Pure functions, so they can be unit tested without a terminal.

/**
 * Decode an OSC 52 clipboard request ("Pc;Pd").
 *
 * tmux (mouse selection, `set-clipboard on`), Claude Code, Neovim and
 * others copy by sending this; xterm.js itself ignores it. Returns the text
 * to put on the clipboard, or null for queries ("?"), clears (empty data)
 * and anything malformed. Queries are never answered: that would hand the
 * clipboard to the remote host.
 */
export function decodeOsc52(data: string): string | null {
  const sep = data.indexOf(";");
  if (sep < 0) return null;
  const payload = data.slice(sep + 1).replace(/\s+/g, "");
  if (!payload || payload === "?") return null;
  if (!/^[A-Za-z0-9+/]+=*$/.test(payload)) return null;
  try {
    const bin = atob(payload);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
  } catch {
    return null;
  }
}

export type MouseTrackingMode = "none" | "x10" | "vt200" | "drag" | "any";

const MOUSE_MODE_PARAMS: Record<number, MouseTrackingMode> = {
  9: "x10",
  1000: "vt200",
  1002: "drag",
  1003: "any",
};

/**
 * True when a DECSET (`CSI ? Pm h`) only re-enables the mouse tracking mode
 * that is already active.
 *
 * xterm.js fires its "protocol changed" event even when nothing changed,
 * and that event clears the selection and stops the drag in progress.
 * Programs that redraw often (Claude Code, tmux on pane changes) re-send
 * the mode and make selecting text with Shift or select mode impossible.
 * Such sequences can be swallowed: they would have no effect anyway.
 */
export function redundantMouseEnable(params: (number | number[])[], current: MouseTrackingMode): boolean {
  if (params.length === 0) return false;
  return params.every((p) => typeof p === "number" && MOUSE_MODE_PARAMS[p] !== undefined && MOUSE_MODE_PARAMS[p] === current);
}

// -- mouse reports -----------------------------------------------------------------
//
// With a mouse mode on, xterm.js turns every move, press and wheel turn into a report that goes to the
// remote side as keyboard input. If the program that asked for them has gone (it crashed, the
// connection dropped, a `kill -9`), the shell is left reading them as typed text: `35;2;16M35;6;15M…`.
// So reports are only sent while a mode is actually on, and every end of a session switches the modes off.

/** Turn off everything a full-screen program may have switched on, for a session that has ended or is starting. */
export const RESET_SESSION_MODES =
  // mouse: X10, normal, drag, any-motion, UTF-8, SGR, urxvt, focus events
  "\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1005l\x1b[?1006l\x1b[?1015l" +
  // bracketed paste, application cursor keys, the alternate screens, keypad, the cursor shown again
  "\x1b[?2004l\x1b[?1l\x1b[?47l\x1b[?1047l\x1b[?1049l\x1b>\x1b[?25h";

/** Every mouse mode off. Also what a shell prompt means: no program is asking for the mouse any more. */
export const MOUSE_OFF = "\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l";

/** What a soft terminal reset (DECSTR, `CSI ! p`) must also turn off, which xterm.js leaves on. */
export const RESET_INPUT_MODES = MOUSE_OFF + "\x1b[?2004l\x1b[?1l";

const SGR_REPORT = /^(?:\x1b\[<(\d+);\d+;\d+[Mm])+$/;
const URXVT_REPORT = /^(?:\x1b\[(\d+);\d+;\d+M)+$/;
const LEGACY_REPORT = /^(?:\x1b\[M[\s\S]{3})+$/;

/**
 * The button code of the first mouse report in `data`, or null if `data` isn't made of mouse reports.
 * (SGR and legacy codes: 32 added for motion, 64 for the wheel; urxvt adds 32 to all of them.)
 */
function mouseCode(data: string): number | null {
  if (!data.startsWith("\x1b[")) return null;
  const sgr = SGR_REPORT.exec(data);
  if (sgr) return Number(/^\x1b\[<(\d+)/.exec(data)![1]);
  if (URXVT_REPORT.test(data)) return Number(/^\x1b\[(\d+)/.exec(data)![1]) - 32;
  if (LEGACY_REPORT.test(data)) return data.charCodeAt(3) - 32;
  return null;
}

export const isMouseReport = (data: string): boolean => mouseCode(data) !== null;

/**
 * Whether `data` may be sent to the remote side. Anything that isn't a mouse report always may. A report
 * may only while a mouse mode is on; motion with a button held needs drag or any-motion tracking, and
 * motion with no button (hover, code 35) only any-motion (1003). A report is dropped whole, never cut,
 * so a half of one can't reach the shell.
 */
export function mouseReportAllowed(data: string, mode: MouseTrackingMode): boolean {
  const code = mouseCode(data);
  if (code === null) return true;
  if (mode === "none") return false;
  const motion = (code & 32) !== 0 && (code & 64) === 0;
  const hover = motion && (code & 3) === 3;
  if (hover) return mode === "any";
  if (motion) return mode === "drag" || mode === "any";
  return true;
}
