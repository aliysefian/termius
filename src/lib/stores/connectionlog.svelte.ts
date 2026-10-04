// A local, per-computer log of connections: when, how long, how it ended.
// Never synced, never leaves this file: see docs/vault-architecture.md for
// what is and isn't kept on disk. Entries carry a host id only when the
// target is a saved host, so they survive a rename but never a label that
// was never saved.
import type { Uuid } from "$lib/types";

export interface ConnectionLogEntry {
  id: string;
  hostId: Uuid | null;
  label: string;
  kind: "host" | "adhoc" | "telnet" | "serial" | "local";
  startedAt: number;
  endedAt: number | null;
  /** Exit code when the remote shell exited; null for a dropped link, a user-closed tab, or no exit code reported. */
  exitCode: number | null;
  reason: "exited" | "dropped" | "closed" | "failed" | null;
}

const KEY = "sshvault.connectionlog.v1";
const MAX_ENTRIES = 1000;

function load(): ConnectionLogEntry[] {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as ConnectionLogEntry[]) : [];
  } catch {
    return [];
  }
}

function persist(entries: ConnectionLogEntry[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(entries));
  } catch {
    // Storage full or unavailable; the in-memory list still works this session.
  }
}

let counter = 0;

class ConnectionLogStore {
  entries = $state<ConnectionLogEntry[]>(load());

  start(hostId: Uuid | null, label: string, kind: ConnectionLogEntry["kind"]): string {
    const id = `cl-${++counter}-${Date.now().toString(36)}`;
    this.entries = [{ id, hostId, label, kind, startedAt: Date.now(), endedAt: null, exitCode: null, reason: null }, ...this.entries].slice(0, MAX_ENTRIES);
    persist($state.snapshot(this.entries));
    return id;
  }

  end(id: string, exitCode: number | null, reason: ConnectionLogEntry["reason"]) {
    const e = this.entries.find((x) => x.id === id);
    if (!e || e.endedAt) return;
    e.endedAt = Date.now();
    e.exitCode = exitCode;
    e.reason = reason;
    persist($state.snapshot(this.entries));
  }

  forHost(hostId: Uuid, limit = 5): ConnectionLogEntry[] {
    return this.entries.filter((e) => e.hostId === hostId).slice(0, limit);
  }

  clear() {
    this.entries = [];
    persist([]);
  }

  toCsv(): string {
    const rows = this.entries.map((e) => [e.label, e.kind, new Date(e.startedAt).toISOString(), e.endedAt ? new Date(e.endedAt).toISOString() : "", e.endedAt ? String(Math.round((e.endedAt - e.startedAt) / 1000)) : "", e.exitCode ?? "", e.reason ?? ""]);
    const esc = (v: string | number) => `"${String(v).replace(/"/g, '""')}"`;
    return [["label", "kind", "started", "ended", "seconds", "exit_code", "reason"], ...rows].map((r) => r.map(esc).join(",")).join("\n");
  }
}

export const connectionLog = new ConnectionLogStore();
