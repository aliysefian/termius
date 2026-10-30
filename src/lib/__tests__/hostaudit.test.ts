// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { auditHost, auditText, bySeverityOrder, parseSshd, parseSudoers, parseUfwStatus, scoreText, type AuditReport, type Severity } from "../hostaudit";

/** The script's output from sections, as the script prints it. */
type Sections = Record<string, string[]>;
const render = (s: Sections) =>
  Object.entries(s)
    .map(([name, lines]) => [`@@${name}`, ...lines].join("\n"))
    .join("\n") + "\n";

const listen = (proto: "tcp" | "udp", addr: string, port: number, proc = "") => `${proto}   ${proto === "tcp" ? "LISTEN" : "UNCONN"} 0      128    ${addr}:${port}   0.0.0.0:*    ${proc ? `users:(("${proc}",pid=7,fd=3))` : ""}`;

const UFW = ["Status: active", "Logging: on (low)", "Default: deny (incoming), allow (outgoing), deny (routed)", "New profiles: skip", "", "To                         Action      From", "--                         ------      ----", "22/tcp                     ALLOW IN    Anywhere", "443/tcp                    ALLOW IN    Anywhere", "22/tcp (v6)                ALLOW IN    Anywhere (v6)"];

/** A hardened host, read by root, so nothing is out of reach. Every check on it passes. */
function baseline(): Sections {
  return {
    HOST: ["os=Linux", "uid=0", "user=root", "now=1800000000", "hostname=web-01", "kernel=Linux 6.8.0", "distro=Ubuntu 24.04 LTS", "family=ubuntu debian", "authkeys=yes", "systemd=yes"],
    SSHD: [
      "mode=effective",
      "port 22",
      "permitrootlogin no",
      "passwordauthentication no",
      "permitemptypasswords no",
      "pubkeyauthentication yes",
      "maxauthtries 3",
      "x11forwarding no",
      "clientaliveinterval 300",
      "clientalivecountmax 2",
      "allowgroups sshusers",
      "ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes128-gcm@openssh.com",
      "macs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com,umac-128-etm@openssh.com",
      "kexalgorithms sntrup761x25519-sha512@openssh.com,curve25519-sha256",
    ],
    PORTS: ["tool ss", "Netid State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process", listen("tcp", "0.0.0.0", 22, "sshd"), listen("tcp", "0.0.0.0", 443, "nginx"), listen("tcp", "127.0.0.1", 5432, "postgres")],
    FIREWALL: ["@ufwconf", "ENABLED=yes", 'DEFAULT_INPUT_POLICY="DROP"', 'DEFAULT_FORWARD_POLICY="DROP"', "@ufw", ...UFW, "@iptables", "@nft"],
    PASSWD: ["root:x:0:0:root:/root:/bin/bash", "daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin", "sync:x:4:65534:sync:/bin:/bin/sync", "www-data:x:33:33:www-data:/var/www:/usr/sbin/nologin", "alice:x:1000:1000:Alice:/home/alice:/bin/bash"],
    SHADOW: ["perm=640 root shadow", "readable=yes"],
    UID_MIN: ["UID_MIN\t\t\t 1000"],
    SUDOERS: ["readable=yes", "root\tALL=(ALL:ALL) ALL", "%sudo\tALL=(ALL:ALL) ALL"],
    SUID: ["/usr/bin/passwd", "/usr/bin/sudo", "/usr/bin/mount", "@rc 0"],
    PATHDIRS: ["755 root root /usr/local/bin", "755 root root /usr/bin", "755 root root /bin"],
    SYSCTL: ["net.ipv4.ip_forward=0", "net.ipv6.conf.all.forwarding=0", "net.ipv4.conf.all.accept_source_route=0", "net.ipv4.conf.all.accept_redirects=0", "net.ipv4.tcp_syncookies=1", "net.ipv4.conf.all.rp_filter=1", "fs.protected_hardlinks=1", "fs.protected_symlinks=1", "kernel.dmesg_restrict=1", "kernel.kptr_restrict=2"],
    NETROLE: ["iface lo", "iface eth0"],
    SERVICES: ["listed=yes", "active ssh.service", "active ufw.service", "active systemd-journald.service", "active rsyslog.service", "active auditd.service", "active fail2ban.service"],
    DOCKER: ["present=no"],
    PATCH: ["family=apt", 'APT::Periodic::Update-Package-Lists "1";', 'APT::Periodic::Unattended-Upgrade "1";', "unattended=install ok installed", "lists_mtime=1799990000", "stamp_mtime=1799990000", "upgradable=3", "security=0"],
    F2B: ["@file /etc/fail2ban/jail.d/sshd.local", "[sshd]", "enabled = true", "port = ssh"],
    END: [],
  };
}

