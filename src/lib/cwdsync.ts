// Keeping a terminal and the file browser in the same folder: the command that takes a terminal to a folder, the
// rule for when the browser follows the terminal, and the line that makes a shell report its folder.

import type { Uuid } from "$lib/types";

/** A word for a POSIX shell: in single quotes, with each `'` inside as `'\''`. */
export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** The line that takes a shell to `path`. A Windows shell gets double quotes (a path there cannot hold one). */
export function cdCommand(path: string, windows = false): string {
  return windows ? `cd "${path}"` : `cd ${shellQuote(path)}`;
}

export interface FollowInput {
  /** The browser is following the terminal. */
  follow: boolean;
  /** What the browser pane shows: a host's id, `"local"` for this computer, or null for nothing. */
  paneHostId: Uuid | "local" | null;
  /** What the terminal in front is on: a host's id, `"local"` for a shell on this computer, or null. */
  terminalHostId: Uuid | "local" | null;
  /** The folder the terminal reported, if it has. */
  cwd: string | undefined;
  /** Where the browser is now. */
  current: string;
}

/** The folder the browser should go to, or null to stay. Only a terminal on the same host (or both on this computer) is followed. */
export function followedPath(i: FollowInput): string | null {
  if (!i.follow || !i.cwd || i.paneHostId === null || i.terminalHostId !== i.paneHostId) return null;
  const norm = (p: string) => (p.length > 1 ? p.replace(/\/+$/, "") : p);
  return norm(i.cwd) === norm(i.current) ? null : i.cwd;
}

/** Makes bash and zsh report the folder after every command (OSC 7), which is what the browser follows. */
export const OSC7_BASH_ZSH = `__sv_cwd() { printf '\\033]7;file://%s%s\\033\\\\' "\${HOSTNAME:-$(hostname)}" "$PWD"; }; if [ -n "$ZSH_VERSION" ]; then precmd_functions+=(__sv_cwd); else PROMPT_COMMAND="__sv_cwd\${PROMPT_COMMAND:+;$PROMPT_COMMAND}"; fi`;

/** The same for fish. */
export const OSC7_FISH = `function __sv_cwd --on-variable PWD; printf '\\033]7;file://%s%s\\033\\\\' (hostname) $PWD; end; __sv_cwd`;
