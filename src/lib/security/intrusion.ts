// Who is on the host and who is knocking: logged-in users, recent logins, fail2ban's jails and the failed
// SSH attempts in the logs. Parsers for what `who`, `last`, `fail2ban-client` and sshd's log lines look like, and
// the scripts that fetch them. Nothing here needs a host to test.
import { shq } from "$lib/ops/quote";
import { isIP } from "./net";

export interface LoginSession {
  user: string;
  /** `pts/3`, `tty1`. */
  tty: string;
  /** Where it came from; empty for a local login. */
  source: string;
  /** As the host printed it. */
  since: string;
  /** What is running on that terminal, when the host could say. */
  command: string;
}

export interface RecentLogin {
  user: string;
  tty: string;
  source: string;
  /** As the host printed it. */
  when: string;
  /** `still logged in`, or how long it lasted. */
  duration: string;
}

export interface Jail {
  name: string;
  currentlyFailed: number;
  totalFailed: number;
  currentlyBanned: number;
  totalBanned: number;
  banned: string[];
}

export interface Fail2ban {
  installed: boolean;
  running: boolean;
  jails: Jail[];
  /** Bans in the last 24 hours, from the log; null where it couldn't be counted. */
  bans24h: number | null;
}

export interface Attacker {
  ip: string;
  attempts: number;
  users: string[];
  /** The most recent attempt. */
  lastSeen: Date | null;
  country: string;
}

export const SESSIONS_SCRIPT = [
  "echo @@who; who 2>/dev/null",
  "echo @@cmds; for t in $(who 2>/dev/null | awk '{print $2}'); do printf '%s|' \"$t\"; ps -t \"$t\" -o args= --sort=start_time 2>/dev/null | tail -n 1; echo; done",
  "echo @@last; last -n 10 -a -F 2>/dev/null || last -n 10 -a 2>/dev/null",
  "echo @@end",
].join("; ");

/** `who`: `ali   pts/1   2026-10-20 06:57 (151.234.175.42)`. A `(tmux(123).%2)` in the brackets is no address. */
export function parseWho(text: string, commands: Map<string, string> = new Map()): LoginSession[] {
  const out: LoginSession[] = [];
  for (const line of text.split(/\r?\n/)) {
    const m = /^(\S+)\s+(\S+)\s+(\d{4}-\d\d-\d\d\s+\d\d:\d\d|\w{3}\s+\d+\s+\d\d:\d\d)\s*(?:\((.*)\))?\s*$/.exec(line.trim());
    if (!m) continue;
    const host = m[4] ?? "";
    out.push({ user: m[1], tty: m[2], since: m[3], source: /^tmux\(|^:\d/.test(host) ? "" : host, command: commands.get(m[2]) ?? "" });
  }
  return out;
}

export function parseCommands(text: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const line of text.split(/\r?\n/)) {
    const i = line.indexOf("|");
    if (i > 0) map.set(line.slice(0, i), line.slice(i + 1).trim());
  }
  return map;
}

const TIME = /^(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+\w{3}\s+\d+\s+(?:\d\d:\d\d(?::\d\d)?)(?:\s+\d{4})?/;

/** `last -n 10 -a -F`: user, tty, login time, then `still logged in` or `- <logout time> (duration)`, and the host last. */
export function parseLast(text: string): RecentLogin[] {
  const out: RecentLogin[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^(reboot|shutdown|wtmp|btmp)\b/.test(line)) continue;
    const head = /^(\S+)\s+(\S+)\s+(.*)$/.exec(line);
    if (!head) continue;
    const t = TIME.exec(head[3]);
    if (!t) continue;
    let rest = head[3].slice(t[0].length).trim();
    let duration = "";
    const still = /^still logged in\b/.exec(rest);
    if (still) {
      duration = "still logged in";
      rest = rest.slice(still[0].length).trim();
    } else {
      const gone = /^(?:-\s*)?(?:gone - no logout|crash|down|(?:\w{3}\s+\w{3}\s+\d+\s+[\d:]+(?:\s+\d{4})?|\d\d:\d\d))\s*(?:\(([^)]*)\))?/.exec(rest);
      if (gone) {
        duration = gone[1] ?? "";
        rest = rest.slice(gone[0].length).trim();
      }
    }
    const source = /^(?:tmux\(|:\d)/.test(rest) ? "" : rest.split(/\s+/)[0] ?? "";
    out.push({ user: head[1], tty: head[2], source, when: t[0], duration });
  }
  return out;
}

