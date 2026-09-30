// IPC wrapper for the SSH terminal engine (src-tauri/src/ssh.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Uuid } from "./types";

export type SessionStatus =
  | { kind: "connecting" }
  | { kind: "new_host_key"; host: string; fingerprint: string }
  | { kind: "connected" }
  | { kind: "disconnected"; code: number | null }
  | { kind: "host_key_changed"; host: string; port: number; fingerprint: string }
  | { kind: "error"; message: string };

export interface StatusEvent {
  pane_id: string;
  status: SessionStatus;
}

/** One-off credentials for hosts without a stored identity. Never persisted. */
export interface Credentials {
  username: string;
  password: string;
}

/**
 * A jump host on the way to an unsaved host: a saved host (its own
 * credentials, proxy and jump chain apply), or an unsaved one reached with
 * ssh-agent.
 */
export type AdhocHop = { host_id: Uuid } | { hostname: string; port: number; username: string };

/** An unsaved `user@host:port` connection from quick connect. */
export interface AdhocTarget {
  hostname: string;
  port: number;
  username: string;
  password?: string;
  /** Jump hosts, outermost first (`ssh -J a,b`). */
  jumps?: AdhocHop[];
}

/** Finds the saved host a `-J` hop names (by label, or by address and user). */
export type HopFinder = (name: string, port: number | undefined, user: string | undefined) => Uuid | undefined;

/** `[user@]host[:port]`, with `[v6::addr]` for IPv6. */
function splitAddress(s: string): { user?: string; host: string; port?: number } | null {
  const m = s.match(/^(?:([^@\s]+)@)?(\[[^\]]+\]|[^:@\s\[\]]+)(?::(\d{1,5}))?$/);
  if (!m) return null;
  const port = m[3] ? Number(m[3]) : undefined;
  if (port !== undefined && (port < 1 || port > 65535)) return null;
  return { user: m[1], host: m[2].replace(/^\[|\]$/g, ""), port };
}

/**
 * Parse `user@host[:port]`, or an OpenSSH-style command line such as
 * `ssh -J bastion -p 2222 user@host`. Understands `-J`, `-p`, `-l` and
 * `-o ProxyJump=/Port=/User=`; anything else is refused rather than ignored.
 * A hop without a user must name a saved host; one with a user matches a
 * saved host with that user, or else connects with ssh-agent.
 */
export function parseAdhoc(input: string, findHop?: HopFinder): Omit<AdhocTarget, "password"> | null {
  const tokens = input.trim().split(/\s+/).filter(Boolean);
  if (tokens[0] === "ssh") tokens.shift();
  let jumpSpec: string | undefined;
  let port: number | undefined;
  let user: string | undefined;
  let dest: string | undefined;
  const option = (kv: string) => {
    const m = kv.match(/^(\w+)=(.+)$/);
    if (!m) return false;
    const key = m[1].toLowerCase();
    if (key === "proxyjump") jumpSpec = m[2];
    else if (key === "port") port = Number(m[2]);
    else if (key === "user") user = m[2];
    else return false;
    return true;
  };
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    const flag = t.match(/^-([Jplo])(.*)$/);
    if (flag) {
      const value = flag[2] || tokens[++i];
      if (!value) return null;
      if (flag[1] === "J") jumpSpec = value;
      else if (flag[1] === "p") port = Number(value);
      else if (flag[1] === "l") user = value;
      else if (!option(value)) return null;
    } else if (t.startsWith("-") || dest !== undefined) {
      return null;
    } else {
      dest = t;
    }
  }
  const d = dest === undefined ? null : splitAddress(dest);
  if (!d) return null;
  const username = d.user ?? user;
  port = d.port ?? port ?? 22;
  if (!username || !Number.isInteger(port) || port < 1 || port > 65535) return null;
  const out: Omit<AdhocTarget, "password"> = { username, hostname: d.host, port };
  if (jumpSpec && jumpSpec.toLowerCase() !== "none") {
    const jumps: AdhocHop[] = [];
    for (const spec of jumpSpec.split(",")) {
      const h = splitAddress(spec);
      if (!h) return null;
      const saved = findHop?.(h.host, h.port, h.user);
      if (saved) jumps.push({ host_id: saved });
      else if (h.user) jumps.push({ hostname: h.host, port: h.port ?? 22, username: h.user });
      else return null;
    }
    out.jumps = jumps;
  }
  return out;
}

const encoder = new TextEncoder();

export const ssh = {
  connect(
    paneId: string,
    hostId: Uuid,
    cols: number,
    rows: number,
    credentials: Credentials | null,
    onData: (bytes: Uint8Array) => void,
  ): Promise<void> {
    const channel = new Channel<ArrayBuffer | number[]>((msg) => {
      // Small payloads arrive as ArrayBuffer; very large ones may arrive via
      // the fetch path as a plain array. Normalise both.
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
    });
    return invoke<void>("ssh_connect", { paneId, hostId, cols, rows, credentials, onData: channel });
  },

  /** Connect a pane to an unsaved host. No password means use ssh-agent. */
  connectAdhoc(
    paneId: string,
    target: AdhocTarget,
    cols: number,
    rows: number,
    onData: (bytes: Uint8Array) => void,
  ): Promise<void> {
    const channel = new Channel<ArrayBuffer | number[]>((msg) => {
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
    });
    return invoke<void>("ssh_connect_adhoc", {
      paneId,
      hostname: target.hostname,
      port: target.port,
      username: target.username,
      password: target.password ?? null,
      jumps: target.jumps ?? [],
      cols,
      rows,
      onData: channel,
    });
  },

  write(paneId: string, data: string | Uint8Array): Promise<void> {
    const bytes = typeof data === "string" ? encoder.encode(data) : data;
    return invoke<void>("ssh_write", { paneId, data: Array.from(bytes) });
  },

  resize(paneId: string, cols: number, rows: number): Promise<void> {
    return invoke<void>("ssh_resize", { paneId, cols, rows });
  },

  disconnect(paneId: string): Promise<void> {
    return invoke<void>("ssh_disconnect", { paneId });
  },

  onStatus(handler: (e: StatusEvent) => void): Promise<UnlistenFn> {
    return listen<StatusEvent>("ssh:status", (e) => handler(e.payload));
  },
};
