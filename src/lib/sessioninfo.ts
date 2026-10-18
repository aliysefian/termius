// What the status bar can truthfully say about the focused pane. Each fact is shown only where it holds:
// a Telnet or serial line has no TERM we negotiated, an RDP pane has no text encoding, and latency is only
// known for a saved host the reachability probe has measured.

import type { Health, Host } from "$lib/types";

export type SessionTarget =
  | { kind: "host" }
  | { kind: "adhoc" }
  | { kind: "telnet" }
  | { kind: "serial" }
  | { kind: "local" };

export interface SessionFacts {
  /** "SSH", "Mosh", "Telnet", "Serial", "Local", "RDP", "VNC" or "FTP". */
  protocol: string;
  /** The TERM we ask the remote for, only where we do (SSH and local shells). */
  term: string | null;
  /** The terminal is always UTF-8 (xterm.js); absent for graphical sessions. */
  encoding: string | null;
  /** Round trip of the last reachability probe, in ms. */
  latencyMs: number | null;
}

/** The TERM both the SSH pty request and the local pty set (src-tauri/src/ssh.rs, localpty.rs). */
export const TERM = "xterm-256color";

const GRAPHICAL = new Set(["rdp", "vnc"]);

export function sessionFacts(target: SessionTarget, host: Pick<Host, "protocol" | "mosh"> | undefined, health: Health | undefined): SessionFacts {
  let protocol: string;
  switch (target.kind) {
    case "local":
      protocol = "Local";
      break;
    case "telnet":
      protocol = "Telnet";
      break;
    case "serial":
      protocol = "Serial";
      break;
    case "adhoc":
      protocol = "SSH";
      break;
    case "host": {
      const p = host?.protocol ?? "";
      protocol = p === "telnet" ? "Telnet" : GRAPHICAL.has(p) || p === "ftp" ? p.toUpperCase() : host?.mosh ? "Mosh" : "SSH";
      break;
    }
  }
  const text = !GRAPHICAL.has(protocol.toLowerCase());
  return {
    protocol,
    term: protocol === "SSH" || protocol === "Local" ? TERM : null,
    encoding: text ? "UTF-8" : null,
    latencyMs: target.kind === "host" && health?.state === "up" ? health.latency_ms : null,
  };
}
