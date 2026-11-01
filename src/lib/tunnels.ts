// The Tunnels workspace's rules, kept apart from the screen so they can be tested: what state a tunnel is in, how
// rows are searched and filtered, how a tunnel reads in words, and what makes a tunnel valid (the same checks the
// backend makes in forward.rs, so the form can say what is wrong before anything is sent; the backend still decides).

import { describeForward, type ForwardKind, type ForwardLog, type ForwardRule, type ForwardStatus, type Host, type Uuid } from "./types";

/** `reconnecting` is not here: a tunnel does not reconnect by itself, it fails and is started again. */
export type TunnelStatus = "stopped" | "connecting" | "active" | "stopping" | "failed";

/** An operation in flight, from the moment it is asked for until the backend reports the result. */
export type Pending = "starting" | "stopping" | "restarting";

export type TypeFilter = "all" | ForwardKind["kind"];
export type StatusFilter = "all" | "active" | "stopped" | "connecting" | "failed";

export function tunnelStatus(st: ForwardStatus | undefined, pending?: Pending): TunnelStatus {
  if (pending === "stopping") return "stopping";
  // A restart is stop-then-start in the backend: until it reports active (or fails) it is on its way up, not active.
  if (pending === "restarting" && st?.state !== "error") return "connecting";
  switch (st?.state) {
    case "active":
      return "active";
    case "starting":
      return "connecting";
    case "error":
      return "failed";
    default:
      return pending === "starting" ? "connecting" : "stopped";
  }
}

export const STATUS_TEXT: Record<TunnelStatus, string> = { stopped: "Stopped", connecting: "Connecting", active: "Active", stopping: "Stopping", failed: "Failed" };

export const KIND_TEXT: Record<ForwardKind["kind"], string> = { local: "Local", remote: "Remote", dynamic: "Dynamic" };

/** A tunnel as the screen shows it. */
export interface TunnelRow {
  id: Uuid;
  rule: ForwardRule;
  host: Host | undefined;
  hostLabel: string;
  status: TunnelStatus;
  /** The port actually bound (differs from the rule's when it asked for 0). */
  boundPort: number | null;
  error: string | null;
  /** When it became active, if it is. */
  activeSince: number | null;
}

export function buildRow(id: Uuid, rule: ForwardRule, host: Host | undefined, st: ForwardStatus | undefined, pending?: Pending, activeSince?: number | null): TunnelRow {
  const status = tunnelStatus(st, pending);
  return {
    id,
    rule,
    host,
    hostLabel: host?.label ?? "missing host",
    status,
    boundPort: st?.state === "active" ? st.port : null,
    error: st?.state === "error" ? st.message : null,
    activeSince: status === "active" || status === "stopping" ? (activeSince ?? null) : null,
  };
}

/** Whether Start is possible: only while stopped or failed, and the host still exists. */
export const canStart = (r: TunnelRow) => (r.status === "stopped" || r.status === "failed") && !!r.host;
/** Stop applies to anything that is up or on its way up. */
export const canStop = (r: TunnelRow) => r.status === "active" || r.status === "connecting";
export const canRestart = (r: TunnelRow) => (r.status === "active" || r.status === "failed") && !!r.host;
/** Editing is allowed any time; a running tunnel is told a restart is needed. */
export const isBusy = (r: TunnelRow) => r.status === "connecting" || r.status === "stopping";

/** The two ends of a tunnel in the table's words: `127.0.0.1:8000 → 127.0.0.1:8000`. */
export function mapping(rule: ForwardRule, boundPort: number | null = null): { from: string; to: string } {
  const port = rule.bind_port === 0 && boundPort ? boundPort : rule.bind_port;
  const from = `${rule.bind_addr || "*"}:${port === 0 ? "auto" : port}`;
  if (rule.kind === "dynamic") return { from, to: "SOCKS5" };
  return { from, to: `${rule.dest_host}:${rule.dest_port}` };
}

export const mappingText = (rule: ForwardRule, boundPort: number | null = null) => {
  const m = mapping(rule, boundPort);
  return `${m.from} → ${m.to}`;
};

/** The labelled ends for the overview's picture, by tunnel type. */
export function endpoints(rule: ForwardRule, hostLabel: string, boundPort: number | null = null): { start: { label: string; value: string }; middle: string; end: { label: string; value: string } } {
  const m = mapping(rule, boundPort);
  switch (rule.kind) {
    case "local":
      return { start: { label: "Local endpoint", value: m.from }, middle: `SSH tunnel via ${hostLabel}`, end: { label: "Remote destination", value: m.to } };
    case "remote":
      return { start: { label: "Remote endpoint", value: `${m.from} on ${hostLabel}` }, middle: "SSH tunnel back to this computer", end: { label: "Local destination", value: m.to } };
    case "dynamic":
      return { start: { label: "SOCKS5 listener", value: m.from }, middle: `SSH tunnel via ${hostLabel}`, end: { label: "Destination", value: "chosen by each connecting program" } };
  }
}