const run = (s: Sections): AuditReport => auditHost(render(s));
const failing = (r: AuditReport) => r.checks.filter((c) => c.outcome === "fail").map((c) => `${c.id}:${c.severity}`).sort();
const outcome = (r: AuditReport, id: string) => r.checks.find((c) => c.id === id)?.outcome;
const edit = (s: Sections, section: string, f: (lines: string[]) => string[]): Sections => ({ ...s, [section]: f(s[section] ?? []) });
/** Replace the lines of a section that match `re` with `to` (or add it when none matches). */
const setLine = (s: Sections, section: string, re: RegExp, to: string): Sections =>
  edit(s, section, (l) => (l.some((x) => re.test(x)) ? l.map((x) => (re.test(x) ? to : x)) : [...l, to]));
const addLines = (s: Sections, section: string, ...more: string[]): Sections => edit(s, section, (l) => [...l, ...more]);
/** Add rules to the end of ufw's table, before the iptables part that follows it. */
const ufwRule = (s: Sections, ...rules: string[]): Sections => edit(s, "FIREWALL", (l) => l.flatMap((x) => (x === "@iptables" ? [...rules, x] : [x])));

describe("a hardened host", () => {
  it("passes every check, which anchors the cases below", () => {
    const r = run(baseline());
    expect(r.supported && r.complete && r.root).toBe(true);
    expect(failing(r)).toEqual([]);
    expect(r.checks.filter((c) => c.outcome === "unknown").map((c) => c.id)).toEqual([]);
    expect(r.score).toMatchObject({ failed: 0, unknown: 0, percent: 100 });
    expect(r.checks.length).toBeGreaterThan(30);
    // The ids are stable and unique: the window keys on them.
    expect(new Set(r.checks.map((c) => c.id)).size).toBe(r.checks.length);
  });
});

/** sshd moved to 6579: its setting, the socket it listens on, and the firewall rule that lets it in. */
const onPort6579 = (s: Sections): Sections =>
  setLine(setLine(setLine(s, "SSHD", /^port /, "port 6579"), "PORTS", /sshd/, listen("tcp", "0.0.0.0", 6579, "sshd")), "FIREWALL", /^22\/tcp {5}/, "6579/tcp                   ALLOW IN    Anywhere");

