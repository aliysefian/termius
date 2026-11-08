// What the process, port and login views do with the lists they are given: search, sort, build a tree, say what a signal
// does, and turn a listener into a tunnel. Pure, so each decision is tested.
import type { AuthEvent, ListeningPort, LoginEntry, ProcessInfo, SignalName } from "./hostsample";
import type { ForwardRule, Uuid } from "./types";

// -- processes -------------------------------------------------------------------------------------------------------

export type ProcessSort = "cpu" | "mem" | "pid" | "name" | "user" | "age";
export type Dir = "asc" | "desc";

/** The direction a column starts in: numbers biggest first, words A to Z. */
export const defaultDir = (k: ProcessSort): Dir => (k === "name" || k === "user" || k === "pid" ? "asc" : "desc");

/** A process matches a search when every word is in its name, command, user or pid. */
export function matchesProcess(p: ProcessInfo, query: string): boolean {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = `${p.pid} ${p.name} ${p.user} ${p.command}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

const keyOf = (p: ProcessInfo, k: ProcessSort): number | string | null => {
  switch (k) {
    case "cpu":
      return p.cpu_pct;
    case "mem":
      return p.rss;
    case "pid":
      return p.pid;
    case "name":
      return p.name.toLowerCase();
    case "user":
      return p.user.toLowerCase();
    case "age":
      return p.age_secs;
  }
};

/** Sorted by a column. A process with no figure for it goes last whichever way it is sorted, never in among the zeros. */
export function sortProcesses(list: ProcessInfo[], key: ProcessSort, dir: Dir): ProcessInfo[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...list].sort((a, b) => {
    const x = keyOf(a, key);
    const y = keyOf(b, key);
    if (x === null && y === null) return a.pid - b.pid;
    if (x === null) return 1;
    if (y === null) return -1;
    const c = typeof x === "string" ? x.localeCompare(y as string) : x - (y as number);
    return c * sign || a.pid - b.pid;
  });
}

export interface ProcessRow {
  p: ProcessInfo;
  depth: number;
  /** Has children in the list (shown or not). */
  parent: boolean;
}

/**
 * The rows to draw. Flat: matching processes in sort order. As a tree: each process under its parent, siblings in sort
 * order, a match shown with the chain of parents above it so its place is clear. A process whose parent is not in the list
 * (the list is bounded) starts its own branch.
 */
export function processRows(list: ProcessInfo[], o: { query: string; sort: ProcessSort; dir: Dir; tree: boolean }): ProcessRow[] {
  if (!o.tree) return sortProcesses(list.filter((p) => matchesProcess(p, o.query)), o.sort, o.dir).map((p) => ({ p, depth: 0, parent: false }));
  const byPid = new Map(list.map((p) => [p.pid, p]));
  const kids = new Map<number, ProcessInfo[]>();
  const roots: ProcessInfo[] = [];
  for (const p of list) {
    const up = p.ppid !== p.pid && byPid.has(p.ppid) ? p.ppid : null;
    if (up === null) roots.push(p);
    else kids.set(up, [...(kids.get(up) ?? []), p]);
  }
  const keep = new Set<number>();
  const searching = o.query.trim() !== "";
  if (searching) {
    for (const p of list) {
      if (!matchesProcess(p, o.query)) continue;
      // The match, and every ancestor, so the tree reads from the top.
      for (let q: ProcessInfo | undefined = p, guard = 0; q && !keep.has(q.pid) && guard < 64; q = byPid.get(q.ppid), guard++) keep.add(q.pid);
    }
  }
  const rows: ProcessRow[] = [];
  const walk = (p: ProcessInfo, depth: number, seen: Set<number>) => {
    if (seen.has(p.pid) || (searching && !keep.has(p.pid))) return;
    seen.add(p.pid);
    const children = sortProcesses(kids.get(p.pid) ?? [], o.sort, o.dir);
    rows.push({ p, depth, parent: children.length > 0 });
    for (const c of children) walk(c, depth + 1, seen);
  };
  const seen = new Set<number>();
  for (const r of sortProcesses(roots, o.sort, o.dir)) walk(r, 0, seen);
  return rows;
}

const SIGNAL_WORDS: Record<SignalName, { title: string; does: string; danger: boolean }> = {
  TERM: { title: "Terminate", does: "SIGTERM asks the program to stop. It can save its work and exit; some ignore it.", danger: true },
  KILL: { title: "Force kill", does: "SIGKILL stops it at once. It gets no chance to save or clean up: unsaved data is lost and its children can be left running. Try Terminate first.", danger: true },
  HUP: { title: "Hang up", does: "SIGHUP is usually read as 'reload your configuration', and sometimes as 'quit'.", danger: true },
  INT: { title: "Interrupt", does: "SIGINT is what Ctrl+C sends.", danger: true },
  STOP: { title: "Pause", does: "SIGSTOP freezes the program until it is continued. Connections it holds may time out.", danger: true },
  CONT: { title: "Continue", does: "SIGCONT resumes a paused program.", danger: false },
  USR1: { title: "Send SIGUSR1", does: "A user-defined signal: what it does is up to the program.", danger: true },
  USR2: { title: "Send SIGUSR2", does: "A user-defined signal: what it does is up to the program.", danger: true },
};

export const signalInfo = (s: SignalName) => SIGNAL_WORDS[s];

/** What to say before a signal is sent: the target host, the process, and what the signal does. Names no secrets. */
export function signalQuestion(p: ProcessInfo, host: string, signal: SignalName): string {
  const sshd = /^sshd/.test(p.name) ? "\n\nThis looks like an SSH server process. Ending it can close a login session, possibly the one used for monitoring." : "";
  return `Send ${signal} to ${p.name} (process ${p.pid}, user ${p.user || "unknown"}) on ${host}?\n\n${SIGNAL_WORDS[signal].does}${sshd}\n\nThe host checks that this number is still the same program before sending anything.`;
}

/** The outcome of a signal as a sentence, and how it should be shown. */
export function signalResult(o: import("./hostsample").SignalOutcome, p: ProcessInfo, signal: SignalName): { kind: "info" | "error"; text: string } {
  const what = `${p.name} (${p.pid})`;
  switch (o.kind) {
    case "sent":
      return { kind: "info", text: `Sent SIG${signal} to ${what}.` };
    case "gone":
      return { kind: "info", text: `${what} had already ended.` };
    case "changed":
      return { kind: "error", text: `Process ${p.pid} is no longer ${p.name}: the number was reused. Nothing was sent.` };
    case "refused":
      return { kind: "error", text: `The host did not allow it: ${o.reason}` };
    case "unknown":
      return { kind: "error", text: `The host did not answer, so ${what} may or may not have received SIG${signal}. It was not sent again: check the list before trying again.` };
  }
}

// -- listening ports ---------------------------------------------------------------------------------------------------

export type PortSort = "port" | "proto" | "address" | "process";
export type PortScope = "all" | "public" | "local";

export interface PortFilter {
  query: string;
  proto: "all" | "tcp" | "udp";
  scope: PortScope;
  /** A process name; empty is any. */
  process: string;
}

export function filterPorts(list: ListeningPort[], f: PortFilter): ListeningPort[] {
  const words = f.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return list.filter((p) => {
    if (f.proto !== "all" && p.proto !== f.proto) return false;
    if (f.scope === "local" && !p.local_only) return false;
    if (f.scope === "public" && p.local_only) return false;
    if (f.process && p.process !== f.process) return false;
    const hay = `${p.proto} ${p.address} ${p.port} ${p.process ?? ""} ${p.pid ?? ""} ${p.user ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export function sortPorts(list: ListeningPort[], key: PortSort, dir: Dir): ListeningPort[] {
  const sign = dir === "asc" ? 1 : -1;
  const k = (p: ListeningPort): number | string => (key === "port" ? p.port : key === "proto" ? p.proto : key === "address" ? p.address : (p.process ?? "￿").toLowerCase());
  return [...list].sort((a, b) => {
    const x = k(a);
    const y = k(b);
    const c = typeof x === "string" ? x.localeCompare(y as string) : x - (y as number);
    return c * sign || a.port - b.port || a.proto.localeCompare(b.proto);
  });
}

/** `host:port`, with the all-addresses forms written as the port alone and IPv6 in brackets. */
export function addressText(p: ListeningPort): string {
  const all = p.address === "0.0.0.0" || p.address === "::" || p.address === "*";
  return all ? `:${p.port}` : p.ipv6 ? `[${p.address}]:${p.port}` : `${p.address}:${p.port}`;
}

/** What the bind address means for who can reach it, in words. It never says a service is reachable from outside. */
export function scopeText(p: ListeningPort): string {
  if (p.local_only) return "This host only (loopback)";
  if (p.address === "0.0.0.0" || p.address === "::" || p.address === "*") return "All addresses: reachable wherever the firewall and network allow";
  return `Bound to ${p.address}`;
}

/**
 * A saved-tunnel template that reaches this listener from here: a local forward to the host's own side of the port. The
 * destination is the host's loopback unless the listener is bound to one specific address, which is then used. TCP only: an
 * SSH tunnel carries nothing else.
 */
export function tunnelFor(p: ListeningPort, hostId: Uuid, hostLabel: string): Extract<ForwardRule, { kind: "local" }> | null {
  if (p.proto !== "tcp") return null;
  const all = p.address === "0.0.0.0" || p.address === "::" || p.address === "*";
  return {
    kind: "local",
    label: `${hostLabel} :${p.port}${p.process ? ` (${p.process})` : ""}`,
    host_id: hostId,
    auto_start: false,
    description: "Made from the host's listening ports.",
    bind_addr: "127.0.0.1",
    bind_port: p.port,
    dest_host: all || p.local_only ? "127.0.0.1" : p.address,
    dest_port: p.port,
  };
}

// -- logins --------------------------------------------------------------------------------------------------------------

export interface FailureCount {
  from: string;
  count: number;
  users: string[];
}

/**
 * How many failed attempts each source address made, most first. A count is a fact; it is not a verdict: nothing here says
 * an address is an attacker, and a typo from a colleague counts the same as a stranger.
 */
export function failuresBySource(events: AuthEvent[]): FailureCount[] {
  const by = new Map<string, { count: number; users: Set<string> }>();
  for (const e of events) {
    if (e.kind !== "failed" && e.kind !== "invalid_user") continue;
    const k = e.from ?? "unknown address";
    const cur = by.get(k) ?? { count: 0, users: new Set<string>() };
    cur.count++;
    if (e.user) cur.users.add(e.user);
    by.set(k, cur);
  }
  return [...by.entries()].map(([from, v]) => ({ from, count: v.count, users: [...v.users].slice(0, 5) })).sort((a, b) => b.count - a.count || a.from.localeCompare(b.from));
}

/** "2m ago", "1h 22m ago", "3d ago" from a time on the host's clock and the host's own "now". Null without a time. */
export function agoText(startedMs: number | null, nowMs: number): string | null {
  if (startedMs === null) return null;
  const s = Math.max(0, Math.round((nowMs - startedMs) / 1000));
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${m % 60}m ago`;
  return `${Math.floor(h / 24)}d ago`;
}

/** Logins newest first, those without a parseable time last. */
export function newestFirst(list: LoginEntry[]): LoginEntry[] {
  return [...list].sort((a, b) => (b.started_ms ?? -1) - (a.started_ms ?? -1));
}