/** Every word typed must appear in the tunnel's name, description, host, type, ports, destination or status. */
export function matches(row: TunnelRow, query: string, type: TypeFilter, status: StatusFilter): boolean {
  if (type !== "all" && row.rule.kind !== type) return false;
  if (status !== "all") {
    const shown = row.status === "stopping" ? "active" : row.status;
    if (shown !== status) return false;
  }
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const r = row.rule;
  const hay = [
    r.label,
    r.description ?? "",
    describeForward(r),
    r.kind === "local" ? "local -L" : r.kind === "remote" ? "remote -R" : "dynamic socks socks5 -D",
    r.bind_addr,
    String(r.bind_port),
    row.boundPort ? String(row.boundPort) : "",
    "dest_host" in r ? `${r.dest_host} ${r.dest_port} ${r.dest_host}:${r.dest_port}` : "",
    row.hostLabel,
    row.host?.hostname ?? "",
    STATUS_TEXT[row.status],
  ]
    .join(" ")
    .toLowerCase();
  return words.every((w) => hay.includes(w));
}

export interface Counts {
  total: number;
  active: number;
  stopped: number;
  failed: number;
  connecting: number;
}

/** From every configured tunnel, not only the ones a filter shows. */
export function counts(rows: TunnelRow[]): Counts {
  const c: Counts = { total: rows.length, active: 0, stopped: 0, failed: 0, connecting: 0 };
  for (const r of rows) {
    if (r.status === "active" || r.status === "stopping") c.active++;
    else if (r.status === "stopped") c.stopped++;
    else if (r.status === "failed") c.failed++;
    else c.connecting++;
  }
  return c;
}

/** `3m 12s`, `2h 5m`, `4d 3h`: the two largest units. */
export function formatUptime(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

// -- validation ------------------------------------------------------------------------------------------------

export type FieldErrors = Partial<Record<"label" | "host_id" | "bind_addr" | "bind_port" | "dest_host" | "dest_port", string>>;

function hostOk(host: string): boolean {
  if (!host || host.length > 253 || /\s/.test(host)) return false;
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host)) return host.split(".").every((n) => Number(n) <= 255);
  if (host.includes(":")) return /^[0-9a-fA-F:]+$/.test(host) && host.length <= 39 && host.split(":").length >= 3 && host.split(":").length <= 8;
  return host.split(".").every((l) => l.length > 0 && l.length <= 63 && !l.startsWith("-") && !l.endsWith("-") && /^[A-Za-z0-9_-]+$/.test(l));
}

const isWild = (a: string) => ["0.0.0.0", "::", "[::]", "*", ""].includes(a);
const overlaps = (a: string, b: string) => a === b || isWild(a) || isWild(b) || (["127.0.0.1", "::1", "localhost"].includes(a) && ["127.0.0.1", "::1", "localhost"].includes(b) && (a === "localhost" || b === "localhost"));

/**
 * What is wrong with a tunnel before it is saved, by field. Port 0 is allowed (it picks a free port). `others` are
 * the other saved tunnels, for a port two of them would both want. The backend makes the same checks.
 */
export function validateTunnel(rule: ForwardRule, hostIds: Set<Uuid>, id: Uuid | null, others: { id: Uuid; rule: ForwardRule }[]): FieldErrors {
  const e: FieldErrors = {};
  if (!rule.label.trim()) e.label = "Give the tunnel a name";
  if (!rule.host_id) e.host_id = "Choose the SSH host to tunnel through";
  else if (!hostIds.has(rule.host_id)) e.host_id = "That host no longer exists";
  const remote = rule.kind === "remote";
  const addr = rule.bind_addr.trim();
  if (!((remote && (addr === "" || addr === "*")) || hostOk(addr))) e.bind_addr = "Use an IP address such as 127.0.0.1, or a host name";
  if (!Number.isInteger(rule.bind_port) || rule.bind_port < 0 || rule.bind_port > 65535) e.bind_port = "Use a port from 1 to 65535 (0 picks a free one)";
  if (rule.kind !== "dynamic") {
    if (!hostOk(rule.dest_host.trim())) e.dest_host = "Enter a host name or IP address";
    if (!Number.isInteger(rule.dest_port) || rule.dest_port < 1 || rule.dest_port > 65535) e.dest_port = "Use a port from 1 to 65535";
  }
  if (!e.bind_port && !e.bind_addr && rule.bind_port !== 0) {
    for (const o of others) {
      if (o.id === id) continue;
      const a = rule;
      const b = o.rule;
      const clash =
        (a.kind !== "remote" && b.kind !== "remote" && b.bind_port === a.bind_port && overlaps(a.bind_addr.trim(), b.bind_addr.trim())) ||
        (a.kind === "remote" && b.kind === "remote" && b.host_id === a.host_id && b.bind_port === a.bind_port && overlaps(a.bind_addr.trim(), b.bind_addr.trim()));
      if (clash) {
        e.bind_port = `Port ${rule.bind_port} is already used by the tunnel "${b.label}"`;
        break;
      }
    }
  }
  return e;
}