// Each case changes one thing and says which check must fail, and how badly. Nothing else may fail: that catches a
// rule that reads the wrong line, and one that fires on a host that is fine.
const CASES: { name: string; change: (s: Sections) => Sections; fails: string[] }[] = [
  // ---- SSH
  { name: "root may log in", change: (s) => setLine(s, "SSHD", /^permitrootlogin/, "permitrootlogin yes"), fails: ["ssh.root-login:high"] },
  { name: "root may log in with a key while passwords are on", change: (s) => setLine(setLine(s, "SSHD", /^permitrootlogin/, "permitrootlogin prohibit-password"), "SSHD", /^passwordauthentication/, "passwordauthentication yes"), fails: ["ssh.password-login:high", "ssh.root-login:low"] },
  { name: "root with a key only is fine when passwords are off", change: (s) => setLine(s, "SSHD", /^permitrootlogin/, "permitrootlogin prohibit-password"), fails: [] },
  { name: "passwords may be used", change: (s) => setLine(s, "SSHD", /^passwordauthentication/, "passwordauthentication yes"), fails: ["ssh.password-login:high"] },
  { name: "empty passwords may be used", change: (s) => setLine(s, "SSHD", /^permitemptypasswords/, "permitemptypasswords yes"), fails: ["ssh.empty-passwords:critical"] },
  { name: "keys are switched off", change: (s) => setLine(s, "SSHD", /^pubkeyauthentication/, "pubkeyauthentication no"), fails: ["ssh.pubkey:medium"] },
  { name: "six tries per connection", change: (s) => setLine(s, "SSHD", /^maxauthtries/, "maxauthtries 6"), fails: ["ssh.max-auth-tries:low"] },
  { name: "X11 forwarding", change: (s) => setLine(s, "SSHD", /^x11forwarding/, "x11forwarding yes"), fails: ["ssh.x11:low"] },
  { name: "no idle timeout", change: (s) => setLine(s, "SSHD", /^clientaliveinterval/, "clientaliveinterval 0"), fails: ["ssh.idle-timeout:low"] },
  { name: "no allow list", change: (s) => edit(s, "SSHD", (l) => l.filter((x) => !/^allowgroups/.test(x))), fails: ["ssh.allow-list:low"] },
  { name: "a SHA-1 MAC", change: (s) => setLine(s, "SSHD", /^macs/, "macs hmac-sha2-512-etm@openssh.com,hmac-sha1"), fails: ["ssh.algorithms:medium"] },
  { name: "a 64-bit MAC", change: (s) => setLine(s, "SSHD", /^macs/, "macs umac-64-etm@openssh.com"), fails: ["ssh.algorithms:medium"] },
  { name: "a CBC cipher", change: (s) => setLine(s, "SSHD", /^ciphers/, "ciphers aes128-gcm@openssh.com,aes128-cbc"), fails: ["ssh.algorithms:medium"] },
  { name: "a SHA-1 key exchange", change: (s) => setLine(s, "SSHD", /^kexalgorithms/, "kexalgorithms curve25519-sha256,diffie-hellman-group14-sha1"), fails: ["ssh.algorithms:medium"] },
  // ---- network (the firewall lets 22 and 443 in and nothing else)
  { name: "a Docker API on plain TCP that the firewall lets in", change: (s) => addLines(setLine(s, "FIREWALL", /^443\/tcp/, "443/tcp                    ALLOW IN    Anywhere\n2375/tcp                   ALLOW IN    Anywhere"), "PORTS", listen("tcp", "0.0.0.0", 2375, "dockerd")), fails: ["net.docker-api:critical"] },
  { name: "a Docker API the firewall blocks", change: (s) => addLines(s, "PORTS", listen("tcp", "0.0.0.0", 2375, "dockerd")), fails: [] },
  { name: "Redis on every interface, let in", change: (s) => addLines(edit(s, "FIREWALL", (l) => l.flatMap((x) => (x.startsWith("443/tcp") ? [x, "6379/tcp                   ALLOW IN    Anywhere"] : [x]))), "PORTS", listen("tcp", "0.0.0.0", 6379, "redis-server")), fails: ["net.databases:high"] },
  { name: "Redis on every interface but blocked by the firewall", change: (s) => addLines(s, "PORTS", listen("tcp", "0.0.0.0", 6379, "redis-server")), fails: [] },
  { name: "PostgreSQL on a public address", change: (s) => addLines(edit(s, "FIREWALL", (l) => l.flatMap((x) => (x.startsWith("443/tcp") ? [x, "5432/tcp                   ALLOW IN    Anywhere"] : [x]))), "PORTS", listen("tcp", "203.0.113.5", 5432, "postgres")), fails: ["net.databases:high"] },
  { name: "a dev server that is let in", change: (s) => addLines(edit(s, "FIREWALL", (l) => l.flatMap((x) => (x.startsWith("443/tcp") ? [x, "5173/tcp                   ALLOW IN    Anywhere"] : [x]))), "PORTS", listen("tcp", "0.0.0.0", 5173, "node")), fails: ["net.other-listeners:medium"] },
  { name: "a debugger port that is let in", change: (s) => addLines(edit(s, "FIREWALL", (l) => l.flatMap((x) => (x.startsWith("443/tcp") ? [x, "9229/tcp                   ALLOW IN    Anywhere"] : [x]))), "PORTS", listen("tcp", "0.0.0.0", 9229, "node")), fails: ["net.other-listeners:critical"] },
  { name: "an odd service that is let in", change: (s) => addLines(edit(s, "FIREWALL", (l) => l.flatMap((x) => (x.startsWith("443/tcp") ? [x, "7777:7780/tcp               ALLOW IN    Anywhere"] : [x]))), "PORTS", listen("tcp", "0.0.0.0", 7778, "game")), fails: ["net.other-listeners:low"] },
  { name: "ufw is off", change: (s) => edit(s, "FIREWALL", (l) => l.map((x) => (x === "Status: active" ? "Status: inactive" : x))), fails: ["net.default-drop:high", "net.firewall-active:high"] },
  { name: "ufw lets everything in by default", change: (s) => edit(s, "FIREWALL", (l) => l.map((x) => x.replace("deny (incoming)", "allow (incoming)"))), fails: ["net.default-drop:high"] },
  // ---- privileges
  { name: "a second account with user id 0", change: (s) => addLines(s, "PASSWD", "toor:x:0:0::/root:/bin/bash"), fails: ["priv.uid0:critical"] },
  { name: "an account with no password", change: (s) => addLines(s, "SHADOW", "empty alice"), fails: ["priv.empty-passwords:critical"] },
  { name: "readable password hashes", change: (s) => setLine(s, "SHADOW", /^perm=/, "perm=644 root root"), fails: ["priv.shadow-file:high"] },
  { name: "sudo for anything, no password", change: (s) => addLines(s, "SUDOERS", "alice ALL=(ALL) NOPASSWD:ALL"), fails: ["priv.sudoers:high"] },
  { name: "sudo never asks", change: (s) => addLines(s, "SUDOERS", "Defaults !authenticate"), fails: ["priv.sudoers:high"] },
  { name: "a wildcard in a passwordless rule", change: (s) => addLines(s, "SUDOERS", "deploy ALL=(root) NOPASSWD: /usr/bin/systemctl restart app-*"), fails: ["priv.sudoers:medium"] },
  { name: "an unrestricted package manager, no password", change: (s) => addLines(s, "SUDOERS", "%ops ALL=(root) NOPASSWD: /usr/bin/apt-get"), fails: ["priv.sudoers:medium"] },
  { name: "a fixed command, no password, is fine", change: (s) => addLines(s, "SUDOERS", "%ops ALL=(root) NOPASSWD: /usr/bin/apt-get update, /usr/bin/apt-get -s upgrade"), fails: [] },
  { name: "a service account with a shell", change: (s) => addLines(s, "PASSWD", "svc:x:998:998::/var/lib/svc:/bin/bash"), fails: ["priv.service-shells:low"] },
  { name: "a set-uid shell", change: (s) => addLines(s, "SUID", "/usr/local/bin/bash"), fails: ["priv.suid:critical"] },
  { name: "a set-uid find", change: (s) => addLines(s, "SUID", "/usr/bin/find"), fails: ["priv.suid:critical"] },
  { name: "an unfamiliar set-uid program", change: (s) => addLines(s, "SUID", "/opt/vendor/bin/helper"), fails: ["priv.suid:low"] },
  { name: "a world-writable command directory", change: (s) => addLines(s, "PATHDIRS", "777 root root /opt/tools"), fails: ["priv.path-dirs:high"] },
  { name: "a program anyone can change", change: (s) => addLines(s, "PATHDIRS", "755 root root /opt/tools", "ww /opt/tools/deploy"), fails: ["priv.path-dirs:high"] },
  { name: "a directory another group can write to", change: (s) => addLines(s, "PATHDIRS", "2770 root ops /srv/shared/bin"), fails: ["priv.path-dirs:low"] },
  { name: "the user's own directory is theirs to write", change: (s) => addLines(s, "PATHDIRS", "770 root root /root/bin"), fails: [] },
  // ---- kernel
  { name: "forwarding with no reason", change: (s) => setLine(s, "SYSCTL", /^net.ipv4.ip_forward/, "net.ipv4.ip_forward=1"), fails: ["kernel.ip-forward:low"] },
  { name: "forwarding with an open forward policy", change: (s) => setLine(edit(s, "FIREWALL", (l) => l.map((x) => x.replace("deny (routed)", "allow (routed)"))), "SYSCTL", /^net.ipv4.ip_forward/, "net.ipv4.ip_forward=1"), fails: ["kernel.ip-forward:medium"] },
  { name: "forwarding on a VPN gateway", change: (s) => setLine(addLines(s, "NETROLE", "iface wg0"), "SYSCTL", /^net.ipv4.ip_forward/, "net.ipv4.ip_forward=1"), fails: [] },
  { name: "forwarding on a container host", change: (s) => setLine(edit(s, "SERVICES", (l) => [...l, "active docker.service"]), "SYSCTL", /^net.ipv4.ip_forward/, "net.ipv4.ip_forward=1"), fails: [] },
  { name: "source routing", change: (s) => setLine(s, "SYSCTL", /accept_source_route/, "net.ipv4.conf.all.accept_source_route=1"), fails: ["kernel.source-route:medium"] },
  { name: "redirects", change: (s) => setLine(s, "SYSCTL", /accept_redirects/, "net.ipv4.conf.all.accept_redirects=1"), fails: ["kernel.redirects:medium"] },
  { name: "no SYN cookies", change: (s) => setLine(s, "SYSCTL", /tcp_syncookies/, "net.ipv4.tcp_syncookies=0"), fails: ["kernel.syncookies:medium"] },
  { name: "loose reverse path filtering", change: (s) => setLine(s, "SYSCTL", /rp_filter/, "net.ipv4.conf.all.rp_filter=2"), fails: ["kernel.rp-filter:low"] },
  { name: "unprotected hard links", change: (s) => setLine(s, "SYSCTL", /protected_hardlinks/, "fs.protected_hardlinks=0"), fails: ["kernel.protected-hardlinks:medium"] },
  { name: "unprotected symlinks", change: (s) => setLine(s, "SYSCTL", /protected_symlinks/, "fs.protected_symlinks=0"), fails: ["kernel.protected-symlinks:medium"] },
  { name: "an open kernel log", change: (s) => setLine(s, "SYSCTL", /dmesg_restrict/, "kernel.dmesg_restrict=0"), fails: ["kernel.dmesg:low"] },
  { name: "visible kernel pointers", change: (s) => setLine(s, "SYSCTL", /kptr_restrict/, "kernel.kptr_restrict=1"), fails: ["kernel.kptr:low"] },
  // ---- patching, logging, intrusion detection
  { name: "automatic updates off", change: (s) => setLine(setLine(s, "PATCH", /Update-Package-Lists/, 'APT::Periodic::Update-Package-Lists "0";'), "PATCH", /Unattended-Upgrade/, 'APT::Periodic::Unattended-Upgrade "0";'), fails: ["patch.automatic:medium"] },
  { name: "unattended-upgrades not installed", change: (s) => setLine(s, "PATCH", /^unattended=/, "unattended="), fails: ["patch.automatic:medium"] },
  { name: "a few security updates waiting", change: (s) => setLine(s, "PATCH", /^security=/, "security=2"), fails: ["patch.pending:medium"] },
  { name: "many security updates waiting", change: (s) => setLine(s, "PATCH", /^security=/, "security=12"), fails: ["patch.pending:high"] },
  { name: "security updates waiting and old lists", change: (s) => setLine(setLine(s, "PATCH", /^security=/, "security=1"), "PATCH", /^stamp_mtime=/, "stamp_mtime=1799000000"), fails: ["patch.pending:high"] },
  { name: "old lists and nothing waiting", change: (s) => setLine(s, "PATCH", /^stamp_mtime=/, "stamp_mtime=1799000000"), fails: ["patch.pending:medium"] },
  { name: "no log collector", change: (s) => edit(s, "SERVICES", (l) => l.filter((x) => !/journald|rsyslog/.test(x))), fails: ["log.collector:medium"] },
  { name: "no audit daemon", change: (s) => edit(s, "SERVICES", (l) => l.filter((x) => !/auditd/.test(x))), fails: ["log.audit:low"] },
  { name: "no brute-force protection with key-only SSH", change: (s) => edit(s, "SERVICES", (l) => l.filter((x) => !/fail2ban/.test(x))), fails: ["ids.brute-force:low"] },
  { name: "no brute-force protection with passwords on", change: (s) => setLine(edit(s, "SERVICES", (l) => l.filter((x) => !/fail2ban/.test(x))), "SSHD", /^passwordauthentication/, "passwordauthentication yes"), fails: ["ids.brute-force:medium", "ssh.password-login:high"] },
  { name: "the sshd jail is off", change: (s) => setLine(s, "F2B", /^enabled/, "enabled = false"), fails: ["ids.ssh-jail:medium"] },
  { name: "the jail watches the wrong port", change: (s) => onPort6579(s), fails: ["ids.ssh-jail:medium"] },
  { name: "the jail watches the real port", change: (s) => setLine(onPort6579(s), "F2B", /^port/, "port = 6579"), fails: [] },
];

