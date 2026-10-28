// Types and plain helpers for the monitoring engine (the Rust side is src-tauri/src/monitoring). Nothing here
// touches the window or the backend, so the rules the forms check and the words the page shows are tested.
import type { Uuid } from "$lib/types";

export type CheckSpec =
  | { kind: "ping"; count: number; timeout_ms: number }
  | { kind: "tcp"; port: number }
  | { kind: "ssh_metrics" }
  /** A web page. `expect_status` 0 means any 2xx or 3xx. */
  | { kind: "http"; url: string; expect_status: number; timeout_ms: number; contains: string | null };

export interface MonitorTarget {
  label: string;
  address: string;
  tags: string[];
  enabled: boolean;
  interval_secs: number;
  checks: CheckSpec[];
  ssh_host_id: Uuid | null;
  notes: string;
}

export type Metric = "ping_loss" | "ping_latency" | "port_closed" | "cpu" | "mem" | "disk" | "http_down" | "http_time" | "cert_days";
export type Op = "gt" | "lt";
export type Scope = { type: "all" } | { type: "tags"; tags: string[] } | { type: "target"; id: Uuid };
export type ChannelKind = "telegram" | "bale" | "email";
export type Status = "ok" | "warning" | "critical" | "unknown";

export interface AlertRule {
  name: string;
  enabled: boolean;
  metric: Metric;
  op: Op;
  warn: number | null;
  crit: number | null;
  attempts: number;
  scope: Scope;
  /** Empty: every enabled channel. */
  channels: ChannelKind[];
}

export interface StateView {
  rule_id: Uuid;
  rule: string;
  subject: string;
  status: Status;
  value: number | null;
  since: number;
}

export interface TargetStatus {
  id: Uuid;
  checked_at: number;
  ping: { loss_pct: number; avg_ms: number | null; error: string | null } | null;
  ports: { port: number; open: boolean; latency_ms: number | null; error: string | null }[];
  /** One per web check. The address is shown without its query. */
  web: { url: string; status: number | null; ok: boolean; time_ms: number | null; cert_days: number | null; error: string | null }[];
  metrics: { cpu: number | null; mem: number | null; disk: number | null } | null;
  states: StateView[];
  errors: string[];
}

export interface Delivery {
  channel: ChannelKind;
  error: string | null;
}

export interface AlertEntry {
  at: number;
  target_id: Uuid;
  target: string;
  rule: string;
  status: Status;
  previous: Status;
  summary: string;
  deliveries: Delivery[];
}

export interface ScanReport {
  host: string;
  scanned: number;
  open: { port: number; service: string | null; latency_ms: number }[];
  took_ms: number;
}

export interface Snapshot {
  running: boolean;
  paused: boolean;
  statuses: TargetStatus[];
  history: AlertEntry[];
  config_error: string | null;
}

export interface BotConfig {
  enabled: boolean;
  bot_token: string;
  chat_id: string;
  base_url: string | null;
  /** Set by the backend: a token is saved (it is never sent back). */
  token_set: boolean;
}

export type SmtpSecurity = "start_tls" | "tls" | "plain";

export interface EmailConfig {
  enabled: boolean;
  server: string;
  port: number;
  security: SmtpSecurity;
  username: string;
  password: string;
  from: string;
  to: string;
  password_set: boolean;
}

export interface NotifyChannels {
  telegram: BotConfig;
  bale: BotConfig;
  email: EmailConfig;
  include_address: boolean;
}

export interface ChannelsRecord {
  rev: number;
  channels: NotifyChannels;
}

export const CHANNEL_NAMES: Record<ChannelKind, string> = { telegram: "Telegram", bale: "Bale", email: "Email" };

