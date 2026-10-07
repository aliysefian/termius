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
}

export const MARK_NO_SYSTEMD = "@@NO-SYSTEMD";
const MARK_FILES = "@@FILES";

/** Prints the units, then a marker, then which are enabled. A host without systemd says so. */
export const LIST_SCRIPT = [
  "command -v systemctl >/dev/null 2>&1 || { echo '" + MARK_NO_SYSTEMD + "'; exit 0; }",
  "systemctl list-units --type=service --all --no-legend --plain --no-pager 2>/dev/null",
  "echo '" + MARK_FILES + "'",
  "systemctl list-unit-files --type=service --no-legend --no-pager 2>/dev/null",
].join("; ");

export function parseUnits(output: string): { systemd: boolean; units: Unit[] } {
  if (output.includes(MARK_NO_SYSTEMD)) return { systemd: false, units: [] };
  const [listed, files = ""] = output.split(MARK_FILES);
  const enabled = new Map<string, string>();
  for (const line of files.split("\n")) {
    const f = line.trim().split(/\s+/);
    if (f.length >= 2 && f[0].endsWith(".service")) enabled.set(f[0], f[1]);
  }
  const units: Unit[] = [];
  for (const raw of listed.split("\n")) {
    // A failed unit is marked with a bullet in some versions, even with --plain.
    const line = raw.replace(/^[●○×*]\s*/, "").trim();
    const m = /^(\S+\.service)\s+(\S+)\s+(\S+)\s+(\S+)\s*(.*)$/.exec(line);
    if (!m) continue;
    units.push({ name: m[1], load: m[2], active: m[3], sub: m[4], description: m[5], enabled: enabled.get(m[1]) ?? "" });
  }
  units.sort((a, b) => a.name.localeCompare(b.name));
  return { systemd: true, units };
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

export type Tone = "good" | "bad" | "muted";

/** A colour for a unit's state: running is good, failed is bad, the rest is quiet. */
export function toneOf(u: Pick<Unit, "active" | "sub">): Tone {
  if (u.active === "failed") return "bad";
  if (u.active === "active" && u.sub === "running") return "good";
  if (u.active === "active") return "good";
  return "muted";
}
