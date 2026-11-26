// "Why is this running?": the types the host answers with, and the wording and rules the drawer shows them by. Nothing
// here talks to the host; the answer is read by `api.hostSamples.investigate`.
import { formatUptime } from "./hostmetrics";

export type Protocol = "tcp" | "udp";
export type InvestigateTarget = { type: "pid"; pid: number } | { type: "port"; port: number; protocol: Protocol };

export interface ProcInfo {
  pid: number;
  ppid: number | null;
  start_ticks: number | null;
  started_ms: number | null;
  name: string;
  /** Can hold secrets typed on a command line: shown, never logged. */
  command: string | null;
  owner: string | null;
  uid: number | null;
  state: string | null;
  unit: string | null;
}

export type AncestryEnd =
  | { kind: "root" }
  | { kind: "parent_gone"; pid: number }
  | { kind: "parent_denied"; pid: number }
  | { kind: "parent_reused"; pid: number }
  | { kind: "cycle"; pid: number }
  | { kind: "depth_limit" }
  | { kind: "unread"; pid: number };

export interface ServiceAttribution {
  unit_name: string;
  unit_type: "service" | "scope";
  state: string | null;
  main_pid: number | null;
  evidence: string[];
  confidence: "confirmed" | "inferred";
}

export interface Subject {
  process: ProcInfo;
  /** Root first, the process itself last. */
  ancestry: ProcInfo[];
  ancestry_end: AncestryEnd;
  service: ServiceAttribution | null;
}

export interface SocketInfo {
  protocol: Protocol;
  state: string;
  address: string;
  port: number;
  ipv6: boolean;
  local_only: boolean;
  owners: { pid: number; name: string }[];
  uid: number | null;
}

export type InvestigateStatus = "found" | "process_not_found" | "port_not_found" | "unsupported_os";

export interface Investigation {
  host_id: string;
  target: { type: "pid"; pid: number } | { type: "port"; port: number; protocol: Protocol };
  status: InvestigateStatus;
  taken_at_ms: number;
  os: string | null;
  sockets: SocketInfo[];
  subjects: Subject[];
  warnings: { code: string; message: string }[];
}

/** The target as the user would say it: `PID 4231`, `TCP :8080`. */
export function targetText(t: InvestigateTarget): string {
  return t.type === "pid" ? `PID ${t.pid}` : `${t.protocol.toUpperCase()} :${t.port}`;
}

/** A whole number in range, or why not. The host checks again; this only saves a round trip. */
export function parseTargetInput(kind: "pid" | "port", text: string, protocol: Protocol): { target: InvestigateTarget } | { error: string } {
  const t = text.trim();
  if (!/^\d{1,10}$/.test(t)) return { error: kind === "pid" ? "A process id is a whole number, like 4231." : "A port is a whole number from 1 to 65535." };
  const n = Number(t);
  if (kind === "pid") return n >= 1 && n <= 4_294_967_295 ? { target: { type: "pid", pid: n } } : { error: "A process id is a whole number from 1 up." };
  return n >= 1 && n <= 65535 ? { target: { type: "port", port: n, protocol } } : { error: "A port is a whole number from 1 to 65535." };
}

export interface EmptyState {
  title: string;
  text: string;
}

/** What to say when there is nothing to show, or `null` when there is a result. */
export function emptyState(r: Investigation): EmptyState | null {
  switch (r.status) {
    case "unsupported_os":
      return { title: "Unsupported host", text: `This reads Linux hosts only${r.os ? `; this one reports ${r.os}` : ""}.` };
    case "process_not_found":
      return r.warnings.some((w) => w.code === "process_denied")
        ? { title: "Insufficient permissions", text: "That process exists, but the account used for this connection may not read it." }
        : { title: "Process not found", text: "That process no longer exists. It may have ended since the list was read." };
    case "port_not_found":
      return r.warnings.some((w) => w.code === "ss_missing")
        ? { title: "Cannot list sockets", text: "`ss` is not installed on this host, so its listening sockets cannot be read." }
        : { title: "Port not found", text: "No local socket is bound to that port. A connection to the port from elsewhere does not count." };
    default:
      return null;
  }
}

/** How a chain ended, or `null` when it reached the top. These say why the chain stops short, never guess the rest. */
export function endText(e: AncestryEnd): string | null {
  switch (e.kind) {
    case "root":
      return null;
    case "parent_gone":
      return `Its parent (${e.pid}) has exited, so what started it can no longer be read.`;
    case "parent_denied":
      return `Its parent (${e.pid}) exists but this account may not read it.`;
    case "parent_reused":
      return `Its recorded parent (${e.pid}) started after it did: that number was reused, so the real parent has exited.`;
    case "cycle":
      return `The chain loops back to ${e.pid}; it was cut there.`;
    case "depth_limit":
      return "The chain is longer than 32 levels; the rest is not followed.";
    case "unread":
      return `Its parent (${e.pid}) was not part of what the host sent.`;
  }
}

/** What the unit is called to a person. A scope is not a service: a login session or a container, never "the service". */
export function unitKind(s: ServiceAttribution): string {
  if (s.unit_type === "service") return "Service";
  if (s.unit_name.startsWith("session-")) return "Login session";
  if (/^(docker|libpod|cri-containerd|crio)-/.test(s.unit_name)) return "Container";
  return "Scope";
}

export function confidenceText(s: ServiceAttribution): string {
  return s.confidence === "confirmed" ? "Confirmed by systemd" : "From the cgroup only";
}

export function listenText(s: SocketInfo): string {
  const any = s.address === "0.0.0.0" || s.address === "::" || s.address === "*";
  const where = s.ipv6 ? `[${s.address}]` : s.address;
  return `${s.protocol.toUpperCase()}${s.ipv6 ? " (IPv6)" : ""} ${where}:${s.port}${any ? " · all addresses" : s.local_only ? " · this host only" : ""}`;
}

/** UDP has no listening state: a bound socket is all there is, which says it receives, not that anything is serving. */
export function udpNote(s: SocketInfo): string | null {
  return s.protocol === "udp" ? "UDP has no listening state: this socket is bound, and the process owning it receives what is sent to the port." : null;
}

export function startedText(p: ProcInfo, now: number): string | null {
  if (p.started_ms === null) return null;
  const ago = Math.max(0, Math.round((now - p.started_ms) / 1000));
  return `${new Date(p.started_ms).toLocaleString()} (${formatUptime(ago)} ago)`;
}

const STATES: Record<string, string> = { R: "running", S: "sleeping", D: "waiting on disk", Z: "zombie", T: "stopped", t: "traced", I: "idle", X: "dead" };
export function stateText(state: string | null): string | null {
  return state === null ? null : STATES[state] ? `${STATES[state]} (${state})` : state;
}

/** Whether a result is old enough that it should be read again before it is trusted. */
export function isStaleResult(r: Investigation, now: number, limitMs = 60_000): boolean {
  return now - r.taken_at_ms > limitMs;
}

/** A newer request replaces an older one: the answer to the older must not overwrite the newer. */
export function isCurrent(seq: number, latest: number): boolean {
  return seq === latest;
}
