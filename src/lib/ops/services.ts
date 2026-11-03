// The service manager: list systemd units and start, stop, restart, reload, enable or disable one.
// What to run and how to read it; the connection and the confirmation are the view's.
import { shq } from "./quote";
import { UNIT } from "./logs";

export type Verb = "start" | "stop" | "restart" | "reload" | "enable" | "disable";
export const VERBS: Verb[] = ["start", "stop", "restart", "reload", "enable", "disable"];

/** Which verbs change a running service for people using it. These ask first on a production host. */
export const DISRUPTIVE: Verb[] = ["stop", "restart", "disable"];

export interface Unit {
  name: string;
  load: string;
  active: string;
  sub: string;
  description: string;
  /** From the unit files: enabled, disabled, static, masked ... Empty if the host didn't say. */
  enabled: string;
  /** When it last changed state, as the host wrote it ("2026-11-03 10:20:30 UTC"); empty if the host didn't say. */
  since: string;
}

export const MARK_NO_SYSTEMD = "@@NO-SYSTEMD";
const MARK_FILES = "@@FILES";
const MARK_STATE = "@@STATE";

/** Prints the units, then a marker, then which are enabled. A host without systemd says so. */
export const LIST_SCRIPT = [
  "command -v systemctl >/dev/null 2>&1 || { echo '" + MARK_NO_SYSTEMD + "'; exit 0; }",
  "systemctl list-units --type=service --all --no-legend --plain --no-pager 2>/dev/null",
  "echo '" + MARK_FILES + "'",
  "systemctl list-unit-files --type=service --no-legend --no-pager 2>/dev/null",
  "echo '" + MARK_STATE + "'",
  // Best effort: an older systemd that can't show many units at once just leaves the column empty.
  "systemctl show '*.service' --property=Id,StateChangeTimestamp --no-pager 2>/dev/null",
].join("; ");

/** The time part of a `systemctl show` timestamp ("Tue 2026-11-03 10:20:30 UTC" gives "2026-11-03 10:20:30 UTC"). */
export function sinceText(raw: string): string {
  const m = /(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?: \S+)?)/.exec(raw);
  return m ? m[1] : "";
}

export function parseUnits(output: string): { systemd: boolean; units: Unit[] } {
  if (output.includes(MARK_NO_SYSTEMD)) return { systemd: false, units: [] };
  const [listed, rest = ""] = output.split(MARK_FILES);
  const [files = "", state = ""] = rest.split(MARK_STATE);
  const enabled = new Map<string, string>();
  for (const line of files.split("\n")) {
    const f = line.trim().split(/\s+/);
    if (f.length >= 2 && f[0].endsWith(".service")) enabled.set(f[0], f[1]);
  }
  const since = new Map<string, string>();
  for (const block of state.split(/\n\s*\n/)) {
    const id = /^Id=(\S+)/m.exec(block)?.[1];
    const at = sinceText(/^StateChangeTimestamp=(.*)$/m.exec(block)?.[1] ?? "");
    if (id && at) since.set(id, at);
  }
  const units: Unit[] = [];
  for (const raw of listed.split("\n")) {
    // A failed unit is marked with a bullet in some versions, even with --plain.
    const line = raw.replace(/^[●○×*]\s*/, "").trim();
    const m = /^(\S+\.service)\s+(\S+)\s+(\S+)\s+(\S+)\s*(.*)$/.exec(line);
    if (!m) continue;
    units.push({ name: m[1], load: m[2], active: m[3], sub: m[4], description: m[5], enabled: enabled.get(m[1]) ?? "", since: since.get(m[1]) ?? "" });
  }
  units.sort((a, b) => a.name.localeCompare(b.name));
  return { systemd: true, units };
}

const SHOW_PROPS = ["Id", "Description", "LoadState", "ActiveState", "SubState", "UnitFileState", "MainPID", "Result", "NRestarts", "Restart", "User", "MemoryCurrent", "StateChangeTimestamp", "ActiveEnterTimestamp", "FragmentPath"];
const MARK_STATUS = "@@STATUS";

/** The unit's properties, then its status text. The unit is one quoted word. */
export function detailsScript(unit: string): string {
  const err = unitError(unit);
  if (err) throw new Error(err);
  return `systemctl show --no-pager -p ${SHOW_PROPS.join(",")} -- ${shq(unit)} 2>&1; echo '${MARK_STATUS}'; systemctl status --no-pager -l -n 15 -- ${shq(unit)} 2>&1`;
}

export interface UnitDetails {
  props: Record<string, string>;
  status: string;
}

export function parseDetails(output: string): UnitDetails {
  const [shown, status = ""] = output.split(MARK_STATUS);
  const props: Record<string, string> = {};
  for (const line of shown.split("\n")) {
    const i = line.indexOf("=");
    if (i > 0) props[line.slice(0, i)] = line.slice(i + 1).trim();
  }
  return { props, status: status.trim() };
}

/** Bytes as a short size, or "" for what systemd writes when it doesn't know (`[not set]`, a huge sentinel). */
export function sizeText(raw: string | undefined): string {
  const n = Number(raw);
  if (!raw || !Number.isFinite(n) || n < 0 || n >= 2 ** 62) return "";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}

export function unitError(name: string): string | null {
  return UNIT.test(name) ? null : "That isn't a unit name.";
}

/** `sudo -n` never asks for a password: it works where sudo is set up without one, and fails at once where it isn't. */
export function actionScript(verb: Verb, unit: string, sudo: boolean): string {
  if (!VERBS.includes(verb)) throw new Error("Unknown action.");
  const err = unitError(unit);
  if (err) throw new Error(err);
  return `${sudo ? "sudo -n " : ""}systemctl ${verb} -- ${shq(unit)} 2>&1`;
}

export function statusScript(unit: string): string {
  const err = unitError(unit);
  if (err) throw new Error(err);
  return `systemctl status --no-pager -l -n 15 -- ${shq(unit)} 2>&1`;
}

export type Tone = "good" | "bad" | "warn" | "muted";

/** A colour for a unit's state: running is good, failed is bad, the rest is quiet. */
export function toneOf(u: Pick<Unit, "active" | "sub">): Tone {
  if (u.active === "failed") return "bad";
  if (u.active === "activating" || u.active === "deactivating" || u.active === "reloading") return "warn";
  if (u.active === "active" && u.sub === "running") return "good";
  if (u.active === "active") return "good";
  return "muted";
}