export function parseSessions(output: string): { current: LoginSession[]; recent: RecentLogin[] } {
  const s = new Map<string, string>();
  let name: string | null = null;
  let lines: string[] = [];
  for (const l of output.split(/\r?\n/)) {
    const m = /^@@([\w-]+)$/.exec(l);
    if (m) {
      if (name) s.set(name, lines.join("\n"));
      name = m[1];
      lines = [];
    } else lines.push(l);
  }
  if (name) s.set(name, lines.join("\n"));
  return { current: parseWho(s.get("who") ?? "", parseCommands(s.get("cmds") ?? "")), recent: parseLast(s.get("last") ?? "") };
}

/** Ends everything on a terminal. `pts/3` and `tty1` only: the name is checked, never trusted. */
export function killSessionScript(tty: string): string {
  if (!/^(?:pts\/\d+|tty\d+)$/.test(tty)) throw new Error("That isn't a terminal name.");
  return `pkill -KILL -t ${shq(tty)}`;
}

// -- fail2ban -----------------------------------------------------------------------------------------------------

/** Jail names are single words, safe as an argument. */
export const JAIL = /^[\w.-]{1,64}$/;

export const FAIL2BAN_SCRIPT = [
  "echo @@f2b; command -v fail2ban-client >/dev/null 2>&1 || { echo NOT-INSTALLED; echo @@end; exit 0; }",
  "fail2ban-client status 2>&1",
  "for j in $(fail2ban-client status 2>/dev/null | sed -n 's/.*Jail list:[[:space:]]*//p' | tr -d ','); do echo \"@@jail $j\"; fail2ban-client status \"$j\" 2>&1; done",
  // Bans in the last 24 hours: the journal if the service logs there, else the log file's recent lines.
  "echo @@bans; { journalctl -u fail2ban --since '24 hours ago' --no-pager 2>/dev/null | grep -c ' Ban ' ; } || true",
  "echo @@end",
].join("; ");

export function parseFail2ban(output: string): Fail2ban {
  const blocks: { name: string; text: string }[] = [];
  let cur: { name: string; text: string } | null = null;
  for (const line of output.split(/\r?\n/)) {
    const m = /^@@(\w+)(?:\s+(.*))?$/.exec(line);
    if (m) {
      cur = { name: m[1] === "jail" ? `jail:${m[2]}` : m[1], text: "" };
      blocks.push(cur);
    } else if (cur) cur.text += line + "\n";
  }
  const f2b = blocks.find((b) => b.name === "f2b")?.text ?? "";
  if (/NOT-INSTALLED/.test(f2b)) return { installed: false, running: false, jails: [], bans24h: null };
  if (/Failed to access socket|Permission denied|Unable to contact server|Connection refused/i.test(f2b) && !/Jail list/.test(f2b)) {
    return { installed: true, running: false, jails: [], bans24h: null };
  }
  const num = (text: string, label: string) => Number(new RegExp(`${label}:\\s*(\\d+)`).exec(text)?.[1] ?? 0);
  const jails: Jail[] = [];
  for (const b of blocks) {
    if (!b.name.startsWith("jail:")) continue;
    const list = /Banned IP list:\s*(.*)$/m.exec(b.text)?.[1] ?? "";
    jails.push({
      name: b.name.slice(5),
      currentlyFailed: num(b.text, "Currently failed"),
      totalFailed: num(b.text, "Total failed"),
      currentlyBanned: num(b.text, "Currently banned"),
      totalBanned: num(b.text, "Total banned"),
      banned: list.split(/\s+/).filter(isIP),
    });
  }
  const bans = /^(\d+)\s*$/m.exec(blocks.find((b) => b.name === "bans")?.text ?? "");
  return { installed: true, running: true, jails, bans24h: bans ? Number(bans[1]) : null };
}

export function banScript(jail: string, ip: string, ban: boolean): string {
  if (!JAIL.test(jail)) throw new Error("That isn't a jail name.");
  if (!isIP(ip)) throw new Error("That isn't an address.");
  return `fail2ban-client set ${shq(jail)} ${ban ? "banip" : "unbanip"} ${shq(ip)}`;
}

// -- failed logins ------------------------------------------------------------------------------------------------

