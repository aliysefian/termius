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
