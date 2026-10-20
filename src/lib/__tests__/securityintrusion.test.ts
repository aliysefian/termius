import { describe, expect, it } from "vitest";
import { AUTH_LOG_SCRIPT, banScript, geoScript, killSessionScript, parseAuthOutput, parseFail2ban, parseGeo, parseLast, parseLogTime, parseSessions, parseWho, summarizeAuthLog } from "$lib/security/intrusion";

// Taken from a real host (util-linux `who` and `last -F`).
const WHO = `ali      pts/1        2026-10-20 06:57 (151.234.175.42)
safa     pts/2        2026-10-16 22:08 (tmux(1850045).%14)
root     tty1         2026-10-01 08:00
`;
const LAST = `ali      pts/1        Tue Oct 20 06:57:57 2026   still logged in                       151.234.175.42
ali      pts/10       Mon Oct 19 23:11:29 2026 - Mon Oct 19 23:11:29 2026  (00:00)     tmux(2265851).%2
deploy   pts/3        Sun Oct 18 10:00:00 2026 - Sun Oct 18 10:15:30 2026  (00:15)     198.51.100.7
root     tty1         Sun Oct 18 09:00:00 2026 - down                      (00:30)     0.0.0.0
reboot   system boot  Sun Oct 18 08:59:00 2026   still running                         6.8.0-101-generic

wtmp begins Mon Nov 11 08:24:14 2024
`;

describe("current sessions", () => {
  it("reads who, hiding tmux's pseudo-hosts", () => {
    const w = parseWho(WHO, new Map([["pts/1", "-bash"]]));
    expect(w).toEqual([
      { user: "ali", tty: "pts/1", source: "151.234.175.42", since: "2026-10-20 06:57", command: "-bash" },
      { user: "safa", tty: "pts/2", source: "", since: "2026-10-16 22:08", command: "" },
      { user: "root", tty: "tty1", source: "", since: "2026-10-01 08:00", command: "" },
    ]);
  });
  it("reads the last logins and leaves out reboots", () => {
    const l = parseLast(LAST);
    expect(l.map((x) => [x.user, x.tty, x.source, x.when, x.duration])).toEqual([
      ["ali", "pts/1", "151.234.175.42", "Tue Oct 20 06:57:57 2026", "still logged in"],
      ["ali", "pts/10", "", "Mon Oct 19 23:11:29 2026", "00:00"],
      ["deploy", "pts/3", "198.51.100.7", "Sun Oct 18 10:00:00 2026", "00:15"],
      ["root", "tty1", "0.0.0.0", "Sun Oct 18 09:00:00 2026", "00:30"],
    ]);
  });
  it("splits the script's sections", () => {
    const s = parseSessions(`@@who\n${WHO}@@cmds\npts/1|vim notes.txt\n@@last\n${LAST}@@end\n`);
    expect(s.current[0].command).toBe("vim notes.txt");
    expect(s.recent).toHaveLength(4);
  });
  it("only ever kills a terminal by its name", () => {
    expect(killSessionScript("pts/3")).toBe("pkill -KILL -t 'pts/3'");
    expect(() => killSessionScript("pts/3; reboot")).toThrow();
    expect(() => killSessionScript("../../x")).toThrow();
  });
});

describe("fail2ban", () => {
  const OUT = `@@f2b
Status
|- Number of jail:	2
\`- Jail list:	nginx-http-auth, sshd
@@jail nginx-http-auth
Status for the jail: nginx-http-auth
|- Filter
|  |- Currently failed:	0
|  |- Total failed:	3
|  \`- File list:	/var/log/nginx/error.log
\`- Actions
   |- Currently banned:	0
   |- Total banned:	0
   \`- Banned IP list:	
@@jail sshd
Status for the jail: sshd
|- Filter
|  |- Currently failed:	4
|  |- Total failed:	981
|  \`- Journal matches:	_SYSTEMD_UNIT=sshd.service + _COMM=sshd
\`- Actions
   |- Currently banned:	2
   |- Total banned:	57
   \`- Banned IP list:	185.211.57.3 2001:db8::5
@@bans
14
@@end
`;
  it("reads the jails, the banned list and the day's bans", () => {
    const f = parseFail2ban(OUT);
    expect(f).toMatchObject({ installed: true, running: true, bans24h: 14 });
    expect(f.jails.map((j) => j.name)).toEqual(["nginx-http-auth", "sshd"]);
    expect(f.jails[1]).toEqual({ name: "sshd", currentlyFailed: 4, totalFailed: 981, currentlyBanned: 2, totalBanned: 57, banned: ["185.211.57.3", "2001:db8::5"] });
    expect(f.jails[0].banned).toEqual([]);
  });
  it("tells not installed from not running", () => {
    expect(parseFail2ban("@@f2b\nNOT-INSTALLED\n@@end")).toMatchObject({ installed: false });
    expect(parseFail2ban("@@f2b\nERROR   Failed to access socket path: /var/run/fail2ban/fail2ban.sock. Is fail2ban running?\n@@end")).toMatchObject({ installed: true, running: false });
  });
  it("builds ban commands from checked words only", () => {
    expect(banScript("sshd", "1.2.3.4", true)).toBe("fail2ban-client set 'sshd' banip '1.2.3.4'");
    expect(banScript("sshd", "1.2.3.4", false)).toBe("fail2ban-client set 'sshd' unbanip '1.2.3.4'");
    expect(() => banScript("sshd; id", "1.2.3.4", true)).toThrow();
    expect(() => banScript("sshd", "1.2.3.4 && id", true)).toThrow();
  });
});