/** sshd's log, newest last: the journal if the host has one, else the auth log (Debian, then Red Hat). */
export const AUTH_LOG_SCRIPT = [
  "echo @@now; date +%s",
  "echo @@log",
  "out=$(journalctl -q --no-pager -n 5000 -o short-iso -u ssh -u sshd 2>/dev/null)",
  "if [ -n \"$out\" ]; then printf '%s\\n' \"$out\"; elif [ -r /var/log/auth.log ]; then tail -n 5000 /var/log/auth.log; elif [ -r /var/log/secure ]; then tail -n 5000 /var/log/secure; fi",
  "echo @@end",
].join("; ");

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** An ISO timestamp (journalctl `short-iso`, recent rsyslog) or classic syslog `Oct 20 10:15:03`, which has no year. */
export function parseLogTime(line: string, now: Date): Date | null {
  const iso = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.\d+)?([+-]\d\d:?\d\d|Z)?/.exec(line);
  if (iso) {
    const tz = iso[2] ? (iso[2] === "Z" ? "Z" : iso[2].replace(/^([+-]\d\d)(\d\d)$/, "$1:$2")) : "Z";
    const d = new Date(iso[1] + tz);
    return Number.isNaN(d.getTime()) ? null : d;
  }
  const sys = /^(\w{3})\s+(\d+)\s+(\d\d):(\d\d):(\d\d)/.exec(line);
  if (!sys || !MONTHS.includes(sys[1])) return null;
  const d = new Date(Date.UTC(now.getUTCFullYear(), MONTHS.indexOf(sys[1]), Number(sys[2]), Number(sys[3]), Number(sys[4]), Number(sys[5])));
  // A December line read in January belongs to last year.
  if (d.getTime() - now.getTime() > 24 * 3600 * 1000) d.setUTCFullYear(d.getUTCFullYear() - 1);
  return d;
}

const FAILED = /Failed (?:password|publickey|keyboard-interactive\/pam) for (?:invalid user )?(\S+) from (\S+) port \d+/;
const INVALID = /Invalid user (\S*) from (\S+)(?: port \d+)?/;

export interface AuthLogSummary {
  attackers: Attacker[];
  /** Failed attempts in the hour before `now`. */
  failedLastHour: number;
  failedTotal: number;
}

/** Group failed logins by source address. An invalid-user line and the failed-password line after it are one attempt. */
export function summarizeAuthLog(log: string, now: Date, top = 20): AuthLogSummary {
  const by = new Map<string, { failed: number; invalid: number; users: Set<string>; last: Date | null }>();
  let lastHour = 0;
  let total = 0;
  for (const line of log.split(/\r?\n/)) {
    const f = FAILED.exec(line);
    const i = f ? null : INVALID.exec(line);
    const m = f ?? i;
    if (!m || !isIP(m[2])) continue;
    const e = by.get(m[2]) ?? { failed: 0, invalid: 0, users: new Set<string>(), last: null };
    if (f) e.failed++;
    else e.invalid++;
    if (m[1]) e.users.add(m[1]);
    const t = parseLogTime(line, now);
    if (t && (!e.last || t > e.last)) e.last = t;
    if (f) {
      total++;
      if (t && now.getTime() - t.getTime() <= 3600 * 1000 && t <= new Date(now.getTime() + 60_000)) lastHour++;
    }
    by.set(m[2], e);
  }
  const attackers = [...by.entries()]
    .map(([ip, e]) => ({ ip, attempts: Math.max(e.failed, e.invalid), users: [...e.users].slice(0, 6), lastSeen: e.last, country: "" }))
    .sort((a, b) => b.attempts - a.attempts || (b.lastSeen?.getTime() ?? 0) - (a.lastSeen?.getTime() ?? 0))
    .slice(0, top);
  return { attackers, failedLastHour: lastHour, failedTotal: total };
}

export function parseAuthOutput(output: string): { now: Date; log: string } {
  const now = /@@now\s+(\d+)/.exec(output.replace(/\n/g, " "));
  const log = output.split(/^@@log$\n?/m)[1]?.split(/^@@end$/m)[0] ?? "";
  return { now: now ? new Date(Number(now[1]) * 1000) : new Date(), log };
}

/** The country for each address, from `geoiplookup` if the host has it. Offline: nothing is sent anywhere. */
export function geoScript(ips: string[]): string {
  const clean = ips.filter(isIP).slice(0, 25);
  if (clean.length === 0) return "true";
  return `command -v geoiplookup >/dev/null 2>&1 || exit 0; for ip in ${clean.map(shq).join(" ")}; do printf '%s|' "$ip"; geoiplookup "$ip" 2>/dev/null | head -n 1; done`;
}

export function parseGeo(output: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const line of output.split(/\r?\n/)) {
    const m = /^([^|]+)\|.*?:\s*([A-Z]{2}),\s*(.+)$/.exec(line.trim());
    if (m) map.set(m[1], `${m[2]} ${m[3]}`);
  }
  return map;
}