describe("what each rule catches", () => {
  for (const c of CASES) {
    it(c.name, () => {
      expect(failing(run(c.change(baseline())))).toEqual([...c.fails].sort());
    });
  }
});

describe("reading sshd's settings from files", () => {
  const files = (...parts: string[]) => ["mode=files", ...parts];
  const MAIN_FIRST = ["@file /etc/ssh/sshd_config", "Include /etc/ssh/sshd_config.d/*.conf", "Port 2222", "PasswordAuthentication yes", "X11Forwarding no"];

  it("lets the first value win, and reads the drop-ins where the Include is, in name order", () => {
    const c = parseSshd(files(...MAIN_FIRST, "@file /etc/ssh/sshd_config.d/20-b.conf", "PasswordAuthentication yes", "@file /etc/ssh/sshd_config.d/10-a.conf", "PasswordAuthentication no", "Port 22"));
    expect(c.get("PasswordAuthentication")).toEqual({ known: true, values: ["no"] });
    expect(c.get("X11Forwarding")).toEqual({ known: true, values: ["no"] });
    // Port adds up over every file.
    expect(c.get("Port").values).toEqual(["22", "2222"]);
  });

  it("reads the main file first when the Include is at the end", () => {
    const c = parseSshd(files("@file /etc/ssh/sshd_config", "PasswordAuthentication yes", "Include /etc/ssh/sshd_config.d/*.conf", "@file /etc/ssh/sshd_config.d/10-a.conf", "PasswordAuthentication no"));
    expect(c.get("passwordauthentication").values).toEqual(["yes"]);
  });

  it("ignores drop-ins nothing includes", () => {
    const c = parseSshd(files("@file /etc/ssh/sshd_config", "PasswordAuthentication yes", "@file /etc/ssh/sshd_config.d/10-a.conf", "PasswordAuthentication no"));
    expect(c.get("PasswordAuthentication").values).toEqual(["yes"]);
  });

  it("does not know a setting an unreadable file that comes first could have changed", () => {
    const c = parseSshd(files(...MAIN_FIRST, "@denied /etc/ssh/sshd_config.d/00-hardening.conf"));
    expect(c.denied).toEqual(["/etc/ssh/sshd_config.d/00-hardening.conf"]);
    expect(c.get("PasswordAuthentication").known).toBe(false);
    expect(c.get("X11Forwarding").known).toBe(false);
    // What the readable files do say about Port is still given.
    expect(c.get("Port")).toEqual({ known: false, values: ["2222"] });
  });

  it("does know a setting that a readable file before the unreadable one decides", () => {
    const c = parseSshd(files(...MAIN_FIRST, "@file /etc/ssh/sshd_config.d/10-a.conf", "X11Forwarding yes", "@denied /etc/ssh/sshd_config.d/20-b.conf"));
    expect(c.get("X11Forwarding")).toEqual({ known: true, values: ["yes"] });
    // But one only the main file sets comes after the gap.
    expect(c.get("PasswordAuthentication").known).toBe(false);
  });

  it("does not know an unset setting while any file is unreadable, and does when all are read", () => {
    expect(parseSshd(files(...MAIN_FIRST, "@denied /etc/ssh/sshd_config.d/00-a.conf")).get("MaxAuthTries")).toEqual({ known: false, values: [] });
    expect(parseSshd(files(...MAIN_FIRST)).get("MaxAuthTries")).toEqual({ known: true, values: [] });
  });

  it("stops at a Match block, which applies only to some connections", () => {
    const c = parseSshd(files("@file /etc/ssh/sshd_config", "PasswordAuthentication no", "Match User backup", "PasswordAuthentication yes", "X11Forwarding yes"));
    expect(c.get("PasswordAuthentication").values).toEqual(["no"]);
    expect(c.get("X11Forwarding").values).toEqual([]);
  });

  it("treats an Include it did not collect, and an unreadable main file, as gaps", () => {
    expect(parseSshd(files("@file /etc/ssh/sshd_config", "Include /etc/ssh/other.d/*.conf")).get("MaxAuthTries").known).toBe(false);
    expect(parseSshd(files("@denied /etc/ssh/sshd_config")).get("Port").known).toBe(false);
    expect(parseSshd(files(...MAIN_FIRST, "@denied /etc/ssh/sshd_config.d/")).get("PasswordAuthentication").known).toBe(false);
  });

  it("takes sshd -T as complete, and says when there is no sshd", () => {
    const eff = parseSshd(["mode=effective", "port 22", "port 2222", "permitrootlogin no"]);
    expect(eff.get("Port").values).toEqual(["22", "2222"]);
    expect(eff.get("AllowUsers")).toEqual({ known: true, values: [] });
    expect(parseSshd(["mode=none"]).mode).toBe("none");
    expect(parseSshd([]).mode).toBe("none");
  });

  it("skips every SSH check on a host with no sshd, rather than passing them", () => {
    const s = setLine(baseline(), "SSHD", /^mode=/, "mode=none");
    const r = run({ ...s, SSHD: ["mode=none"] });
    expect(r.checks.some((c) => c.domain === "ssh")).toBe(false);
  });
});