export const METRICS: Record<Metric, { label: string; unit: string; hint: string }> = {
  ping_loss: { label: "Ping packet loss", unit: "%", hint: "Share of ping packets that got no answer." },
  ping_latency: { label: "Ping latency", unit: " ms", hint: "Average round trip of the ping packets that were answered." },
  port_closed: { label: "Port closed", unit: "", hint: "A port check could not connect. Set the critical limit to 0." },
  cpu: { label: "CPU use", unit: "%", hint: "Read over SSH. Needs a saved SSH host." },
  mem: { label: "Memory use", unit: "%", hint: "Read over SSH. Needs a saved SSH host." },
  disk: { label: "Disk use", unit: "%", hint: "Of the root file system, read over SSH." },
  http_down: { label: "Site down", unit: "", hint: "A web check got the wrong status, no answer, or a page without its text. Set the critical limit to 0." },
  http_time: { label: "Site response time", unit: " ms", hint: "How long the page took to start answering." },
  cert_days: { label: "Certificate days left", unit: " days", hint: "Days until an https site's certificate ends. Choose “below”." },
};

/** Starting points for the Rules tab. */
export const PRESETS: { name: string; metric: Metric; warn: number | null; crit: number | null; attempts: number; op?: Op }[] = [
  { name: "Ping packet loss", metric: "ping_loss", warn: 0, crit: 50, attempts: 3 },
  { name: "Ping latency", metric: "ping_latency", warn: 200, crit: 500, attempts: 3 },
  { name: "Port closed", metric: "port_closed", warn: null, crit: 0, attempts: 2 },
  { name: "CPU use", metric: "cpu", warn: 80, crit: 90, attempts: 3 },
  { name: "Memory use", metric: "mem", warn: 85, crit: 95, attempts: 3 },
  { name: "Disk use", metric: "disk", warn: 85, crit: 95, attempts: 2 },
  { name: "Site down", metric: "http_down", warn: null, crit: 0, attempts: 2 },
  { name: "Site response time", metric: "http_time", warn: 1500, crit: 5000, attempts: 3 },
  { name: "Certificate expiring", metric: "cert_days", warn: 21, crit: 7, attempts: 1, op: "lt" },
];

export const MIN_INTERVAL_SECS = 10;
/** CPU, memory and disk are read over a second-long script, so they are checked less often. */
export const MIN_SSH_INTERVAL_SECS = 30;

export function emptyTarget(): MonitorTarget {
  return {
    label: "",
    address: "",
    tags: [],
    enabled: true,
    interval_secs: 60,
    checks: [
      { kind: "ping", count: 4, timeout_ms: 1000 },
      { kind: "tcp", port: 22 },
    ],
    ssh_host_id: null,
    notes: "",
  };
}

export function ruleFromPreset(name: string): AlertRule {
  const p = PRESETS.find((x) => x.name === name) ?? PRESETS[0];
  return { name: p.name, enabled: true, metric: p.metric, op: p.op ?? "gt", warn: p.warn, crit: p.crit, attempts: p.attempts, scope: { type: "all" }, channels: [] };
}

/** Same rule as the Rust side's `valid_host`: a name or address that can't be mistaken for an option. */
export function validAddress(a: string): boolean {
  return a.length > 0 && a.length <= 253 && !a.startsWith("-") && /^[A-Za-z0-9._:%-]+$/.test(a);
}

