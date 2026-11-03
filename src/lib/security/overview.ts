// The security overview: checks that only read, as the ordinary user, and never ask for a password. Each one says what
// it found and how sure that is; what can't be read without root is said to be unknown, not assumed fine.

export const OVERVIEW_SCRIPT = [
  "echo @@ports; ss -H -tuln 2>/dev/null || netstat -tuln 2>/dev/null | tail -n +3",
  // The config files as written. `sshd -T` shows what the running daemon uses but needs root.
  "echo @@sshd; for f in /etc/ssh/sshd_config /etc/ssh/sshd_config.d/*.conf; do [ -r \"$f\" ] && grep -hiE '^[[:space:]]*(PermitRootLogin|PasswordAuthentication|PermitEmptyPasswords|PubkeyAuthentication|X11Forwarding|MaxAuthTries|Port)[[:space:]]' \"$f\" 2>/dev/null; done",
  "echo @@readable; [ -r /etc/ssh/sshd_config ] && echo yes || echo no",
  "echo @@end",
].join("; ");

export type Exposure = "all" | "local" | "address";

export interface ListeningPort {
  proto: "tcp" | "udp";
  address: string;
  port: number;
  /** Open to every address the host has, only to the host itself, or to one address. */
  exposure: Exposure;
}

function exposureOf(address: string): Exposure {
  if (/^(0\.0\.0\.0|\*|::|\[::\])$/.test(address)) return "all";
  if (/^127\./.test(address) || /^(::1|\[::1\])$/.test(address)) return "local";
  return "address";
}

/** `ss -tuln` or `netstat -tuln` lines. A line that is neither is skipped. */
export function parsePorts(text: string): ListeningPort[] {
  const out: ListeningPort[] = [];
  const seen = new Set<string>();
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    // ss: tcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:*     netstat: tcp 0 0 0.0.0.0:22 0.0.0.0:* LISTEN
    const m = /^(tcp|udp)6?\s+(?:\S+\s+)?\d+\s+\d+\s+(\S+)\s+\S+/.exec(line) ?? /^(tcp|udp)6?\s+\d+\s+\d+\s+(\S+)\s+\S+/.exec(line);
    if (!m) continue;
    const local = m[2].replace(/%[\w.-]+(?=:\d+$)/, "");
    const i = local.lastIndexOf(":");
    if (i < 0) continue;
    const port = Number(local.slice(i + 1));
    if (!Number.isInteger(port) || port < 1 || port > 65535) continue;
    const address = local.slice(0, i);
    const key = `${m[1]}|${address}|${port}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ proto: m[1] as "tcp" | "udp", address, port, exposure: exposureOf(address) });
  }
  return out.sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto));
}

/** The first value given for each option, as OpenSSH uses (the first one wins). Keys are lower-cased. */
export function parseSshdConfig(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) {
    const m = /^\s*(\w+)\s+(.+?)\s*$/.exec(line);
    if (!m) continue;
    const k = m[1].toLowerCase();
    if (!(k in out)) out[k] = m[2].toLowerCase();
  }
  return out;
}

export type Status = "ok" | "warn" | "info" | "unknown";

export interface Finding {
  id: string;
  status: Status;
  title: string;
  detail: string;
}

export interface Overview {
  ports: ListeningPort[];
  sshd: Record<string, string>;
  /** Whether the config file could be read by this user. */
  configReadable: boolean;
}

export function parseOverview(output: string): Overview {
  const parts = new Map<string, string>();
  let name: string | null = null;
  let lines: string[] = [];
  for (const l of output.split(/\r?\n/)) {
    const m = /^@@(\w+)$/.exec(l);
    if (m) {
      if (name) parts.set(name, lines.join("\n"));
      name = m[1];
      lines = [];
    } else lines.push(l);
  }
  if (name) parts.set(name, lines.join("\n"));
  return { ports: parsePorts(parts.get("ports") ?? ""), sshd: parseSshdConfig(parts.get("sshd") ?? ""), configReadable: (parts.get("readable") ?? "").trim() === "yes" };
}

/** What the read-only checks conclude. Only what was read is judged. */
export function findings(o: Overview): Finding[] {
  const out: Finding[] = [];
  if (!o.configReadable) {
    out.push({ id: "sshd-config", status: "unknown", title: "SSH server settings", detail: "This user can't read /etc/ssh/sshd_config, so they can't be checked here. Reading them needs root." });
  } else {
    const root = o.sshd["permitrootlogin"];
    out.push(
      root === "yes"
        ? { id: "root-login", status: "warn", title: "Root can log in over SSH", detail: "PermitRootLogin is yes in the config files. Prefer prohibit-password or no." }
        : root === undefined
          ? { id: "root-login", status: "info", title: "Root login: not set in the config", detail: "OpenSSH's built-in default applies (prohibit-password on current versions: keys only)." }
          : { id: "root-login", status: "ok", title: "Root login is restricted", detail: `PermitRootLogin is ${root}.` },
    );
    const pw = o.sshd["passwordauthentication"];
    out.push(
      pw === "no"
        ? { id: "password-auth", status: "ok", title: "Password logins are off", detail: "Only keys (or other methods) are accepted." }
        : pw === "yes"
          ? { id: "password-auth", status: "warn", title: "Password logins are on", detail: "PasswordAuthentication is yes. Anyone who guesses a password can get in; keys are safer." }
          : { id: "password-auth", status: "info", title: "Password logins: not set in the config", detail: "OpenSSH's built-in default (yes) applies unless another file says otherwise." },
    );
    if (o.sshd["permitemptypasswords"] === "yes") out.push({ id: "empty-passwords", status: "warn", title: "Accounts with empty passwords may log in", detail: "PermitEmptyPasswords is yes." });
    out.push({ id: "sshd-note", status: "info", title: "Read from the config files", detail: "This is what the files say. The running server (sshd -T, needs root) may use something else, for example if it wasn't restarted after an edit." });
  }
  const open = o.ports.filter((p) => p.exposure === "all" && p.proto === "tcp");
  out.push({
    id: "ports",
    status: o.ports.length ? "info" : "unknown",
    title: o.ports.length ? `${open.length} TCP port${open.length === 1 ? "" : "s"} open to every address` : "Listening ports",
    detail: o.ports.length
      ? open.length
        ? `Ports ${[...new Set(open.map((p) => p.port))].join(", ")} accept connections on all of the host's addresses. Whether they are reachable from outside depends on the firewall.`
        : "Everything listening is limited to the host itself or to one address."
      : "Neither ss nor netstat gave a list for this user, so open ports are unknown.",
  });
  return out;
}
