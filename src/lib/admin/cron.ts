// A host's scheduled jobs, read-only: whoever is connected's crontab, the system's cron files, other users' crontabs where
// they can be read, the daily and weekly directories, and systemd timers (which are not cron and are shown apart).
import SCRIPT from "./cron.sh?raw";
import { describeCron, MACROS, validateCron } from "./cronparse";
import { complete, nonEmpty, sections, type Limit } from "./sections";

export const CRON_SCRIPT: string = SCRIPT.replace(/\r\n/g, "\n");

export type CronScope = "user" | "system" | "cron.d" | "spool";

export interface CronJob {
  scope: CronScope;
  /** The account the job runs as: the crontab's owner, or the user column of a system file. */
  user: string;
  schedule: string;
  command: string;
  /** Where it was found: "your crontab", "/etc/crontab", "/etc/cron.d/backup", "alice's crontab". */
  source: string;
  line: number;
  /** The schedule in words, or why it is not one. */
  summary: string;
  valid: boolean;
}

export interface CronEnv {
  source: string;
  name: string;
  value: string;
}

export interface Timer {
  unit: string;
  activates: string;
  description: string;
  active: string;
  next: string;
  last: string;
}

export interface CronReport {
  me: string;
  jobs: CronJob[];
  env: CronEnv[];
  periodic: { period: string; script: string }[];
  timers: Timer[];
  /** Cron daemons that are running (cron, crond, cronie, anacron). Empty means none found running, or unknown. */
  daemons: string[];
  limits: Limit[];
}

const ENV_RE = /^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/;

/** The jobs and variables of one crontab. `withUser`: system files carry the account to run as before the command. */
export function parseCrontab(lines: string[], source: string, scope: CronScope, owner: string, withUser: boolean): { jobs: CronJob[]; env: CronEnv[] } {
  const jobs: CronJob[] = [];
  const env: CronEnv[] = [];
  lines.forEach((raw, i) => {
    const l = raw.trim();
    if (!l || l.startsWith("#")) return;
    const e = ENV_RE.exec(l);
    // A schedule never starts with a letter or "=", so `MAILTO=x` is a variable and `* * * * * a=b cmd` is a job.
    if (e && !/^[\d*@]/.test(l)) {
      env.push({ source, name: e[1], value: e[2].replace(/^["']|["']$/g, "") });
      return;
    }
    const t = l.split(/\s+/);
    const macro = t[0].startsWith("@");
    const nSched = macro ? 1 : 5;
    if (t.length < nSched + (withUser ? 2 : 1)) return;
    const schedule = t.slice(0, nSched).join(" ");
    const user = withUser ? t[nSched] : owner;
    const command = t.slice(nSched + (withUser ? 1 : 0)).join(" ");
    const bad = validateCron(schedule);
    jobs.push({ scope, user, schedule, command, source, line: i + 1, summary: bad ?? describeCron(schedule), valid: bad === null });
  });
  return { jobs, env };
}

/** `#FILE path` blocks into [path, lines]. */
function files(lines: string[]): [string, string[]][] {
  const out: [string, string[]][] = [];
  for (const l of lines) {
    if (l.startsWith("#FILE ")) out.push([l.slice(6).trim(), []]);
    else out[out.length - 1]?.[1].push(l);
  }
  return out;
}

/** The part of a timestamp `systemctl show` prints that is worth showing: "2026-11-08 12:00:00 UTC". */
export function timerTime(raw: string): string {
  const m = /(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?: \S+)?)/.exec(raw);
  return m ? m[1] : "";
}

export function parseTimers(lines: string[]): Timer[] {
  const out: Timer[] = [];
  let cur: Record<string, string> = {};
  const flush = () => {
    if (cur.Id?.endsWith(".timer")) out.push({ unit: cur.Id, activates: cur.Unit ?? "", description: cur.Description ?? "", active: cur.ActiveState ?? "", next: timerTime(cur.NextElapseUSecRealtime ?? ""), last: timerTime(cur.LastTriggerUSec ?? "") });
    cur = {};
  };
  for (const l of lines) {
    if (l.trim() === "") flush();
    else {
      const i = l.indexOf("=");
      if (i > 0) cur[l.slice(0, i)] = l.slice(i + 1);
    }
  }
  flush();
  return out.slice(0, 500);
}

export function parseCron(out: string): CronReport | null {
  const s = sections(out);
  if (!complete(s)) return null;
  const me = nonEmpty(s.get("ME"))[0]?.trim() ?? "";
  const limits: Limit[] = [];
  const jobs: CronJob[] = [];
  const env: CronEnv[] = [];

  const user = nonEmpty(s.get("USER"));
  if (user[0]?.startsWith("missing")) limits.push({ what: "Your crontab", reason: "`crontab` is not installed on this host." });
  else if (user[0] && /no crontab for/i.test(user[0])) {
    // Nothing scheduled by this user: not a problem.
  } else if (user[0] && /^(crontab:|.*(not allowed|permission denied|cannot|can't))/i.test(user[0])) limits.push({ what: "Your crontab", reason: user[0].trim() });
  else {
    const r = parseCrontab(s.get("USER") ?? [], "your crontab", "user", me, false);
    jobs.push(...r.jobs);
    env.push(...r.env);
  }

  for (const [path, lines] of files(s.get("SYSTEM") ?? [])) {
    const r = parseCrontab(lines, path, path === "/etc/crontab" ? "system" : "cron.d", "root", true);
    jobs.push(...r.jobs);
    env.push(...r.env);
  }

  const spool = s.get("SPOOL") ?? [];
  if (spool.some((l) => l.startsWith("#UNREADABLE"))) limits.push({ what: "Other users' crontabs", reason: "They are kept in a directory only root can read. Reading with sudo shows them.", needsRoot: true });
  for (const [path, lines] of files(spool)) {
    const owner = path.split("/").pop() ?? "";
    if (owner === me && jobs.some((j) => j.scope === "user")) continue; // already read as `crontab -l`
    const r = parseCrontab(lines, `${owner}'s crontab`, "spool", owner, false);
    jobs.push(...r.jobs);
    env.push(...r.env);
  }

  const daemons = nonEmpty(s.get("DAEMON")).flatMap((l) => {
    const [name, state] = l.trim().split(/\s+/);
    return state === "active" ? [name] : [];
  });
  const periodic = nonEmpty(s.get("PERIODIC")).flatMap((l) => {
    const [period, script] = l.trim().split(/\s+/);
    return period && script ? [{ period, script }] : [];
  });
  return { me, jobs, env, periodic, timers: parseTimers(s.get("TIMERS") ?? []), daemons, limits };
}

export { MACROS };