describe("reading ufw's rules", () => {
  it("reads defaults, ports, ranges, lists and what comes from only one address", () => {
    const u = parseUfwStatus([
      "Status: active",
      "Default: deny (incoming), allow (outgoing), disabled (routed)",
      "",
      "To                         Action      From",
      "--                         ------      ----",
      "22/tcp                     ALLOW IN    Anywhere",
      "80,443/tcp                 ALLOW IN    Anywhere",
      "6000:6010/udp              ALLOW IN    Anywhere",
      "3306                       ALLOW IN    10.0.0.0/8",
      "9000/tcp                   DENY IN     Anywhere",
      "Nginx Full                 ALLOW IN    Anywhere",
      "443/tcp (v6)               ALLOW IN    Anywhere (v6)",
    ]);
    expect(u).toMatchObject({ active: true, defaultIn: "drop", defaultRouted: null, unparsed: 1 });
    expect(u.rules).toHaveLength(5);
    expect(u.rules[1]).toEqual({ ports: [[80, 80], [443, 443]], proto: "tcp", fromAnywhere: true });
    expect(u.rules[2]).toEqual({ ports: [[6000, 6010]], proto: "udp", fromAnywhere: true });
    expect(u.rules[3].fromAnywhere).toBe(false);
  });

  it("says a port is blocked only when every rule was understood", () => {
    const withProfile = ufwRule(baseline(), "Nginx Full                 ALLOW IN    Anywhere");
    const r = run(addLines(withProfile, "PORTS", listen("tcp", "0.0.0.0", 6379, "redis-server")));
    // Cannot tell whether "Nginx Full" covers 6379, so the exposure is reported with its caveat.
    expect(failing(r)).toEqual(["net.databases:high"]);
    expect(r.checks.find((c) => c.id === "net.databases")?.finding).toContain("could not be read");
  });

  it("does not count a rule that lets a port in from one address only", () => {
    const s = ufwRule(baseline(), "5432/tcp                   ALLOW IN    10.0.0.0/8");
    expect(failing(run(addLines(s, "PORTS", listen("tcp", "0.0.0.0", 5432, "postgres"))))).toEqual([]);
    // The same port, let in from anywhere, is the finding: so the test above is not passing for want of a rule.
    const open = ufwRule(baseline(), "5432/tcp                   ALLOW IN    Anywhere");
    expect(failing(run(addLines(open, "PORTS", listen("tcp", "0.0.0.0", 5432, "postgres"))))).toEqual(["net.databases:high"]);
  });

  it("lets everything in while ufw is inactive", () => {
    const s = edit(baseline(), "FIREWALL", (l) => l.map((x) => (x === "Status: active" ? "Status: inactive" : x)));
    expect(failing(run(addLines(s, "PORTS", listen("tcp", "0.0.0.0", 6379, "redis-server"))))).toContain("net.databases:high");
  });
});

