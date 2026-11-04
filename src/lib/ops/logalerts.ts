// Noticing trouble in the log lines as they arrive, to hand to the existing alert engine (stores/alerts.svelte.ts).
// Three kinds: a critical journal entry, repeated failed SSH logins, and a service that failed. Each is announced once
// per host and key for a cooldown, and a host can raise only so many in a window, so a flood of lines is one alert
// and not a storm. Pure: it is given rows and the time.
import type { Row } from "./logrows";

export interface LogAlertRules {
  /** Priority 0-2 (emergency, alert, critical) in the journal. */
  critical: boolean;
  /** Failed SSH logins: this many from the same host within the window. */
  auth: boolean;
  authCount: number;
  authWindowSecs: number;
  /** A systemd unit entering the failed state. */
  service: boolean;
  /** Seconds before the same thing on the same host is announced again. */
  cooldownSecs: number;
}

export const DEFAULT_LOG_RULES: LogAlertRules = { critical: false, auth: false, authCount: 5, authWindowSecs: 300, service: false, cooldownSecs: 600 };

export type LogAlertKind = "log" | "auth" | "service";

export interface LogSignal {
  hostId: string;
  host: string;
  kind: LogAlertKind;
  /** What makes it the same alert again (a unit, a source address). */
  key: string;
  message: string;
  at: number;
}

const AUTH_FAIL = /(failed password|invalid user|authentication failure|failed publickey|connection closed by authenticating user|maximum authentication attempts exceeded)/i;
const SERVICE_FAIL = /(failed with result|entered failed state|main process exited, code=\w+, status=\d+\/(?!0\b)\S+|start request repeated too quickly)/i;
/** The address a login attempt came from: `from 203.0.113.9`, `rhost=203.0.113.9`, or an IPv6. */
const FROM = /(?:\bfrom\s+|rhost=)([0-9a-fA-F:.]{3,45})\b/;

const isSshd = (r: Row) => /^sshd?$/.test(r.ident) || /^ssh(d)?(@.*)?\.service$/.test(r.unit) || r.unit === "sshd.service";

export const MAX_PER_HOST_WINDOW = 5;
const STORM_WINDOW_MS = 10 * 60_000;

export class LogAlertDetector {
  /** Failed-login times per host. */
  #auth = new Map<string, number[]>();
  /** When each host and key was last announced. */
  #last = new Map<string, number>();
  /** Announcements per host, to cap a storm. */
  #burst = new Map<string, number[]>();

  constructor(private readonly rules: () => LogAlertRules) {}

  reset() {
    this.#auth.clear();
    this.#last.clear();
    this.#burst.clear();
  }

  /** What to announce for these rows (newest last), at `now`. */
  feed(rows: Row[], now = Date.now()): LogSignal[] {
    const r = this.rules();
    if (!r.critical && !r.auth && !r.service) return [];
    const out: LogSignal[] = [];
    for (const row of rows) {
      if (r.critical && row.priority !== null && row.priority <= 2) {
        this.#raise(out, row, "log", `${row.unit || row.ident || "journal"}:${row.message.slice(0, 80)}`, `${row.host}: critical log entry from ${row.unit || row.ident || "the journal"}: ${clip(row.message)}`, now);
      }
      if (r.service && SERVICE_FAIL.test(row.message)) {
        const unit = row.unit || /([\w@.\\-]+\.service)/.exec(row.message)?.[1] || "a service";
        this.#raise(out, row, "service", unit, `${row.host}: ${unit} failed (${clip(row.message, 100)})`, now);
      }
      if (r.auth && isSshd(row) && AUTH_FAIL.test(row.message)) {
        const list = this.#auth.get(row.hostId) ?? [];
        const keep = list.filter((t) => now - t < r.authWindowSecs * 1000);
        keep.push(now);
        this.#auth.set(row.hostId, keep);
        if (keep.length >= Math.max(2, r.authCount)) {
          const from = FROM.exec(row.message)?.[1];
          this.#raise(out, row, "auth", "ssh-auth", `${row.host}: ${keep.length} failed SSH logins in ${Math.round(r.authWindowSecs / 60) || 1} min${from ? `, latest from ${from}` : ""}.`, now);
        }
      }
    }
    return out;
  }

  #raise(out: LogSignal[], row: Row, kind: LogAlertKind, key: string, message: string, now: number) {
    const id = `${row.hostId}|${kind}|${key}`;
    const cool = this.rules().cooldownSecs * 1000;
    const last = this.#last.get(id);
    if (last !== undefined && now - last < cool) return;
    const burst = (this.#burst.get(row.hostId) ?? []).filter((t) => now - t < STORM_WINDOW_MS);
    if (burst.length >= MAX_PER_HOST_WINDOW) return;
    burst.push(now);
    this.#burst.set(row.hostId, burst);
    this.#last.set(id, now);
    out.push({ hostId: row.hostId, host: row.host, kind, key, message, at: now });
  }
}

const clip = (s: string, n = 140) => (s.length > n ? `${s.slice(0, n)}…` : s);