/** Changing any of these on a running tunnel needs a restart to take effect; name, description and auto-start don't. */
export function needsRestart(before: ForwardRule, after: ForwardRule): boolean {
  const route = (r: ForwardRule) => JSON.stringify([r.host_id, r.kind, r.bind_addr.trim(), r.bind_port, "dest_host" in r ? [r.dest_host.trim(), r.dest_port] : null]);
  return route(before) !== route(after);
}

/** The review step and the overview in plain words. */
export function summary(rule: ForwardRule, hostLabel: string): { title: string; lines: [string, string][] } {
  const m = mapping(rule);
  switch (rule.kind) {
    case "local":
      return { title: "Local forwarding", lines: [["SSH host", hostLabel], ["Local endpoint", m.from], ["Remote destination", m.to]] };
    case "remote":
      return { title: "Remote forwarding", lines: [["SSH host", hostLabel], ["Remote endpoint (on the host)", m.from], ["Local destination", m.to]] };
    case "dynamic":
      return { title: "Dynamic forwarding", lines: [["SSH host", hostLabel], ["Local SOCKS5 listener", m.from]] };
  }
}

// -- logs --------------------------------------------------------------------------------------------------------

export type LevelFilter = "all" | ForwardLog["level"];

export function filterLogs(logs: ForwardLog[], query: string, level: LevelFilter): ForwardLog[] {
  const q = query.trim().toLowerCase();
  return logs.filter((l) => (level === "all" || l.level === level) && (!q || l.message.toLowerCase().includes(q)));
}

const pad = (n: number, w = 2) => String(n).padStart(w, "0");
export function logTime(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

/** The text a person pastes into a ticket: what the tunnel is, how it stands, and its log. No secrets are in it. */
export function diagnostics(row: TunnelRow, logs: ForwardLog[]): string {
  const head = [
    `Tunnel: ${row.rule.label}`,
    `Type: ${KIND_TEXT[row.rule.kind]}`,
    `Mapping: ${mappingText(row.rule, row.boundPort)}`,
    `SSH host: ${row.hostLabel}`,
    `Status: ${STATUS_TEXT[row.status]}${row.error ? ` (${row.error})` : ""}`,
  ];
  const lines = logs.map((l) => `${new Date(l.at_ms).toISOString()} ${l.level.toUpperCase().padEnd(5)} ${l.message}`);
  return [...head, "", ...(lines.length ? lines : ["(no log lines)"])].join("\n");
}

/** What a bulk action did: how many went through, and which did not, with the reason. */
export function bulkMessage(verb: string, total: number, failures: { label: string; error: string }[]): { kind: "info" | "error"; text: string } {
  if (!failures.length) return { kind: "info", text: `${verb} ${total} tunnel${total === 1 ? "" : "s"}.` };
  const done = total - failures.length;
  const list = failures.slice(0, 3).map((f) => `${f.label} (${f.error})`).join("; ");
  const more = failures.length > 3 ? ` and ${failures.length - 3} more` : "";
  return { kind: "error", text: `${verb} ${done} of ${total}. Failed: ${list}${more}.` };
}

// -- the form's draft -------------------------------------------------------------------------------------------

/** All of a tunnel's fields at once, so switching the type in a form doesn't lose what was typed for another type. */
export interface Draft {
  label: string;
  description: string;
  host_id: Uuid;
  auto_start: boolean;
  kind: ForwardKind["kind"];
  bind_addr: string;
  bind_port: number;
  dest_host: string;
  dest_port: number;
}

export function draftFromRule(rule: ForwardRule): Draft {
  return {
    label: rule.label,
    description: rule.description ?? "",
    host_id: rule.host_id,
    auto_start: rule.auto_start,
    kind: rule.kind,
    bind_addr: rule.bind_addr,
    bind_port: rule.bind_port,
    dest_host: "dest_host" in rule ? rule.dest_host : "127.0.0.1",
    dest_port: "dest_port" in rule ? rule.dest_port : 80,
  };
}

/** A new tunnel's starting values for a type: a local or SOCKS listener stays on this computer, a remote one on the server. */
export function newDraft(hostId: Uuid, kind: ForwardKind["kind"] = "local"): Draft {
  return { label: "", description: "", host_id: hostId, auto_start: false, kind, bind_addr: kind === "remote" ? "127.0.0.1" : "127.0.0.1", bind_port: kind === "dynamic" ? 1080 : 8080, dest_host: "127.0.0.1", dest_port: 80 };
}

export function ruleFromDraft(d: Draft): ForwardRule {
  const common = { label: d.label.trim(), host_id: d.host_id, auto_start: d.auto_start, ...(d.description.trim() ? { description: d.description.trim() } : {}), bind_addr: d.bind_addr.trim(), bind_port: Number(d.bind_port) };
  return d.kind === "dynamic" ? { ...common, kind: "dynamic" } : { ...common, kind: d.kind, dest_host: d.dest_host.trim(), dest_port: Number(d.dest_port) };
}
