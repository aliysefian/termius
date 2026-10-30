// A read-only security audit of a Linux host: what hostaudit.sh collects, and the rules that judge it. The script only
// prints what it read; every decision (what is a weakness, how bad, how to fix it) is made here, so each rule is
// tested on recorded output without a connection.
//
// A check ends one of three ways. It passes, it fails (with a severity, what was found, the risk and the fix), or it
// is "unknown": the connected account could not read what the check needs (the sshd drop-ins, sudoers, the firewall
// rules are root's). An unknown check is never counted as a pass, and says how to check it as root.
//
// Nothing here changes a host. The fixes are text to read and apply yourself.
import SCRIPT from "./hostaudit.sh?raw";
import { parsePorts, parseSections, type ListeningPort } from "./hostdetail";

// A checkout with Windows line endings would make the remote shell fail to parse it.
export const AUDIT_SCRIPT: string = SCRIPT.replace(/\r\n/g, "\n");

export type Severity = "critical" | "high" | "medium" | "low" | "info";
export const SEVERITIES: Severity[] = ["critical", "high", "medium", "low", "info"];
export const SEVERITY_NAMES: Record<Severity, string> = { critical: "Critical", high: "High", medium: "Medium", low: "Low", info: "Info" };

export type Domain = "ssh" | "network" | "privilege" | "kernel" | "containers" | "patching";
export const DOMAINS: { id: Domain; name: string }[] = [
  { id: "ssh", name: "Identity & remote access (SSH)" },
  { id: "network", name: "Network perimeter & firewall" },
  { id: "privilege", name: "Privileges & users" },
  { id: "kernel", name: "Kernel hardening" },
  { id: "containers", name: "Containers" },
  { id: "patching", name: "Patching, logging & intrusion detection" },
];

export type Outcome = "pass" | "fail" | "unknown";

export interface Check {
  id: string;
  domain: Domain;
  title: string;
  outcome: Outcome;
  /** Only on a failure. */
  severity: Severity | null;
  /** The file, setting or service the check is about. */
  component: string;
  /** What was seen. For a pass, the evidence; for a failure, actual against expected; for unknown, why it could not be read. */
  finding: string;
  /** How an attacker uses it. Failures only. */
  risk: string;
  /** A failure: the fix. Unknown: how to check it as root. */
  remediation: string;
}

export interface Score {
  passed: number;
  failed: number;
  unknown: number;
  /** Passed of the checks that could be judged; null when none could. */
  percent: number | null;
  /** The same figure if every unknown check turned out to fail, and if every one passed. */
  worst: number | null;
  best: number | null;
}

export interface AuditReport {
  /** False for anything but Linux. */
  supported: boolean;
  /** The script reached its end. False when the connection or a timeout cut it short. */
  complete: boolean;
  os: string;
  hostname: string | null;
  distro: string | null;
  kernel: string | null;
  user: string | null;
  /** Ran as root, so nothing was out of reach. */
  root: boolean;
  checks: Check[];
  score: Score;
  bySeverity: Record<Severity, number>;
}

// -- small helpers ----------------------------------------------------------------------------------------------------

/** `key=value` lines, first one wins. */
function keyValues(lines: string[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const l of lines) {
    const i = l.indexOf("=");
    if (i > 0 && !out.has(l.slice(0, i))) out.set(l.slice(0, i), l.slice(i + 1).trim());
  }
  return out;
}

const int = (s: string | undefined): number | null => {
  if (s === undefined || !/^-?\d+$/.test(s.trim())) return null;
  return Number(s.trim());
};