describe("reading sudoers", () => {
  it("reads who may run what without a password, through aliases and continued lines", () => {
    const { rules, noAuthenticate } = parseSudoers([
      "Defaults env_reset",
      "Cmnd_Alias PKG = /usr/bin/apt-get, /usr/bin/dpkg",
      "root ALL=(ALL:ALL) ALL",
      "%ops ALL=(root) NOPASSWD: PKG, /usr/bin/systemctl restart nginx",
      "deploy ALL=(ALL) NOPASSWD: /usr/bin/rsync -a /srv/, \\",
      "    /usr/bin/ls",
      "ci ALL = (ALL) NOPASSWD: SETENV: ALL",
    ]);
    expect(noAuthenticate).toBe(false);
    expect(rules).toEqual([
      { who: "%ops", commands: ["/usr/bin/apt-get", "/usr/bin/dpkg", "/usr/bin/systemctl restart nginx"] },
      { who: "deploy", commands: ["/usr/bin/rsync -a /srv/", "/usr/bin/ls"] },
      { who: "ci", commands: ["ALL"] },
    ]);
    expect(parseSudoers(["Defaults !authenticate"]).noAuthenticate).toBe(true);
  });

  it("flags an alias that hides ALL or an unrestricted pager", () => {
    const s = addLines(baseline(), "SUDOERS", "Cmnd_Alias EDIT = /usr/bin/vim", "ops ALL=(root) NOPASSWD: EDIT");
    expect(failing(run(s))).toEqual(["priv.sudoers:medium"]);
  });
});