/** Same rule as the backend: http or https, a host, no user name or password in the address. */
export function validUrl(u: string): boolean {
  if (u.length > 2048 || /[\s\u0000-\u001f]/.test(u)) return false;
  const m = /^https?:\/\/([^/?#]*)/i.exec(u);
  if (!m || !m[1] || m[1].includes("@")) return false;
  const authority = m[1].startsWith("[") ? m[1].slice(1).split("]")[0] : m[1].replace(/:\d*$/, "");
  const port = /:(\d+)$/.exec(m[1]);
  if (port && (Number(port[1]) < 1 || Number(port[1]) > 65535)) return false;
  return validAddress(authority);
}

/** An address as the engine names it: no scheme, no query, at most 60 characters. */
export function shortUrl(u: string): string {
  return u.replace(/^https?:\/\//i, "").split(/[?#]/)[0].slice(0, 60);
}

/** The host name in an address, to fill in a host's address from its website. */
export function hostOf(u: string): string {
  const m = /^https?:\/\/([^/?#]*)/i.exec(u.trim());
  if (!m) return "";
  return m[1].startsWith("[") ? m[1].slice(1).split("]")[0] : m[1].replace(/:\d*$/, "");
}

/** `null` when the host is fine to save, otherwise what to fix. */
export function checkTarget(t: MonitorTarget): string | null {
  if (!t.label.trim()) return "Give the host a name.";
  if (!validAddress(t.address.trim())) return "The address must be a host name or an IP address.";
  if (!Number.isFinite(t.interval_secs) || t.interval_secs < MIN_INTERVAL_SECS) return `Check at most every ${MIN_INTERVAL_SECS} seconds.`;
  if (t.checks.length === 0) return "Add at least one check.";
  if (t.checks.length > 20 || t.tags.length > 20 || t.notes.length > 2000) return "That is too many checks or tags, or too long a note.";
  if (t.checks.some((c) => c.kind === "ssh_metrics") && t.interval_secs < MIN_SSH_INTERVAL_SECS) return `CPU, memory and disk are read at most every ${MIN_SSH_INTERVAL_SECS} seconds.`;
  for (const c of t.checks) {
    if (c.kind === "ping" && (!(c.count >= 1 && c.count <= 10) || !(c.timeout_ms >= 200 && c.timeout_ms <= 10000))) return "Ping: 1 to 10 packets, and a wait of 200 to 10000 ms.";
    if (c.kind === "tcp" && !(Number.isInteger(c.port) && c.port >= 1 && c.port <= 65535)) return "Ports are numbers from 1 to 65535.";
    if (c.kind === "ssh_metrics" && !t.ssh_host_id) return "CPU, memory and disk checks need a saved SSH host to log in with.";
    if (c.kind === "http") {
      if (t.checks.filter((x) => x.kind === "http").length > 5) return "Watch at most 5 web addresses per host.";
      if (t.checks.some((x, i) => x.kind === "http" && t.checks.findIndex((y) => y.kind === "http" && shortUrl(y.url) === shortUrl(x.url)) !== i)) return "Two web addresses are the same up to the query. Each one needs to differ.";
      if (!validUrl(c.url)) return "Web checks need an http:// or https:// address, without a user name or password in it.";
      if (c.expect_status !== 0 && !(Number.isInteger(c.expect_status) && c.expect_status >= 100 && c.expect_status <= 599)) return "The expected status is a number from 100 to 599, or empty for any 2xx or 3xx.";
      if (!(c.timeout_ms >= 500 && c.timeout_ms <= 30000)) return "Web checks wait 500 to 30000 ms for an answer.";
      if ((c.contains ?? "").length > 200) return "The text the page must contain can be at most 200 characters.";
    }
  }
  return null;
}

export function checkRule(r: AlertRule): string | null {
  if (!r.name.trim()) return "Give the rule a name.";
  if (r.warn === null && r.crit === null) return "Set a warning limit, a critical limit, or both.";
  if ((r.warn !== null && !Number.isFinite(r.warn)) || (r.crit !== null && !Number.isFinite(r.crit))) return "The limits must be numbers.";
  if (r.warn !== null && r.crit !== null && (r.op === "gt" ? r.warn > r.crit : r.warn < r.crit)) return "The warning limit must be reached before the critical one.";
  if (!(Number.isInteger(r.attempts) && r.attempts >= 1 && r.attempts <= 20)) return "Use 1 to 20 checks in a row.";
  if (r.scope.type === "tags" && r.scope.tags.every((t) => !t.trim())) return "Name at least one tag, or apply the rule to all hosts.";
  return null;
}

/** "a, b ,c" to ["a", "b", "c"]. */
export function splitList(text: string): string[] {
  return text
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** "22, 443" to [22, 443]; `null` if something isn't a port. */
export function parsePorts(text: string): number[] | null {
  const out: number[] = [];
  for (const p of splitList(text)) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (n < 1 || n > 65535) return null;
    if (!out.includes(n)) out.push(n);
  }
  return out;
}

const ORDER: Record<Status, number> = { critical: 3, warning: 2, unknown: 1, ok: 0 };

/** The most serious of several states, or `null` when there are none. */
export function worst(states: { status: Status }[]): Status | null {
  if (states.length === 0) return null;
  return states.reduce<Status>((a, s) => (ORDER[s.status] > ORDER[a] ? s.status : a), "ok");
}

export const STATUS_TEXT: Record<Status, string> = { ok: "OK", warning: "Warning", critical: "Critical", unknown: "Unknown" };

/** Tailwind classes for a status pill. */
export function statusClass(s: Status | null): string {
  switch (s) {
    case "ok":
      return "border-success/30 bg-success/10 text-success";
    case "warning":
      return "border-warning/30 bg-warning/10 text-warning";
    case "critical":
      return "border-danger/30 bg-danger/10 text-danger";
    default:
      return "border-line bg-base text-fg-muted";
  }
}

export function formatValue(metric: Metric, v: number | null): string {
  if (v === null) return "–";
  if (metric === "port_closed") return v >= 1 ? "closed" : "open";
  if (metric === "http_down") return v >= 1 ? "down" : "up";
  const shown = Math.abs(v - Math.round(v)) < 0.05 ? String(Math.round(v)) : v.toFixed(1);
  return `${shown}${METRICS[metric].unit}`;
}

/** "the limit": `> 80% warning, > 90% critical`. */
export function describeLimits(r: Pick<AlertRule, "metric" | "op" | "warn" | "crit">): string {
  const sign = r.op === "gt" ? ">" : "<";
  const part = (v: number | null, word: string) => (v === null ? null : `${sign} ${formatValue(r.metric, v)} ${word}`);
  if (r.metric === "port_closed") return "when a port can't be reached";
  if (r.metric === "http_down") return "when the site isn't answering correctly";
  return [part(r.warn, "warning"), part(r.crit, "critical")].filter(Boolean).join(", ");
}

export function describeScope(s: Scope, targetName: (id: Uuid) => string): string {
  switch (s.type) {
    case "all":
      return "all hosts";
    case "tags":
      return `hosts tagged ${s.tags.join(", ")}`;
    case "target":
      return targetName(s.id);
  }
}

/** "5 s ago", "3 min ago", "2 h ago". */
export function ago(unixSecs: number, nowMs: number = Date.now()): string {
  const s = Math.max(0, Math.round(nowMs / 1000 - unixSecs));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

/** One line for an alert's delivery: who got it and who did not. */
export function describeDeliveries(d: Delivery[]): string {
  if (d.length === 0) return "Not sent: no channel is switched on.";
  const sent = d.filter((x) => !x.error).map((x) => CHANNEL_NAMES[x.channel]);
  // The backend already starts its errors with the channel's name ("Email: the server said…").
  const why = (x: Delivery) => (x.error ?? "").replace(new RegExp(`^${CHANNEL_NAMES[x.channel]}:\\s*`), "");
  const failed = d.filter((x) => x.error).map((x) => `${CHANNEL_NAMES[x.channel]} (${why(x)})`);
  return [sent.length ? `Sent to ${sent.join(", ")}` : "", failed.length ? `Failed: ${failed.join("; ")}` : ""].filter(Boolean).join(". ");
}

export function emptyChannels(): NotifyChannels {
  const bot = (): BotConfig => ({ enabled: false, bot_token: "", chat_id: "", base_url: null, token_set: false });
  return {
    telegram: bot(),
    bale: bot(),
    email: { enabled: false, server: "", port: 587, security: "start_tls", username: "", password: "", from: "", to: "", password_set: false },
    include_address: false,
  };
}

export const SMTP_PORTS: Record<SmtpSecurity, number> = { start_tls: 587, tls: 465, plain: 25 };
