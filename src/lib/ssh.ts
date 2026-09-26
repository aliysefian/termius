// IPC wrapper for the SSH terminal engine (src-tauri/src/ssh.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Uuid } from "./types";

export type SessionStatus =
  | { kind: "connecting" }
  | { kind: "new_host_key"; fingerprint: string }
  | { kind: "connected" }
  | { kind: "disconnected"; code: number | null }
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