const NOW = new Date("2026-10-20T10:30:00Z");
const LOG = `2026-10-20T10:10:00+0000 host sshd[1]: Invalid user admin from 185.211.57.3 port 40001
2026-10-20T10:10:01+0000 host sshd[1]: Failed password for invalid user admin from 185.211.57.3 port 40001 ssh2
2026-10-20T10:12:00+0000 host sshd[1]: Failed password for root from 185.211.57.3 port 40002 ssh2
2026-10-20T10:13:00+0000 host sshd[1]: Failed password for root from 185.211.57.3 port 40003 ssh2
2026-10-20T10:20:00+0000 host sshd[1]: Failed password for ubuntu from 198.51.100.9 port 5 ssh2
2026-10-20T08:00:00+0000 host sshd[1]: Failed password for root from 198.51.100.9 port 6 ssh2
2026-10-20T10:25:00+0000 host sshd[1]: Invalid user test from 203.0.113.50 port 7
2026-10-20T10:26:00+0000 host sshd[1]: Accepted publickey for ali from 151.234.175.42 port 9 ssh2
2026-10-20T10:26:00+0000 host sshd[1]: Failed password for root from not-an-ip port 9 ssh2
`;

describe("failed logins", () => {
  const s = summarizeAuthLog(LOG, NOW);
  it("groups by address, an invalid user and its failed password counting once", () => {
    expect(s.attackers.map((a) => [a.ip, a.attempts])).toEqual([
      ["185.211.57.3", 3],
      ["198.51.100.9", 2],
      ["203.0.113.50", 1],
    ]);
    expect(s.attackers[0].users.sort()).toEqual(["admin", "root"]);
  });
  it("remembers the latest time and counts the last hour", () => {
    expect(s.attackers[0].lastSeen?.toISOString()).toBe("2026-10-20T10:13:00.000Z");
    // 3 + 1 failed-password lines in the hour; the 08:00 one and the bare "Invalid user" don't count.
    expect(s.failedLastHour).toBe(4);
    expect(s.failedTotal).toBe(5);
  });
  it("reads classic syslog times, which have no year", () => {
    expect(parseLogTime("Oct 20 10:15:03 host sshd", NOW)?.toISOString()).toBe("2026-10-20T10:15:03.000Z");
    expect(parseLogTime("Dec 31 23:59:59 host", new Date("2027-01-01T00:10:00Z"))?.getUTCFullYear()).toBe(2026);
    expect(parseLogTime("nonsense", NOW)).toBeNull();
  });
  it("takes the clock from the host, not from here", () => {
    const o = parseAuthOutput(`@@now\n1792492200\n@@log\nline one\nline two\n@@end\n`);
    expect(o.now.getTime()).toBe(1792492200 * 1000);
    expect(o.log).toBe("line one\nline two\n");
    expect(AUTH_LOG_SCRIPT).toContain("journalctl");
    expect(AUTH_LOG_SCRIPT).toContain("/var/log/auth.log");
    expect(AUTH_LOG_SCRIPT).toContain("/var/log/secure");
  });
  it("looks countries up on the host, offline, for valid addresses only", () => {
    expect(geoScript(["1.2.3.4", "x; id"])).toContain("'1.2.3.4'");
    expect(geoScript(["x; id"])).toBe("true");
    expect(geoScript(["1.2.3.4"])).not.toMatch(/curl|wget|http/);
    expect([...parseGeo("1.2.3.4|GeoIP Country Edition: CN, China\n5.6.7.8|IP Address not found\n")]).toEqual([["1.2.3.4", "CN China"]]);
  });
});