describe("a host read without root", () => {
  const real = readFileSync("src/lib/__tests__/hostaudit/ubuntu-user.txt", "utf8");
  const r = auditHost(real);

  it("reports what it could read and says plainly what it could not", () => {
    expect(r.supported && r.complete).toBe(true);
    expect(r.root).toBe(false);
    expect(r.hostname).toBe("web-01");
    const unknown = r.checks.filter((c) => c.outcome === "unknown").map((c) => c.id).sort();
    expect(unknown).toEqual([
      "priv.empty-passwords",
      "priv.sudoers",
      "ssh.algorithms",
      "ssh.allow-list",
      "ssh.empty-passwords",
      "ssh.idle-timeout",
      "ssh.max-auth-tries",
      "ssh.password-login",
      "ssh.pubkey",
      "ssh.root-login",
      "ssh.x11",
    ]);
    // An unknown check always says how to find out.
    for (const c of r.checks.filter((c) => c.outcome === "unknown")) expect(c.remediation).toMatch(/root|Check/);
  });

  it("finds what a person reading the same host by hand found", () => {
    expect(failing(r)).toEqual([
      "kernel.ip-forward:medium",
      "kernel.kptr:low",
      "kernel.rp-filter:low",
      "log.audit:low",
      "net.other-listeners:medium",
      "patch.automatic:medium",
      "patch.pending:high",
      "priv.path-dirs:low",
      "priv.service-shells:low",
    ]);
    const pending = r.checks.find((c) => c.id === "patch.pending");
    expect(pending?.finding).toMatch(/40 security updates are waiting \(75 updates in all\); the package lists are 24 days old/);
    expect(r.checks.find((c) => c.id === "net.other-listeners")?.finding).toMatch(/dev server \(node on every interface, port 5173\/tcp\)/);
  });

  it("does not report sshd's own port, the loopback databases, or rootless Docker's root as problems", () => {
    for (const id of ["net.databases", "net.docker-api", "docker.published-ports", "docker.root-user", "ids.ssh-jail", "priv.suid", "priv.uid0"]) expect(outcome(r, id)).toBe("pass");
    expect(r.checks.find((c) => c.id === "net.other-listeners")?.finding).not.toContain("6579");
  });

  it("scores only what could be judged, and shows the range", () => {
    expect(r.score).toMatchObject({ passed: 20, failed: 9, unknown: 11, percent: 69, worst: 50, best: 78 });
    expect(scoreText(r.score)).toBe("69% (50% to 78% once the 11 unchecked items are known)");
    expect(r.bySeverity).toEqual({ critical: 0, high: 1, medium: 3, low: 5, info: 0 });
  });
});