const uniq = (items: string[]) => [...new Set(items)];
const list = (items: string[], max = 6): string => {
  const all = uniq(items);
  return all.length <= max ? all.join(", ") : `${all.slice(0, max).join(", ")} and ${all.length - max} more`;
};
/** "a", "a and b", "a, b and c". */
const and = (items: string[]): string => (items.length < 2 ? (items[0] ?? "") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`);

const isLoopbackAddress = (a: string) => /^127\./.test(a) || a === "::1" || a === "::ffff:127.0.0.1";

/** Files named by a `@file` or `@denied` line, each with what it held. */
interface FileDump {
  path: string;
  denied: boolean;
  lines: string[];
}

function parseFiles(lines: string[]): FileDump[] {
  const out: FileDump[] = [];
  let cur: FileDump | null = null;
  for (const l of lines) {
    if (l.startsWith("@file ")) out.push((cur = { path: l.slice(6).trim(), denied: false, lines: [] }));
    else if (l.startsWith("@denied ")) out.push((cur = { path: l.slice(8).trim(), denied: true, lines: [] }));
    else if (cur && l.trim() !== "") cur.lines.push(l);
  }
  return out;
}

// -- sshd ---------------------------------------------------------------------------------------------------------------

/** What is known about one sshd keyword: `known` is false when an unreadable file could be deciding it. */
export interface Setting {
  known: boolean;
  /** Every value given (a keyword such as Port or AllowGroups may be given more than once); empty: not set, so the default applies. */
  values: string[];
}

export interface SshConfig {
  /** `none`: no sshd is installed. `effective`: `sshd -T`, so complete. `files`: read from the files this account could open. */
  mode: "none" | "effective" | "files";
  get(keyword: string): Setting;
  /** Names of the files that could not be read. */
  denied: string[];
}

// Keywords that add up over several lines; for the rest the first value wins.
const ACCUMULATING = new Set(["port", "allowusers", "allowgroups", "denyusers", "denygroups", "listenaddress"]);
const DROPIN_DIR = "/etc/ssh/sshd_config.d/";

export function parseSshd(lines: string[]): SshConfig {
  const mode = /^mode=(\w+)/.exec(lines[0] ?? "")?.[1];
  if (mode === "effective") {
    const seen = new Map<string, string[]>();
    for (const l of lines.slice(1)) {
      const m = /^(\S+)\s+(.*)$/.exec(l.trim());
      if (!m) continue;
      const k = m[1].toLowerCase();
      seen.set(k, [...(seen.get(k) ?? []), m[2].trim()]);
    }
    return { mode: "effective", get: (k) => ({ known: true, values: seen.get(k.toLowerCase()) ?? [] }), denied: [] };
  }
  if (mode !== "files") return { mode: "none", get: () => ({ known: true, values: [] }), denied: [] };

  const files = parseFiles(lines.slice(1));
  const main = files.find((f) => f.path === "/etc/ssh/sshd_config");
  const dropins = files.filter((f) => f.path.startsWith(DROPIN_DIR) && f.path.endsWith(".conf")).sort((a, b) => a.path.localeCompare(b.path));
  const dirDenied = files.some((f) => f.path === DROPIN_DIR);
  const denied = files.filter((f) => f.denied).map((f) => f.path);

  // The settings in the order sshd meets them. A file that can't be read is a gap: whatever comes after it can be
  // overridden by it, and a keyword nobody sets might be set in it.
  type Entry = { key: string; value: string; afterGap: boolean };
  const entries: Entry[] = [];
  let gap = false;
  const addFile = (f: FileDump) => {
    if (f.denied) {
      gap = true;
      return;
    }
    for (const l of f.lines) {
      const m = /^(\S+)\s*=?\s*(.*)$/.exec(l.trim());
      if (!m) continue;
      const key = m[1].toLowerCase();
      if (key === "match") break; // what follows applies only to some connections
      if (key === "include") {
        const patterns = m[2].split(/\s+/);
        for (const p of patterns) {
          if (p === `${DROPIN_DIR}*.conf` || p === "sshd_config.d/*.conf") {
            if (dirDenied) gap = true;
            dropins.forEach(addFile);
          } else {
            gap = true; // an include we did not collect
          }
        }
        continue;
      }
      entries.push({ key, value: m[2].trim(), afterGap: gap });
    }
  };
  if (main) addFile(main);
  else gap = true;

  return {
    mode: "files",
    denied,
    get(keyword) {
      const k = keyword.toLowerCase();
      const hits = entries.filter((e) => e.key === k);
      if (ACCUMULATING.has(k)) return { known: !gap, values: hits.map((h) => h.value) };
      if (hits.length === 0) return { known: !gap, values: [] };
      // The first one wins, unless a file we couldn't read comes before it.
      return hits[0].afterGap ? { known: false, values: [] } : { known: true, values: [hits[0].value] };
    },
  };
}

// -- the firewall ---------------------------------------------------------------------------------------------------------

export type Reach = "open" | "closed" | "unknown";

interface UfwRule {
  /** `null`: every port. */
  ports: [number, number][] | null;
  proto: "tcp" | "udp" | "any";
  /** Open to the whole network, not just one address or range. */
  fromAnywhere: boolean;
}

export interface Firewall {
  /** True: something is known to be filtering. False: known not to be. Null: could not be told. */
  active: boolean | null;
  evidence: string;
  inputPolicy: "drop" | "accept" | null;
  forwardPolicy: "drop" | "accept" | null;
  /** Whether a port is let in from anywhere, when the rules were readable. */
  reach(port: number, proto: "tcp" | "udp"): Reach;
}

const unquote = (s: string) => s.trim().replace(/^"|"$/g, "");

function policyWord(s: string | undefined): "drop" | "accept" | null {
  const w = (s ?? "").toLowerCase();
  if (w === "drop" || w === "deny" || w === "reject") return "drop";
  if (w === "accept" || w === "allow") return "accept";
  return null;
}

/** `ufw status verbose`. Rules that are not a plain port (an application profile, a named address) count as unparsed. */
export function parseUfwStatus(lines: string[]): { active: boolean | null; defaultIn: "drop" | "accept" | null; defaultRouted: "drop" | "accept" | null; rules: UfwRule[]; unparsed: number } {
  let active: boolean | null = null;
  let defaultIn: "drop" | "accept" | null = null;
  let defaultRouted: "drop" | "accept" | null = null;
  const rules: UfwRule[] = [];
  let unparsed = 0;
  let inRules = false;
  for (const l of lines) {
    const status = /^Status:\s*(\w+)/i.exec(l);
    if (status) active = status[1].toLowerCase() === "active";
    const def = /^Default:\s*(.*)$/i.exec(l);
    if (def) {
      defaultIn = policyWord(/(\w+)\s*\(incoming\)/i.exec(def[1])?.[1]);
      defaultRouted = policyWord(/(\w+)\s*\(routed\)/i.exec(def[1])?.[1]);
    }
    if (/^--/.test(l.trim())) {
      inRules = true;
      continue;
    }
    if (!inRules || l.trim() === "") continue;
    const cols = l.trim().split(/\s{2,}/);
    if (cols.length < 3) {
      unparsed++;
      continue;
    }
    const action = /^(ALLOW|LIMIT|DENY|REJECT)(?:\s+(IN|OUT|FWD))?$/i.exec(cols[1]);
    if (!action) {
      unparsed++;
      continue;
    }
    // Only what lets traffic in matters here.
    if (/^(DENY|REJECT)$/i.test(action[1]) || (action[2] && action[2].toUpperCase() !== "IN")) continue;
    const to = cols[0].replace(/\s*\(v6\)\s*$/, "");
    const fromAnywhere = /^Anywhere(\s*\(v6\))?$/i.test(cols[2].trim());
    if (/^Anywhere$/i.test(to)) {
      rules.push({ ports: null, proto: "any", fromAnywhere });
      continue;
    }
    const m = /^(\d+(?::\d+)?(?:,\d+(?::\d+)?)*)(?:\/(tcp|udp))?$/.exec(to);
    if (!m) {
      unparsed++;
      continue;
    }
    const ports = m[1].split(",").map((r): [number, number] => {
      const [a, b] = r.split(":").map(Number);
      return [a, b ?? a];
    });
    rules.push({ ports, proto: (m[2] as "tcp" | "udp" | undefined) ?? "any", fromAnywhere });
  }
  return { active, defaultIn, defaultRouted, rules, unparsed };
}

function readFirewall(sec: Map<string, string[]>, services: Set<string> | null, root: boolean): Firewall {
  const lines = sec.get("FIREWALL") ?? [];
  const part = (name: string) => {
    const out: string[] = [];
    let on = false;
    for (const l of lines) {
      if (l.startsWith("@")) on = l === `@${name}`;
      else if (on) out.push(l);
    }
    return out;
  };
  const conf = new Map<string, string>();
  for (const l of part("ufwconf")) {
    const i = l.indexOf("=");
    if (i > 0) conf.set(l.slice(0, i), unquote(l.slice(i + 1)));
  }
  const ufw = parseUfwStatus(part("ufw"));
  const iptables = part("iptables");
  const nft = part("nft");
  const never: Firewall["reach"] = () => "unknown";

  // 1. ufw's own report, which only root gets.
  if (ufw.active !== null) {
    const reach: Firewall["reach"] = (port, proto) => {
      if (!ufw.active) return "open";
      if (ufw.defaultIn === "accept") return "open";
      const hit = ufw.rules.some((r) => r.fromAnywhere && (r.proto === "any" || r.proto === proto) && (r.ports === null || r.ports.some(([a, b]) => port >= a && port <= b)));
      if (hit) return "open";
      return ufw.unparsed > 0 || ufw.defaultIn === null ? "unknown" : "closed";
    };
    return {
      active: ufw.active,
      evidence: `ufw status: ${ufw.active ? "active" : "inactive"}`,
      inputPolicy: ufw.defaultIn,
      forwardPolicy: ufw.defaultRouted,
      reach,
    };
  }
  // 2. ufw's settings files, which anyone can read.
  if (conf.has("ENABLED")) {
    const enabled = conf.get("ENABLED")?.toLowerCase() === "yes";
    const running = services === null || services.has("ufw.service");
    return {
      active: enabled && running,
      evidence: enabled ? "/etc/ufw/ufw.conf: ENABLED=yes" : "/etc/ufw/ufw.conf: ENABLED=no",
      inputPolicy: enabled ? policyWord(conf.get("DEFAULT_INPUT_POLICY")) : null,
      forwardPolicy: enabled ? policyWord(conf.get("DEFAULT_FORWARD_POLICY")) : null,
      reach: never,
    };
  }
  // 3. firewalld
  if (services?.has("firewalld.service")) return { active: true, evidence: "firewalld.service is active", inputPolicy: null, forwardPolicy: null, reach: never };
  // 4. iptables or nftables, readable only by root
  const ipt = iptables.map((l) => /^-P INPUT (\w+)/.exec(l)?.[1]).find(Boolean);
  const iptFwd = iptables.map((l) => /^-P FORWARD (\w+)/.exec(l)?.[1]).find(Boolean);
  const iptRules = iptables.filter((l) => l.startsWith("-A ")).length;
  const nftIn = /hook input[^\n;]*;\s*policy\s+(\w+);/.exec(nft.join("\n"))?.[1];
  const nftFwd = /hook forward[^\n;]*;\s*policy\s+(\w+);/.exec(nft.join("\n"))?.[1];
  if (ipt || nftIn) {
    const inputPolicy = policyWord(nftIn ?? ipt);
    const hasRules = iptRules > 0 || nft.some((l) => /\b(accept|drop|reject)\b/.test(l) && !/policy/.test(l));
    return {
      active: inputPolicy === "drop" || hasRules,
      evidence: nftIn ? "nft list ruleset" : "iptables -S",
      inputPolicy,
      forwardPolicy: policyWord(nftFwd ?? iptFwd),
      reach: never,
    };
  }
  if (services?.has("nftables.service") || services?.has("netfilter-persistent.service")) {
    return { active: null, evidence: "the nftables service is active but its rules are not readable", inputPolicy: null, forwardPolicy: null, reach: never };
  }
  if (root) return { active: false, evidence: "no ufw, firewalld, iptables or nftables rules were found, as root", inputPolicy: null, forwardPolicy: null, reach: never };
  return { active: null, evidence: "no firewall service is running, and the rules themselves need root to read", inputPolicy: null, forwardPolicy: null, reach: never };
}

// -- accounts -------------------------------------------------------------------------------------------------------------

interface Account {
  name: string;
  uid: number;
  shell: string;
  passwordField: string;
}

function parsePasswd(lines: string[]): Account[] {
  const out: Account[] = [];
  for (const l of lines) {
    const f = l.split(":");
    if (f.length < 7) continue;
    const uid = int(f[2]);
    if (uid === null) continue;
    out.push({ name: f[0], passwordField: f[1], uid, shell: f[6].trim() });
  }
  return out;
}

const INTERACTIVE_SHELL = /\/(bash|sh|dash|zsh|ksh|fish|csh|tcsh|ash)$/;
// Accounts whose shell is a deliberate, harmless one.
const SPECIAL_SHELL = new Set(["/bin/sync", "/sbin/shutdown", "/sbin/halt"]);

// -- sudoers ----------------------------------------------------------------------------------------------------------------

// Programs that give a shell, or a way to read or write any file, when sudo runs them without a fixed argument list.
const ESCAPABLE = new Set([
  "apt", "apt-get", "aptitude", "vi", "vim", "nano", "less", "more", "man", "find", "awk", "gawk", "python", "python3", "perl", "ruby", "php", "node", "lua",
  "bash", "sh", "dash", "zsh", "env", "tar", "zip", "rsync", "docker", "systemctl", "journalctl", "git", "ed", "ftp", "mount", "cp", "mv", "tee", "dd",
  "chmod", "chown", "nmap", "make", "wget", "curl", "su",
]);

export interface SudoRule {
  who: string;
  commands: string[];
}

/** The rules that run something without a password, one per command list. */
export function parseSudoers(lines: string[]): { rules: SudoRule[]; noAuthenticate: boolean } {
  const aliases = new Map<string, string[]>();
  const rules: SudoRule[] = [];
  let noAuthenticate = false;
  const splitCommands = (s: string) => s.split(",").map((c) => c.trim()).filter(Boolean);
  // Lines can be continued with a trailing backslash.
  const joined: string[] = [];
  let carry = "";
  for (const raw of lines) {
    const l = raw.replace(/\r$/, "");
    if (l.endsWith("\\")) carry += l.slice(0, -1) + " ";
    else {
      joined.push(carry + l);
      carry = "";
    }
  }
  for (const l of joined) {
    const t = l.trim();
    const alias = /^Cmnd_Alias\s+(\w+)\s*=\s*(.*)$/.exec(t);
    if (alias) {
      aliases.set(alias[1], splitCommands(alias[2]));
      continue;
    }
    if (/^Defaults\b.*!\s*authenticate/.test(t)) noAuthenticate = true;
    const m = /^(\S+)\s+(.+?)=\s*(?:\([^)]*\)\s*)?(.*NOPASSWD:.*)$/.exec(t);
    if (!m) continue;
    const afterTag = m[3].slice(m[3].lastIndexOf("NOPASSWD:") + "NOPASSWD:".length).replace(/^\s*(?:[A-Z]+:\s*)*/, "");
    const commands = splitCommands(afterTag).flatMap((c) => aliases.get(c) ?? [c]);
    rules.push({ who: m[1], commands });
  }
  return { rules, noAuthenticate };
}

// -- set-uid files ----------------------------------------------------------------------------------------------------------

// Set-uid or set-gid programs that ship with the common distributions (basename only).
const STANDARD_SUID = new Set([
  "chage", "chfn", "chsh", "crontab", "expiry", "fusermount", "fusermount3", "gpasswd", "mount", "newgidmap", "newgrp", "newuidmap", "passwd", "ssh-agent", "su", "sudo", "sudoedit",
  "umount", "dbus-daemon-launch-helper", "ssh-keysign", "polkit-agent-helper-1", "utempter", "pam_extrausers_chkpwd", "unix_chkpwd", "wall", "write", "bsd-write", "at", "ping", "ping6",
  "traceroute6", "traceroute6.iputils", "pkexec", "userhelper", "usernetctl", "mount.nfs", "mount.cifs", "plocate", "locate", "mlocate", "Xorg.wrap", "snap-confine", "ksu", "staprun",
  "lockdev", "unix2_chkpwd", "pt_chown", "ssh-sk-helper", "chrome-sandbox", "dotlockfile", "mail-lock", "mail-touchlock", "mail-unlock", "expiry", "gnome-pty-helper", "vmware-user-suid-wrapper",
  "mtr-packet", "sg", "procmail", "lockfile", "postdrop", "postqueue", "ssmtp", "sendmail", "Xwrapper", "start-stop-daemon", "fping", "fping6", "screen", "uuidd", "pppd", "fdmount",
]);

// Programs that give a root shell, or read or write any file, once they are set-uid.
const DANGEROUS_SUID = new Set([
  "bash", "sh", "dash", "zsh", "ksh", "csh", "tcsh", "fish", "ash", "find", "vi", "vim", "vim.basic", "vim.tiny", "nano", "less", "more", "awk", "gawk", "mawk", "nawk", "python", "python2", "python3",
  "perl", "ruby", "php", "node", "lua", "env", "cp", "mv", "tee", "dd", "tar", "zip", "unzip", "nmap", "docker", "systemctl", "man", "ed", "ftp", "gdb", "strace", "tcpdump", "xxd", "base64",
  "make", "wget", "curl", "rsync", "chmod", "chown", "mount.ntfs", "busybox", "openssl", "socat", "nc", "ncat", "netcat",
]);

const basename = (p: string) => p.slice(p.lastIndexOf("/") + 1);

// -- the audit --------------------------------------------------------------------------------------------------------------

const DB_PORTS: Record<number, string> = {
  1433: "SQL Server", 1521: "Oracle", 2379: "etcd", 3306: "MySQL/MariaDB", 5432: "PostgreSQL", 5984: "CouchDB", 6379: "Redis", 9200: "Elasticsearch", 11211: "Memcached", 27017: "MongoDB",
};
// Dev servers: unauthenticated, and they serve the project's source.
const DEV_PORTS = new Set([3000, 3001, 4173, 4200, 5000, 5173, 8000, 8080, 8081, 8888]);
// Debug ports that hand out code execution.
const DEBUG_PORTS: Record<number, string> = { 9229: "the Node.js inspector", 9222: "Chrome DevTools" };

const ROOT_FIX = "Run the same check as root: ";

class Builder {
  checks: Check[] = [];
  add(domain: Domain, id: string, title: string, component: string) {
    const base = { id, domain, title, component };
    const push = (c: Omit<Check, "id" | "domain" | "title" | "component">) => this.checks.push({ ...base, ...c });
    return {
      pass: (finding: string) => push({ outcome: "pass", severity: null, finding, risk: "", remediation: "" }),
      fail: (severity: Severity, finding: string, risk: string, remediation: string) => push({ outcome: "fail", severity, finding, risk, remediation }),
      unknown: (finding: string, how: string) => push({ outcome: "unknown", severity: null, finding, risk: "", remediation: how }),
    };
  }
}

function scoreOf(checks: Check[]): Score {
  const passed = checks.filter((c) => c.outcome === "pass").length;
  const failed = checks.filter((c) => c.outcome === "fail").length;
  const unknown = checks.filter((c) => c.outcome === "unknown").length;
  const pct = (n: number, d: number) => (d === 0 ? null : Math.round((100 * n) / d));
  return { passed, failed, unknown, percent: pct(passed, passed + failed), worst: pct(passed, passed + failed + unknown), best: pct(passed + unknown, passed + failed + unknown) };
}

const emptyReport = (partial: Partial<AuditReport>): AuditReport => ({
  supported: true,
  complete: false,
  os: "",
  hostname: null,
  distro: null,
  kernel: null,
  user: null,
  root: false,
  checks: [],
  score: scoreOf([]),
  bySeverity: { critical: 0, high: 0, medium: 0, low: 0, info: 0 },
  ...partial,
});

/** Read the script's output and judge it. Never throws: output that makes no sense gives a report that says so. */
export function auditHost(stdout: string): AuditReport {
  const sec = parseSections(stdout);
  const host = keyValues(sec.get("HOST") ?? []);
  const os = host.get("os") ?? "";
  const complete = sec.has("END");
  const identity = {
    os,
    hostname: host.get("hostname") || null,
    distro: host.get("distro") || null,
    kernel: host.get("kernel") || null,
    user: host.get("user") || null,
    root: host.get("uid") === "0",
    complete,
  };
  if (!sec.has("HOST")) return emptyReport({ ...identity, supported: false });
  if (os !== "Linux") return emptyReport({ ...identity, supported: false });

  const b = new Builder();
  const root = identity.root;
  const authKeys = host.get("authkeys") === "yes";
  const nowSecs = int(host.get("now"));

  const svcLines = sec.get("SERVICES") ?? [];
  const services: Set<string> | null = svcLines.includes("listed=yes") ? new Set(svcLines.filter((l) => l.startsWith("active ")).map((l) => l.slice(7).trim())) : null;
  const svcOn = (...names: string[]) => (services === null ? null : names.some((n) => services.has(`${n}.service`)));

  const ssh = parseSshd(sec.get("SSHD") ?? []);
  const sshdHere = ssh.mode !== "none";
  const ports = parsePorts(sec.get("PORTS") ?? []);
  const fw = readFirewall(sec, services, root);
  // Port lines add up, so what the readable files give is right as far as it goes; an unreadable file could add more.
  const sshPorts = ((): number[] => {
    const given = ssh.get("port").values.map(Number).filter((n) => Number.isInteger(n));
    return given.length ? given : [22];
  })();

  const denied = ssh.denied.length ? `Could not read ${list(ssh.denied.map(basename), 3)}. ` : "";
  const sshHow = `${ROOT_FIX}\`sshd -T | grep -iE 'permitrootlogin|passwordauthentication|permitemptypasswords|pubkeyauthentication|maxauthtries|x11forwarding|clientalive|allowusers|allowgroups|ciphers|macs|kexalgorithms'\`.`;

  // ---- 1. SSH ----------------------------------------------------------------------------------------------------------------
  if (sshdHere) {
    const pw = ssh.get("passwordauthentication");
    const passwordOn = pw.known ? (pw.values[0] ?? "yes").toLowerCase() === "yes" : null;

    {
      const c = b.add("ssh", "ssh.root-login", "Root login over SSH", "sshd_config: PermitRootLogin");
      const s = ssh.get("permitrootlogin");
      const v = (s.values[0] ?? "prohibit-password").toLowerCase();
      if (!s.known) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if (v === "yes")
        c.fail("high", "PermitRootLogin is yes: root can log in over SSH.", "root is the account every brute-force run tries first, and a guessed or stolen root password is full control with no second step.", "Set `PermitRootLogin no` in a drop-in such as /etc/ssh/sshd_config.d/10-root.conf, run `sshd -t`, then `systemctl reload ssh`. First make sure a named user with sudo can log in.");
      else if (v !== "no" && passwordOn === true)
        c.fail("low", `PermitRootLogin is ${v} while password logins are on.`, "Root can log in with a key, so one leaked key or one unattended authorized_keys entry is root; the account is also the one every scan targets.", "Set `PermitRootLogin no` and use a named account with sudo, then `sshd -t` and `systemctl reload ssh`.");
      else c.pass(`PermitRootLogin ${s.values[0] ?? "(default) prohibit-password"}.`);
    }
    {
      const c = b.add("ssh", "ssh.password-login", "Password logins over SSH", "sshd_config: PasswordAuthentication");
      if (passwordOn === null) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if (passwordOn)
        c.fail("high", `PasswordAuthentication is ${pw.values[0] ?? "(default) yes"}${authKeys ? "; this account already has an authorized_keys file, so keys work" : ""}.`, "Passwords can be guessed or reused from another breach, over the network, forever. Keys cannot.", "Put `PasswordAuthentication no` in a drop-in (for example /etc/ssh/sshd_config.d/10-auth.conf), check that your key logs in from a second terminal first, then `sshd -t` and `systemctl reload ssh`.");
      else c.pass("PasswordAuthentication no.");
    }
    {
      const c = b.add("ssh", "ssh.empty-passwords", "Empty passwords over SSH", "sshd_config: PermitEmptyPasswords");
      const s = ssh.get("permitemptypasswords");
      if (!s.known) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if ((s.values[0] ?? "no").toLowerCase() === "yes")
        c.fail("critical", "PermitEmptyPasswords is yes.", "An account with no password can be logged into by anyone who can reach the port.", "Set `PermitEmptyPasswords no`, then `sshd -t` and `systemctl reload ssh`, and give every account a password or lock it.");
      else c.pass("PermitEmptyPasswords no.");
    }
    {
      const c = b.add("ssh", "ssh.pubkey", "Public-key logins", "sshd_config: PubkeyAuthentication");
      const s = ssh.get("pubkeyauthentication");
      if (!s.known) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if ((s.values[0] ?? "yes").toLowerCase() === "no")
        c.fail("medium", "PubkeyAuthentication is no.", "Without keys the only way in is a password.", "Set `PubkeyAuthentication yes` (the default), then `sshd -t` and `systemctl reload ssh`.");
      else c.pass("PubkeyAuthentication yes.");
    }
    {
      const c = b.add("ssh", "ssh.max-auth-tries", "Login attempts per connection", "sshd_config: MaxAuthTries");
      const s = ssh.get("maxauthtries");
      const n = int(s.values[0]) ?? 6;
      if (!s.known) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if (n > 4)
        c.fail("low", `MaxAuthTries is ${s.values[0] ?? "(default) 6"}; 4 or fewer is the usual limit.`, "Each connection gets more password or key guesses before it is cut off.", "Set `MaxAuthTries 3` in a drop-in, then `sshd -t` and `systemctl reload ssh`.");
      else c.pass(`MaxAuthTries ${n}.`);
    }
    {
      const c = b.add("ssh", "ssh.x11", "X11 forwarding", "sshd_config: X11Forwarding");
      const s = ssh.get("x11forwarding");
      if (!s.known) c.unknown(`${denied}The setting could be in a file this account can't read.`, sshHow);
      else if ((s.values[0] ?? "no").toLowerCase() === "yes")
        c.fail("low", "X11Forwarding is yes.", "A program on the server can watch and send keystrokes to the X display of whoever connects with -X.", "Set `X11Forwarding no`, then `sshd -t` and `systemctl reload ssh`, unless a graphical program is run over SSH here.");
      else c.pass("X11Forwarding no.");
    }
    {
      const c = b.add("ssh", "ssh.idle-timeout", "Idle sessions", "sshd_config: ClientAliveInterval, ClientAliveCountMax");
      const si = ssh.get("clientaliveinterval");
      const sc = ssh.get("clientalivecountmax");
      const interval = int(si.values[0]) ?? 0;
      const count = int(sc.values[0]) ?? 3;
      if (!si.known || !sc.known) c.unknown(`${denied}The settings could be in a file this account can't read.`, sshHow);
      else if (interval === 0 || count === 0)
        c.fail("low", `ClientAliveInterval is ${interval} and ClientAliveCountMax is ${count}: a dead or abandoned session is never closed.`, "A forgotten session on an unlocked laptop stays logged in.", "Set `ClientAliveInterval 300` and `ClientAliveCountMax 2` in a drop-in, then `sshd -t` and `systemctl reload ssh`.");
      else c.pass(`Idle clients are dropped after ${interval * count} s (${interval} s x ${count}).`);
    }
    {
      const c = b.add("ssh", "ssh.allow-list", "Who may log in over SSH", "sshd_config: AllowUsers, AllowGroups");
      const u = ssh.get("allowusers");
      const g = ssh.get("allowgroups");
      const given = [...u.values.map((v) => `AllowUsers ${v}`), ...g.values.map((v) => `AllowGroups ${v}`)];
      if (given.length) c.pass(`Restricted: ${list(given, 3)}.`);
      else if (!u.known || !g.known) c.unknown(`${denied}An allow list could be in a file this account can't read.`, sshHow);
      else
        c.fail("low", "Neither AllowUsers nor AllowGroups is set: every account with a valid login can use SSH, including service accounts.", "An account created for something else (a service, a test user) becomes a way in if it ever has a key or password.", "Add `AllowGroups sshusers` (or AllowUsers) to a drop-in after creating the group and adding the people who log in; check from a second terminal, then `sshd -t` and `systemctl reload ssh`.");
    }
    {
      const c = b.add("ssh", "ssh.algorithms", "Legacy ciphers, MACs and key exchange", "sshd_config: Ciphers, MACs, KexAlgorithms");
      const pick = (k: string) => ssh.get(k).values.join(",").split(",").map((s) => s.trim().replace(/^[+\-^]/, "")).filter(Boolean);
      const ciphers = pick("ciphers");
      const macs = pick("macs");
      const kex = pick("kexalgorithms");
      const knownAll = ssh.get("ciphers").known && ssh.get("macs").known && ssh.get("kexalgorithms").known;
      const weak = [
        ...ciphers.filter((x) => /cbc|arcfour|3des|blowfish|cast128|rc4/.test(x)),
        ...macs.filter((x) => /md5|sha1|umac-64|ripemd/.test(x)),
        ...kex.filter((x) => /sha1/.test(x)),
      ];
      if (weak.length)
        c.fail("medium", `The server offers ${list(weak, 5)}.`, "A client that can be steered to an old algorithm negotiates a weaker integrity check or key exchange than it would otherwise use.", "Restrict them in a drop-in, for example `MACs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com,umac-128-etm@openssh.com`, then `sshd -t` and `systemctl reload ssh`. Settings from the first file that sets one win, so check `sshd -T`.");
      else if (ssh.mode === "effective" || (knownAll && (ciphers.length || macs.length || kex.length))) c.pass("No legacy algorithm is offered.");
      else c.unknown(`${denied || "The algorithms are not set in the files that could be read, so the OpenSSH defaults apply, and those depend on its version. "}`.trim(), `${ROOT_FIX}\`sshd -T | grep -iE '^(ciphers|macs|kexalgorithms) '\` and look for sha1, umac-64 and cbc.`);
    }
  }

  // ---- 2. Network ------------------------------------------------------------------------------------------------------------
  const exposed = ports.items.filter((p) => !isLoopbackAddress(p.address));
  const reachable = (p: ListeningPort) => fw.reach(p.port, p.proto);
  const fwKnown = fw.active === true && exposed.every((p) => reachable(p) !== "unknown");
  const behindFirewall = (p: ListeningPort) => reachable(p) === "closed";
  const who = (p: ListeningPort) => `${p.process ?? "a service"} on ${p.address === "0.0.0.0" || p.address === "::" || p.address === "*" ? "every interface" : p.address}, port ${p.port}/${p.proto}`;
  const noPorts = ports.source === "none";
  const portsHow = `${ROOT_FIX}\`ss -lntup\`.`;
  const reachNote = fwKnown ? "" : " Whether the firewall lets it through could not be read.";

  {
    const c = b.add("network", "net.docker-api", "Docker API on the network", "listening sockets, /etc/docker/daemon.json");
    const dockerPorts = exposed.filter((p) => p.proto === "tcp" && p.port === 2375 && !behindFirewall(p));
    const daemon = ((): { hosts: string[]; tls: boolean; iptables: boolean | null } => {
      try {
        const j = JSON.parse((sec.get("DOCKER") ?? []).join("\n").split("@daemon")[1] ?? "{}") as { hosts?: string[]; tlsverify?: boolean; iptables?: boolean };
        return { hosts: j.hosts ?? [], tls: j.tlsverify === true, iptables: j.iptables ?? null };
      } catch {
        return { hosts: [], tls: false, iptables: null };
      }
    })();
    const tcpHost = daemon.hosts.filter((h) => h.startsWith("tcp://") && !daemon.tls);
    if (noPorts) c.unknown("The listening sockets could not be listed.", portsHow);
    else if (dockerPorts.length || tcpHost.length)
      c.fail("critical", `The Docker API answers over plain TCP (${list([...dockerPorts.map(who), ...tcpHost], 3)}) with no TLS client check.${reachNote}`, "Anyone who can reach port 2375 can start a container that mounts the host's disk: that is root on the machine, with no password.", "Remove the `tcp://` entry from `hosts` in /etc/docker/daemon.json (and any `-H tcp://` in the unit), or use `tlsverify` with client certificates, then restart Docker in a quiet moment. Use the unix socket, or an SSH tunnel to it.");
    else c.pass("No Docker API is listening on the network.");
  }
  {
    const c = b.add("network", "net.databases", "Databases and caches on the network", "listening sockets");
    const hits = exposed.filter((p) => DB_PORTS[p.port] && !behindFirewall(p));
    if (noPorts) c.unknown("The listening sockets could not be listed.", portsHow);
    else if (hits.length)
      c.fail("high", `${list(hits.map((p) => `${DB_PORTS[p.port]} (${who(p)})`), 4)}.${reachNote}`, "A database or cache open to the network is reached by password guessing, by default or empty credentials, and by every exploit for that product; Redis and Memcached often have no login at all.", "Bind it to 127.0.0.1 or a private interface (`bind` in redis.conf, `listen_addresses` in postgresql.conf, `bind-address` in my.cnf) and restart it in a quiet moment, and keep the port closed in the firewall. Reach it through an SSH tunnel or a VPN.");
    else c.pass("No database or cache port is open beyond this machine.");
  }
  {
    const c = b.add("network", "net.other-listeners", "Other services on the network", "listening sockets");
    const expected = (p: ListeningPort) => (p.proto === "tcp" && (p.port === 80 || p.port === 443 || sshPorts.includes(p.port))) || (p.proto === "udp" && [68, 123, 443].includes(p.port));
    const rest = exposed.filter((p) => !expected(p) && !DB_PORTS[p.port] && p.port !== 2375 && !behindFirewall(p));
    const debug = rest.filter((p) => DEBUG_PORTS[p.port]);
    const dev = rest.filter((p) => DEV_PORTS.has(p.port));
    const others = rest.filter((p) => !DEBUG_PORTS[p.port] && !DEV_PORTS.has(p.port));
    if (noPorts) c.unknown("The listening sockets could not be listed.", portsHow);
    else if (rest.length) {
      const parts = [
        ...debug.map((p) => `${DEBUG_PORTS[p.port]} (${who(p)})`),
        ...dev.map((p) => `a dev server (${who(p)})`),
        ...others.map((p) => (p.port === 5353 && p.proto === "udp" ? `mDNS (${who(p)})` : who(p))),
      ];
      c.fail(
        debug.length ? "critical" : dev.length ? "medium" : "low",
        `${list(parts, 5)}.${reachNote}`,
        debug.length
          ? "A debugger port runs any code a client sends it, with no login."
          : dev.length
            ? "A dev server has no login and serves the project's source and its reload socket to anyone who can reach it."
            : "Each service open to the network is more to patch and more to attack, and these are not the usual web or SSH ports.",
        "Bind it to 127.0.0.1 (a dev server: leave out `--host`) and reach it through an SSH tunnel, or stop it if nothing needs it, and make sure the firewall has no allow rule for it.",
      );
    } else c.pass(`Beyond SSH${sshPorts.length ? ` (${sshPorts.join(", ")})` : ""} and the web ports, nothing listens on the network.`);
  }
  {
    const c = b.add("network", "net.firewall-active", "Host firewall", "ufw, firewalld, nftables, iptables");
    if (fw.active === true) c.pass(`A firewall is on (${fw.evidence}).`);
    else if (fw.active === false) c.fail("high", `No firewall is filtering: ${fw.evidence}.`, "Every service that listens is reachable from the network, including the ones started by accident.", "Allow SSH first, then switch it on: `ufw allow <ssh port>/tcp && ufw allow 80,443/tcp && ufw enable` (ufw), or `firewall-cmd --set-default-zone=public` (firewalld). Keep the current session open while you test a second login.");
    else c.unknown(fw.evidence, `${ROOT_FIX}\`ufw status verbose\`, \`nft list ruleset\` or \`iptables -S\`.`);
  }
  {
    const c = b.add("network", "net.default-drop", "Incoming traffic is denied unless allowed", "firewall default input policy");
    if (fw.active === false) c.fail("high", "There is no firewall, so nothing is denied by default.", "Anything listening is reachable.", "See the firewall check above.");
    else if (fw.inputPolicy === "drop") c.pass(`The default for incoming traffic is deny (${fw.evidence}).`);
    else if (fw.inputPolicy === "accept") c.fail("high", `The default for incoming traffic is allow (${fw.evidence}).`, "A new service is reachable the moment it starts, unless someone remembers to block it.", "Set the default to deny and allow what is needed: `ufw default deny incoming` after `ufw allow <ssh port>/tcp`.");
    else c.unknown("The default policy is not visible to this account.", `${ROOT_FIX}\`ufw status verbose\`, \`nft list ruleset\` or \`iptables -S\`.`);
  }

  // ---- 3. Privileges -----------------------------------------------------------------------------------------------------------
  const accounts = parsePasswd(sec.get("PASSWD") ?? []);
  const uidMin = int(/UID_MIN\s+(\d+)/.exec((sec.get("UID_MIN") ?? []).join("\n"))?.[1]) ?? 1000;
  const shadow = sec.get("SHADOW") ?? [];
  const shadowKv = keyValues(shadow);
  {
    const c = b.add("privilege", "priv.uid0", "Accounts with root's user id", "/etc/passwd");
    const extra = accounts.filter((a) => a.uid === 0 && a.name !== "root").map((a) => a.name);
    if (accounts.length === 0) c.unknown("/etc/passwd could not be read.", `${ROOT_FIX}\`awk -F: '$3==0' /etc/passwd\`.`);
    else if (extra.length) c.fail("critical", `${list(extra)} ${extra.length === 1 ? "has" : "have"} user id 0.`, "An account with user id 0 is root under another name, and is easy to overlook.", "Find out why, then give each its own id or remove it: `usermod -u <new id> <name>` or `userdel <name>`.");
    else c.pass("Only root has user id 0.");
  }
  {
    const c = b.add("privilege", "priv.empty-passwords", "Accounts with no password", "/etc/passwd, /etc/shadow");
    const inPasswd = accounts.filter((a) => a.passwordField === "").map((a) => a.name);
    const inShadow = shadow.filter((l) => l.startsWith("empty ")).map((l) => l.slice(6).trim());
    const all = [...new Set([...inPasswd, ...inShadow])];
    if (all.length) c.fail("critical", `${list(all)} ${all.length === 1 ? "has" : "have"} an empty password field.`, "Anyone can log in to that account, locally or over SSH if it allows passwords, with just its name.", "Lock it or set a password: `passwd -l <name>`.");
    else if (shadowKv.get("readable") === "yes") c.pass("No account has an empty password.");
    else c.unknown("Only /etc/passwd could be read (it holds no passwords); an empty password would be in /etc/shadow, which needs root.", `${ROOT_FIX}\`awk -F: '$2==""{print $1}' /etc/shadow\`.`);
  }
  {
    const c = b.add("privilege", "priv.shadow-file", "Who can read the password hashes", "/etc/shadow");
    const perm = /^(\d+)\s+(\S+)\s+(\S+)/.exec(shadowKv.get("perm") ?? "");
    if (!perm) c.unknown("The permissions of /etc/shadow could not be read.", `${ROOT_FIX}\`stat -c '%a %U:%G' /etc/shadow\`.`);
    else {
      const mode = perm[1].slice(-3).padStart(3, "0");
      if (/[1-7]$/.test(mode) || perm[2] !== "root")
        c.fail("high", `/etc/shadow is ${mode} owned by ${perm[2]}:${perm[3]}.`, "Anyone who can read the hashes can crack them offline, at their leisure.", "`chown root:shadow /etc/shadow && chmod 640 /etc/shadow`.");
      else c.pass(`/etc/shadow is ${mode} ${perm[2]}:${perm[3]}.`);
    }
  }
  {
    const c = b.add("privilege", "priv.sudoers", "Passwordless sudo", "/etc/sudoers, /etc/sudoers.d");
    const sud = sec.get("SUDOERS") ?? [];
    if (!sud.includes("readable=yes")) {
      const names = sud.filter((l) => l.startsWith("file ")).map((l) => l.slice(5));
      c.unknown(`sudoers needs root to read${names.length ? `; /etc/sudoers.d holds ${list(names)}` : ""}.`, `${ROOT_FIX}\`cat /etc/sudoers /etc/sudoers.d/*\`, and look for NOPASSWD.`);
    } else {
      const { rules, noAuthenticate } = parseSudoers(sud);
      const everything = rules.filter((r) => r.who !== "root" && r.commands.some((x) => x === "ALL"));
      const wildcard = rules.filter((r) => r.commands.some((x) => x !== "ALL" && x.includes("*")));
      const escapable = rules.flatMap((r) => r.commands.filter((x) => x !== "ALL").map((x) => ({ who: r.who, tokens: x.split(/\s+/) })).filter((x) => x.tokens.length === 1 && ESCAPABLE.has(basename(x.tokens[0]))).map((x) => `${x.who}: ${basename(x.tokens[0])}`));
      if (everything.length || noAuthenticate)
        c.fail("high", `${noAuthenticate ? "sudo never asks for a password (Defaults !authenticate). " : ""}${everything.length ? `${list(everything.map((r) => r.who))} may run anything as root without a password (NOPASSWD: ALL).` : ""}`.trim(), "A stolen session, a hijacked agent or a malicious script run by that account becomes root with nothing to stop it.", "Remove NOPASSWD from the rule with `visudo -f /etc/sudoers.d/<file>`, or list only the commands that must run unattended.");
      else if (wildcard.length || escapable.length)
        c.fail("medium", `${wildcard.length ? `Passwordless rules with a wildcard (${list(wildcard.map((r) => r.who))}). ` : ""}${escapable.length ? `Passwordless commands that can start a root shell: ${list(escapable)}.` : ""}`.trim(), "A wildcard or an unrestricted editor, pager or package manager lets the account run other commands or read and write any file as root.", "Give the exact command and arguments in the rule, without wildcards, for example `/usr/bin/apt-get update`, using `visudo -f`. Check with `sudo -l -U <name>`.");
      else c.pass(rules.length ? `${rules.length} passwordless rule${rules.length === 1 ? "" : "s"}, none for ALL, with a wildcard, or for a command that gives a shell.` : "No rule runs anything without a password.");
    }
  }
  {
    const c = b.add("privilege", "priv.service-shells", "Service accounts with a login shell", "/etc/passwd");
    const odd = accounts.filter((a) => a.uid !== 0 && (a.uid < uidMin || a.uid >= 60000) && INTERACTIVE_SHELL.test(a.shell) && !SPECIAL_SHELL.has(a.shell));
    if (accounts.length === 0) c.unknown("/etc/passwd could not be read.", `${ROOT_FIX}\`awk -F: '$3<1000 && $7 !~ /(nologin|false)$/' /etc/passwd\`.`);
    else if (odd.length)
      c.fail("low", `${list(odd.map((a) => `${a.name} (${a.shell})`))} ${odd.length === 1 ? "is a system account" : "are system accounts"} with an interactive shell.`, "A service account that can open a shell is a way in if it ever gets a key, a password, or a program that runs commands for it.", "Check how it is used first (a service run by systemd needs no shell), then `usermod -s /usr/sbin/nologin <name>`. A job started with `su` or `sudo -iu` for that account will stop working.");
    else c.pass("No system account has a login shell.");
  }
  {
    const c = b.add("privilege", "priv.suid", "Set-uid and set-gid programs", "/usr, /bin, /sbin, /opt");
    const sl = sec.get("SUID") ?? [];
    const found = sl.filter((l) => l.startsWith("/"));
    const cut = (sl.find((l) => l.startsWith("@rc "))?.slice(4).trim() ?? "") === "124";
    const danger = found.filter((f) => DANGEROUS_SUID.has(basename(f)));
    const unfamiliar = found.filter((f) => !STANDARD_SUID.has(basename(f)) && !DANGEROUS_SUID.has(basename(f)));
    if (!sec.has("SUID")) c.unknown("The search did not run.", `${ROOT_FIX}\`find / -xdev -type f \\( -perm -4000 -o -perm -2000 \\)\`.`);
    else if (danger.length)
      c.fail("critical", `${list(danger)} ${danger.length === 1 ? "is" : "are"} set-uid or set-gid.`, "These programs can start a shell or read and write any file, so with the set-uid bit any local user gets what the owner has: usually root.", "Clear the bit unless it is deliberate: `chmod u-s,g-s <path>`, and find out how it got there (a package would not have set it).");
    else if (unfamiliar.length)
      c.fail("low", `${list(unfamiliar)} ${unfamiliar.length === 1 ? "is" : "are"} set-uid or set-gid and not on the usual list${cut ? "; the search was cut short" : ""}.`, "Each set-uid program is code that runs with more rights than the person running it, so a flaw in it is a way to root.", "Check each: `dpkg -S <path>` or `rpm -qf <path>` says which package owns it. Clear the bit on what nothing needs: `chmod u-s <path>`.");
    else c.pass(`${found.length} set-uid or set-gid programs, all the usual ones${cut ? " (the search was cut short, and covered /usr, /bin, /sbin, /opt)" : " in /usr, /bin, /sbin and /opt"}.`);
  }
  {
    const c = b.add("privilege", "priv.path-dirs", "Who can write where commands are run from", "directories on the command path");
    const dirs = new Map<string, { mode: string; shown: string; owner: string; group: string; planted: string[] }>();
    let last: string | null = null;
    for (const l of sec.get("PATHDIRS") ?? []) {
      const ww = /^ww (.+)$/.exec(l);
      if (ww && last) {
        dirs.get(last)?.planted.push(basename(ww[1]));
        continue;
      }
      const m = /^(\d+)\s+(\S+)\s+(\S+)\s+(.+)$/.exec(l);
      if (m) {
        last = m[4];
        // The special bits (setgid, sticky) come before the three that matter here.
        if (!dirs.has(last)) dirs.set(last, { mode: m[1].slice(-3).padStart(3, "0"), shown: m[1], owner: m[2], group: m[3], planted: [] });
      }
    }
    // The connected account's own directories (~/.local/bin) are its own business; root has none that don't matter.
    const homeUser = identity.user && !identity.root ? identity.user : null;
    const worldWritable = [...dirs].filter(([, d]) => /[2367]$/.test(d.mode) || d.planted.length);
    const groupWritable = [...dirs].filter(([, d]) => /^.[2367]/.test(d.mode) && !/[2367]$/.test(d.mode) && d.group !== "root" && d.owner !== homeUser && d.group !== homeUser);
    if (dirs.size === 0) c.unknown("The command directories could not be read.", `${ROOT_FIX}\`ls -ld /usr/local/bin /usr/bin /bin $(echo $PATH | tr : ' ')\`.`);
    else if (worldWritable.length)
      c.fail("high", `${list(worldWritable.map(([p, d]) => `${p} (${d.shown}${d.planted.length ? `, files anyone can change: ${list(d.planted, 3)}` : ""})`))}.`, "Anyone on the machine can replace a program that others, including root, run by name.", "`chmod o-w <dir>` for a directory, `chmod o-w <file>` for a file, and check what was changed: `rpm -V <package>` or `debsums -c`.");
    else if (groupWritable.length)
      c.fail("low", `${list(groupWritable.map(([p, d]) => `${p} (${d.shown} ${d.owner}:${d.group})`))} can be written by the ${list([...new Set(groupWritable.map(([, d]) => d.group))])} group and is on the command path.`, "Anyone in that group can put a program there that others run by name, so one member's account is a way to the others' (and to root's, if root has the directory on its path).", "If the directory has to stay shared, keep it last on the path (so it can't shadow a system command) and write to it only as root: `chmod g-w <dir>`, or give one deploy account write access with an ACL.");
    else c.pass(`${dirs.size} command directories, none writable by anyone it should not be.`);
  }

  // ---- 4. Kernel ---------------------------------------------------------------------------------------------------------------
  const sysctl = keyValues(sec.get("SYSCTL") ?? []);
  const sysctlHow = (k: string) => `${ROOT_FIX}\`sysctl ${k}\`.`;
  const readKey = (k: string): number | null => {
    const v = sysctl.get(k);
    return v === undefined || v === "?" ? null : int(v);
  };
  const sysctlFile = "a file in /etc/sysctl.d/ that sorts last, such as /etc/sysctl.d/99-zz-hardening.conf";
  const kernelCheck = (id: string, title: string, key: string, want: (v: number) => boolean, severity: Severity, expected: string, risk: string, fix: string) => {
    const c = b.add("kernel", id, title, `sysctl ${key}`);
    const v = readKey(key);
    if (v === null) c.unknown(`${key} could not be read.`, sysctlHow(key));
    else if (want(v)) c.pass(`${key} = ${v}.`);
    else c.fail(severity, `${key} is ${v}; ${expected}.`, risk, `Put \`${fix}\` in ${sysctlFile}, then \`sysctl --system\`. Try \`sysctl -w ${fix.replace(/\s/g, "")}\` first: it lasts until reboot and takes no restart.`);
  };
  {
    const c = b.add("kernel", "kernel.ip-forward", "Packet forwarding", "sysctl net.ipv4.ip_forward");
    const v = readKey("net.ipv4.ip_forward");
    const ifaces = (sec.get("NETROLE") ?? []).filter((l) => l.startsWith("iface ")).map((l) => l.slice(6).trim());
    const tunnels = ifaces.filter((n) => /^(wg|tun|tap|ppp|tailscale|zt|vti|ipsec|gre|docker|br-|virbr|veth|lxd|incus|cni|flannel|cali)/.test(n));
    const runtime = svcOn("docker", "containerd", "libvirtd", "lxd", "incus", "k3s", "kubelet", "podman") === true;
    const vpn = services ? [...services].some((s) => /^(wg-quick@|openvpn|strongswan|tailscaled|zerotier|ipsec|xl2tpd|pptpd|wireguard)/.test(s)) : false;
    if (v === null) c.unknown("net.ipv4.ip_forward could not be read.", sysctlHow("net.ipv4.ip_forward"));
    else if (v === 0) c.pass("net.ipv4.ip_forward = 0.");
    else if (tunnels.length || runtime || vpn) c.pass(`net.ipv4.ip_forward = 1, which a gateway, VPN or container host needs (${list([...tunnels, ...(runtime ? ["a container runtime"] : []), ...(vpn ? ["a VPN service"] : [])], 3)}).`);
    else {
      const open = fw.forwardPolicy === "accept";
      c.fail(open ? "medium" : "low", `net.ipv4.ip_forward is 1, and this host has no tunnel, VPN or container runtime${open ? "; the firewall's forward policy is accept too" : fw.forwardPolicy === "drop" ? "; the firewall's forward policy is deny" : ""}.`, "A host that forwards packets is a router: traffic from one network can be sent through it to another, and with an open forward policy nothing stops that.", `Set \`net.ipv4.ip_forward=0\` in ${sysctlFile}${open ? ", and the firewall's forward policy to deny (`DEFAULT_FORWARD_POLICY=\"DROP\"` in /etc/default/ufw, then `ufw reload`)" : ""}. If a VPN gateway is planned, keep forwarding but add explicit forward rules. Try \`sysctl -w net.ipv4.ip_forward=0\` first.`);
    }
  }
  kernelCheck("kernel.source-route", "Source-routed packets", "net.ipv4.conf.all.accept_source_route", (v) => v === 0, "medium", "it should be 0", "A sender can choose the path its packets take, which can be used to get round address-based filters.", "net.ipv4.conf.all.accept_source_route=0");
  kernelCheck("kernel.redirects", "ICMP redirects", "net.ipv4.conf.all.accept_redirects", (v) => v === 0, "medium", "it should be 0", "Anyone on the same network can tell this host to send its traffic through them.", "net.ipv4.conf.all.accept_redirects=0");
  kernelCheck("kernel.syncookies", "SYN-flood protection", "net.ipv4.tcp_syncookies", (v) => v === 1, "medium", "it should be 1", "A flood of half-open connections can fill the connection table and keep real clients out.", "net.ipv4.tcp_syncookies=1");
  kernelCheck("kernel.rp-filter", "Reverse path filtering", "net.ipv4.conf.all.rp_filter", (v) => v === 1, "low", "1 (strict) is expected; 2 is the loose mode many distributions ship", "Packets with a forged source address are accepted if any route back exists.", "net.ipv4.conf.all.rp_filter=1");
  kernelCheck("kernel.protected-hardlinks", "Hard-link protection", "fs.protected_hardlinks", (v) => v === 1, "medium", "it should be 1", "Without it, a user can hard-link a file they do not own in a shared directory and trick a privileged program into changing it.", "fs.protected_hardlinks=1");
  kernelCheck("kernel.protected-symlinks", "Symlink protection", "fs.protected_symlinks", (v) => v === 1, "medium", "it should be 1", "Without it, a user can plant a symlink in /tmp and trick a privileged program into following it.", "fs.protected_symlinks=1");
  kernelCheck("kernel.dmesg", "Kernel log for everyone", "kernel.dmesg_restrict", (v) => v === 1, "low", "it should be 1", "Any local user can read the kernel log, which holds addresses and hardware details that help an exploit.", "kernel.dmesg_restrict=1");
  kernelCheck("kernel.kptr", "Kernel addresses", "kernel.kptr_restrict", (v) => v >= 2, "low", "it should be 2", "Kernel pointers are visible to privileged tools, which tells an exploit where to aim.", "kernel.kptr_restrict=2");

  // ---- 5. Containers -----------------------------------------------------------------------------------------------------------
  const dockerLines = sec.get("DOCKER") ?? [];
  if (dockerLines.includes("present=yes")) {
    const dockerKv = keyValues(dockerLines);
    const rootless = /name=rootless/.test(dockerLines.find((l) => l.startsWith("secopts ")) ?? "");
    const daemonJson = ((): { iptables: boolean | null } => {
      try {
        const j = JSON.parse(dockerLines.join("\n").split("@daemon")[1] ?? "{}") as { iptables?: boolean };
        return { iptables: j.iptables ?? null };
      } catch {
        return { iptables: null };
      }
    })();
    const containers = dockerLines
      .filter((l) => l.startsWith("c|"))
      .map((l) => {
        const f = l.split("|");
        let bindings: Record<string, { HostIp?: string; HostPort?: string }[] | null> = {};
        try {
          bindings = JSON.parse(f[5] || "{}") as typeof bindings;
        } catch {
          /* ignore */
        }
        return { name: (f[1] ?? "").replace(/^\//, ""), image: f[2] ?? "", user: (f[3] ?? "").trim(), privileged: f[4] === "true", bindings, mounts: (f[6] ?? "").split(",").filter(Boolean) };
      });
    const listed = dockerKv.get("containers") === "ok";
    const needList = `${ROOT_FIX}\`docker ps\` and \`docker inspect\` (this account is not in the docker group).`;

    {
      const c = b.add("containers", "docker.socket", "Docker socket permissions", "/var/run/docker.sock");
      const sock = /^sock (\d+) (\S+) (\S+) (.+)$/.exec(dockerLines.find((l) => l.startsWith("sock ")) ?? "");
      if (!sock) c.pass(rootless ? "Docker runs rootless, with its socket in the user's own runtime directory." : "No Docker socket was found at /var/run/docker.sock.");
      else {
        const mode = sock[1].slice(-3).padStart(3, "0");
        if (/[2367]$/.test(mode)) c.fail("high", `${sock[4]} is ${mode}: every user can talk to Docker.`, "Whoever can use the Docker socket can start a container that mounts the host's disk: that is root.", `\`chmod 660 ${sock[4]}\` and keep it owned by root:docker (restart Docker if it recreates it), and make only trusted users members of docker.`);
        else c.pass(`${sock[4]} is ${mode} ${sock[2]}:${sock[3]}.`);
      }
    }
    {
      const c = b.add("containers", "docker.published-ports", "Published container ports and the firewall", "docker ps, /etc/docker/daemon.json");
      if (!listed) c.unknown("The containers could not be listed.", needList);
      else {
        const wide = containers.flatMap((k) =>
          Object.entries(k.bindings).flatMap(([port, binds]) => (binds ?? []).filter((x) => !x.HostIp || x.HostIp === "0.0.0.0" || x.HostIp === "::").map((x) => ({ name: k.name, port: x.HostPort ?? "?", inner: port.split("/")[0] }))),
        );
        const dbWide = wide.filter((w) => DB_PORTS[Number(w.inner)]);
        if (rootless) c.pass(wide.length ? `Docker is rootless, so published ports do not bypass the host firewall (${list(wide.map((w) => `${w.name}:${w.port}`), 3)}).` : "Docker is rootless, and nothing is published on every interface.");
        else if (daemonJson.iptables === false) c.pass("Docker's iptables handling is off, so the host firewall applies to published ports.");
        else if (wide.length)
          c.fail(dbWide.length ? "high" : "medium", `${list(wide.map((w) => `${w.name} publishes ${w.port}${DB_PORTS[Number(w.inner)] ? ` (${DB_PORTS[Number(w.inner)]})` : ""}`), 4)} on every interface.`, "Docker writes its own iptables rules ahead of ufw and firewalld, so a published port is open to the network even where the firewall says it is closed.", "Publish on the loopback address (`-p 127.0.0.1:8080:80`, or `127.0.0.1:8080:80` in compose) and put a reverse proxy in front, or add rules to the DOCKER-USER chain. Recreate the container in a quiet moment.");
        else c.pass("No container publishes a port on every interface.");
      }
    }
    {
      const c = b.add("containers", "docker.privileged", "Privileged containers", "docker inspect");
      if (!listed) c.unknown("The containers could not be listed.", needList);
      else {
        const bad = containers.filter((k) => k.privileged || k.mounts.some((m) => /\/docker\.sock$/.test(m)));
        if (bad.length)
          c.fail("high", `${list(bad.map((k) => `${k.name} (${k.privileged ? "privileged" : "has the Docker socket mounted"})`))}.`, "A privileged container, or one with the Docker socket, can reach the host's devices and start other containers: from inside it, the host is open.", "Drop `--privileged` and grant only the capability needed (`--cap-add`), and do not mount /var/run/docker.sock; recreate the container in a quiet moment.");
        else c.pass(containers.length ? `${containers.length} container${containers.length === 1 ? " is" : "s are"} running, none privileged or holding the Docker socket.` : "No container is running.");
      }
    }
    {
      const c = b.add("containers", "docker.root-user", "Containers running as root", "docker inspect");
      if (!listed) c.unknown("The containers could not be listed.", needList);
      else {
        const asRoot = containers.filter((k) => k.user === "" || k.user === "0" || k.user === "root" || k.user.startsWith("0:") || k.user.startsWith("root:"));
        if (rootless) c.pass("Docker is rootless: root inside a container is an ordinary user on the host.");
        else if (asRoot.length)
          c.fail("low", `${list(asRoot.map((k) => k.name))} ${asRoot.length === 1 ? "starts" : "start"} as root (no user set). Many images drop to their own user after they start; this could not be seen.`, "A flaw in a program running as root in a container gives the attacker root in it, and an escape from the container then starts from root.", "Set `user:` in compose (or `--user 1000:1000`) where the image allows it, and check `docker top <name>` for who the processes run as.");
        else c.pass("Every container runs as a named user.");
      }
    }
  }

  // ---- 6. Patching, logging, intrusion detection --------------------------------------------------------------------------
  const patch = sec.get("PATCH") ?? [];
  const patchKv = keyValues(patch);
  const family = patchKv.get("family");
  {
    const c = b.add("patching", "patch.automatic", "Automatic security updates", family === "rpm" ? "dnf-automatic / yum-cron" : "/etc/apt/apt.conf.d/20auto-upgrades");
    if (family === "apt") {
      const periodic = (k: string) => /^APT::Periodic::\S+\s+"(\d+)"/.exec(patch.find((l) => l.startsWith(`APT::Periodic::${k} `)) ?? "")?.[1];
      const lists = periodic("Update-Package-Lists");
      const unattended = periodic("Unattended-Upgrade");
      const installed = (patchKv.get("unattended") ?? "").includes("installed");
      if (installed && lists && lists !== "0" && unattended && unattended !== "0") c.pass("unattended-upgrades is installed and runs every day (apt-config: Update-Package-Lists and Unattended-Upgrade are on).");
      else
        c.fail("medium", `${installed ? "unattended-upgrades is installed, but" : "unattended-upgrades is not installed, or"} Update-Package-Lists is ${lists ?? "unset"} and Unattended-Upgrade is ${unattended ?? "unset"} (apt-config dump).`, "Security fixes wait for someone to remember to run them, and a published flaw stays open until then.", "Set both to \"1\" in /etc/apt/apt.conf.d/20auto-upgrades (`dpkg-reconfigure -plow unattended-upgrades` does it), and check with `apt-config dump | grep Periodic`. If patching is done by hand on a schedule instead, keep to it.");
    } else if (family === "rpm") {
      const on = patch.filter((l) => /^enabled \S+ enabled/.test(l));
      if (on.length) c.pass(`${on[0].split(" ")[1]} is enabled.`);
      else if (patch.some((l) => l.startsWith("enabled "))) c.fail("medium", "dnf-automatic and yum-cron are not enabled.", "Security fixes wait for someone to run them.", "`dnf install dnf-automatic`, set `apply_updates = yes` in /etc/dnf/automatic.conf, and `systemctl enable --now dnf-automatic-install.timer`.");
      else c.unknown("Could not tell whether automatic updates are on.", `${ROOT_FIX}\`systemctl list-unit-files | grep -E 'dnf-automatic|yum-cron'\`.`);
    } else c.unknown("This host does not use apt or dnf, so its update setup was not checked.", "Check how this distribution applies security updates.");
  }
  {
    const c = b.add("patching", "patch.pending", "Waiting security updates", "apt list --upgradable");
    if (family === "apt") {
      const pending = int(patchKv.get("upgradable")) ?? 0;
      const security = int(patchKv.get("security")) ?? 0;
      const stamp = int(patchKv.get("stamp_mtime")) ?? int(patchKv.get("lists_mtime"));
      const age = stamp !== null && nowSecs !== null ? Math.max(0, Math.floor((nowSecs - stamp) / 86400)) : null;
      const stale = age !== null && age > 7;
      const reboot = patchKv.get("reboot") === "yes" ? " A restart is waiting too." : "";
      if (security > 0)
        c.fail(stale || security >= 10 ? "high" : "medium", `${security} security update${security === 1 ? " is" : "s are"} waiting (${pending} updates in all)${age !== null ? `; the package lists are ${age} day${age === 1 ? "" : "s"} old${stale ? ", so there may be more" : ""}` : ""}.${reboot}`, "Each is a published flaw in software on this host; the ones in network-facing libraries and daemons are what attackers scan for.", "Preview with `apt-get update && apt-get -s upgrade`, then upgrade in a quiet moment: `apt-get upgrade`. Restart the services it names (`needrestart`), and the machine if a kernel was installed.");
      else if (stale) c.fail("medium", `Nothing security-related is waiting in the package lists, but they are ${age} days old, so that says little.${reboot}`, "A host that has not looked for updates in over a week does not know about new flaws.", "`apt-get update`, then check `apt list --upgradable`.");
      else c.pass(`No security update is waiting${age !== null ? ` (package lists ${age} day${age === 1 ? "" : "s"} old)` : ""}.${reboot}`);
    } else c.unknown("Pending updates are only counted on apt-based systems; on others it needs the network.", `${ROOT_FIX}\`dnf updateinfo list security\` (or your distribution's equivalent).`);
  }
  {
    const c = b.add("patching", "log.collector", "A log collector is running", "systemd-journald, rsyslog, syslog-ng");
    const on = svcOn("systemd-journald", "rsyslog", "syslog-ng");
    if (on === null) c.unknown("This host has no systemd, so the services could not be listed.", "Check that syslog or journald is running: `pgrep -l 'rsyslogd|syslog-ng|journald'`.");
    else if (on) {
      const running = ["systemd-journald", "rsyslog", "syslog-ng"].filter((s) => services?.has(`${s}.service`));
      c.pass(`${and(running)} ${running.length === 1 ? "is" : "are"} running.`);
    }
    else c.fail("medium", "Neither systemd-journald, rsyslog nor syslog-ng is running.", "Nothing records logins, errors or attacks, so there is nothing to look at afterwards.", "`systemctl enable --now systemd-journald` (or rsyslog).");
  }
  {
    const c = b.add("patching", "log.audit", "The audit daemon", "auditd");
    const on = svcOn("auditd");
    if (on === null) c.unknown("This host has no systemd, so the services could not be listed.", "Check `pgrep auditd`.");
    else if (on) c.pass("auditd is running.");
    else c.fail("low", "auditd is not running.", "There is no record of who ran which command, used sudo, or touched which sensitive file.", "`apt install auditd` (or `dnf install audit`), `systemctl enable --now auditd`, and add rules for /etc/passwd, /etc/sudoers and the SSH configuration.");
  }
  const f2bFiles = parseFiles(sec.get("F2B") ?? []);
  const guard = svcOn("fail2ban", "crowdsec");
  {
    const c = b.add("patching", "ids.brute-force", "Brute-force protection", "fail2ban, crowdsec");
    if (guard === null) c.unknown("This host has no systemd, so the services could not be listed.", "Check `pgrep -l 'fail2ban|crowdsec'`.");
    else if (guard) {
      const running = ["fail2ban", "crowdsec"].filter((s) => services?.has(`${s}.service`));
      c.pass(`${and(running)} ${running.length === 1 ? "is" : "are"} running.`);
    }
    else {
      const passwordsOn = ssh.get("passwordauthentication");
      const sev: Severity = !sshdHere ? "info" : passwordsOn.known && (passwordsOn.values[0] ?? "yes").toLowerCase() === "no" ? "low" : "medium";
      c.fail(sev, "Neither fail2ban nor crowdsec is running.", "Repeated failed logins are never turned into a ban, so guessing is only limited by how fast the attacker can try.", "`apt install fail2ban` (or `dnf install fail2ban`), enable the sshd jail with the real SSH port in /etc/fail2ban/jail.d/sshd.local, and `systemctl enable --now fail2ban`.");
    }
  }
  if (sshdHere && svcOn("fail2ban") === true) {
    const c = b.add("patching", "ids.ssh-jail", "fail2ban watches the SSH port", "/etc/fail2ban/jail.d, jail.local");
    // jail.d/*.conf first, then jail.local, then jail.d/*.local: later files override earlier ones.
    const rank = (p: string) => (p.endsWith("jail.local") ? 1 : p.endsWith(".local") ? 2 : 0);
    const ordered = [...f2bFiles].sort((a, z) => rank(a.path) - rank(z.path));
    let enabled: string | null = null;
    let port: string | null = null;
    let seenDenied = false;
    for (const f of ordered) {
      if (f.denied) {
        seenDenied = true;
        continue;
      }
      let section = "";
      for (const l of f.lines) {
        const sm = /^\[(.+)\]$/.exec(l.trim());
        if (sm) {
          section = sm[1];
          continue;
        }
        if (section !== "sshd") continue;
        const kv = /^(\w+)\s*=\s*(.*)$/.exec(l.trim());
        if (kv?.[1] === "enabled") enabled = kv[2].trim().toLowerCase();
        if (kv?.[1] === "port") port = kv[2].trim();
      }
    }
    const wantPorts = sshPorts.map(String);
    const covered = (port ?? "ssh").split(",").map((s) => s.trim()).map((s) => (s === "ssh" ? "22" : s));
    if (seenDenied) c.unknown("Some of fail2ban's configuration could not be read.", `${ROOT_FIX}\`fail2ban-client status sshd\` and \`fail2ban-client get sshd ports\`.`);
    else if (enabled !== "true") c.fail("medium", "fail2ban is running but the sshd jail is not enabled in the files that could be read.", "Failed SSH logins are not turned into bans.", "Add `[sshd]` with `enabled = true` and `port = <ssh port>` to /etc/fail2ban/jail.d/sshd.local, then `systemctl restart fail2ban`.");
    else if (!wantPorts.every((p) => covered.includes(p)))
      c.fail("medium", `The sshd jail watches port ${covered.join(", ")}, but sshd listens on ${wantPorts.join(", ")}.`, "Bans are put on the wrong port, so an attacker who is banned can still reach SSH.", `Set \`port = ${wantPorts.join(",")}\` in the [sshd] section of /etc/fail2ban/jail.d/sshd.local and \`systemctl restart fail2ban\`.`);
    else c.pass(`The sshd jail is on and watches port ${covered.join(", ")}.`);
  }

  const checks = b.checks;
  const bySeverity: Record<Severity, number> = { critical: 0, high: 0, medium: 0, low: 0, info: 0 };
  for (const c of checks) if (c.outcome === "fail" && c.severity) bySeverity[c.severity]++;
  return { supported: true, ...identity, checks, score: scoreOf(checks), bySeverity };
}

// -- the report as text --------------------------------------------------------------------------------------------------------

const SEVERITY_TAG: Record<Severity, string> = { critical: "CRITICAL", high: "HIGH", medium: "MEDIUM", low: "LOW", info: "INFO" };

/** The worst failures first. */
export function bySeverityOrder(checks: Check[]): Check[] {
  return checks.filter((c) => c.outcome === "fail").sort((a, z) => SEVERITIES.indexOf(a.severity ?? "info") - SEVERITIES.indexOf(z.severity ?? "info"));
}

export function scoreText(s: Score): string {
  if (s.percent === null) return "Nothing could be judged.";
  const range = s.unknown ? ` (${s.worst}% to ${s.best}% once the ${s.unknown} unchecked item${s.unknown === 1 ? " is" : "s are"} known)` : "";
  return `${s.percent}%${range}`;
}

/** The audit as plain text for a ticket or a document: scorecard, findings worst first, what was not checked, and an order to fix them in. */
export function auditText(r: AuditReport, observedAt: number, hostLabel: string): string {
  const out: string[] = [`Security audit of ${hostLabel}, observed ${new Date(observedAt).toLocaleString()}`];
  if (r.distro || r.kernel) out.push([r.distro, r.kernel].filter(Boolean).join(", "));
  out.push(`Read as ${r.user ?? "an unknown user"}${r.root ? " (root)" : ", without root: checks that need it are listed as not checked"}. Nothing was changed.`, "");
  out.push("1. Security posture", `   Passed ${r.score.passed}, failed ${r.score.failed}, not checked ${r.score.unknown}`, `   Hardening score: ${scoreText(r.score)}`);
  out.push(`   ${SEVERITIES.filter((s) => r.bySeverity[s]).map((s) => `${r.bySeverity[s]} ${SEVERITY_NAMES[s].toLowerCase()}`).join(", ") || "No findings"}`, "");
  const fails = bySeverityOrder(r.checks);
  out.push("2. Findings");
  if (!fails.length) out.push("   None.");
  for (const c of fails) {
    out.push(`- [${SEVERITY_TAG[c.severity ?? "info"]}] ${c.title}`, `  Component: ${c.component}`, `  Finding: ${c.finding}`, `  Risk: ${c.risk}`, `  Remediation: ${c.remediation}`);
  }
  const unknown = r.checks.filter((c) => c.outcome === "unknown");
  if (unknown.length) {
    out.push("", "3. Not checked (needs access this account does not have)");
    for (const c of unknown) out.push(`- ${c.title} (${c.component}): ${c.finding} ${c.remediation}`);
  }
  out.push("", `${unknown.length ? "4" : "3"}. Suggested order`);
  const tier = (label: string, items: Check[]) => items.length && out.push(`   ${label}: ${items.map((c) => c.title).join("; ")}`);
  tier("First (critical and high)", fails.filter((c) => c.severity === "critical" || c.severity === "high"));
  tier("Next (medium)", fails.filter((c) => c.severity === "medium"));
  tier("Later (low)", fails.filter((c) => c.severity === "low" || c.severity === "info"));
  if (unknown.length) out.push("   Also: have someone with root run the checks listed as not checked.");
  return out.join("\n");
}