describe("the report as text", () => {
  const r = auditHost(readFileSync("src/lib/__tests__/hostaudit/ubuntu-user.txt", "utf8"));
  const text = auditText(r, Date.UTC(2026, 9, 30), "web-01");

  it("follows the order a reader expects: scorecard, findings worst first, what was not checked, an order to fix", () => {
    expect(text).toContain("Security audit of web-01");
    expect(text).toContain("without root: checks that need it are listed as not checked");
    expect(text).toContain("Passed 20, failed 9, not checked 11");
    const high = text.indexOf("[HIGH] Waiting security updates");
    const medium = text.indexOf("[MEDIUM]");
    const low = text.indexOf("[LOW]");
    expect(high).toBeGreaterThan(-1);
    expect(high).toBeLessThan(medium);
    expect(medium).toBeLessThan(low);
    expect(text).toMatch(/Component: .*\n {2}Finding: .*\n {2}Risk: .*\n {2}Remediation: /);
    expect(text).toContain("Not checked (needs access this account does not have)");
    expect(text).toContain("First (critical and high): Waiting security updates");
    expect(text).toContain("have someone with root run the checks listed as not checked");
  });

  it("puts the worst first", () => {
    const order = bySeverityOrder(r.checks).map((c) => c.severity);
    const rank: Severity[] = ["critical", "high", "medium", "low", "info"];
    expect(order).toEqual([...order].sort((a, z) => rank.indexOf(a as Severity) - rank.indexOf(z as Severity)));
  });

  it("says so when there is nothing wrong", () => {
    const clean = auditText(run(baseline()), 0, "web-01");
    expect(clean).toContain("Hardening score: 100%");
    expect(clean).toContain("No findings");
    expect(clean).toContain("2. Findings\n   None.");
    expect(clean).not.toContain("Not checked");
  });
});

describe("output that is not what was hoped for", () => {
  it("says a host that is not Linux is not supported, and never throws", () => {
    const r = auditHost("@@HOST\nos=Darwin\nuid=501\n@@END\n");
    expect(r.supported).toBe(false);
    expect(r.checks).toEqual([]);
    expect(r.score.percent).toBeNull();
    expect(scoreText(r.score)).toBe("Nothing could be judged.");
  });

  it("notices output that stopped early", () => {
    const { END: _end, ...cut } = baseline();
    expect(run(cut).complete).toBe(false);
    expect(run(baseline()).complete).toBe(true);
  });

  it("copes with nothing, noise and a half-written section", () => {
    for (const text of ["", "hello\nworld", "@@HOST", "@@HOST\nos=Linux\n@@SSHD\nmode=files\n@file", "\u0000\u0001@@@@@\n@@"]) {
      expect(() => auditHost(text)).not.toThrow();
    }
    const r = auditHost("@@HOST\nos=Linux\nuid=1000\n@@END\n");
    expect(r.supported).toBe(true);
    // With nothing collected, nothing is a pass: every check that ran is unknown or skipped.
    expect(r.checks.filter((c) => c.outcome === "pass")).toEqual([]);
  });

  it("accepts Windows line endings", () => {
    const r = auditHost(render(baseline()).replace(/\n/g, "\r\n"));
    expect(failing(r)).toEqual([]);
    expect(r.score.percent).toBe(100);
  });
});
