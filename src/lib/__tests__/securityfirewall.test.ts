import { describe, expect, it } from "vitest";
import { isIP, isIPv4, isSource, portsInclude, sourceCovers } from "$lib/security/net";
import {
  allowScript, blockScript, checkAllowRequest, checkBlock, checkDelete, checkEnable, deleteScript, enableUfwScript, incomingVerdict, parseFirewall,
  parseFirewalld, parseIptables, parseNft, parseUfwAdded, parseUfwNumbered, parseUfwVerbose, type FirewallState,
} from "$lib/security/firewall";

describe("addresses", () => {
  it("tells addresses from anything else", () => {
    expect(["1.2.3.4", "255.255.255.255", "::1", "2001:db8::1", "fe80::1"].every(isIP)).toBe(true);
    expect(["1.2.3", "256.1.1.1", "a.b.c.d", "1.2.3.4; rm -rf /", "$(id)", "", "1.2.3.4/24"].some(isIP)).toBe(false);
    expect(["10.0.0.0/8", "1.2.3.4", "2001:db8::/32"].every(isSource)).toBe(true);
    expect(["10.0.0.0/33", "10.0.0.0/8/1", "x/8", "10.0.0.0/ 8"].some(isSource)).toBe(false);
    expect(isIPv4("01.2.3.4")).toBe(true);
  });
  it("knows what a source covers", () => {
    expect(sourceCovers("Anywhere", "9.9.9.9")).toBe(true);
    expect(sourceCovers("185.211.57.0/24", "185.211.57.152")).toBe(true);
    expect(sourceCovers("185.211.57.0/24", "185.211.58.1")).toBe(false);
    expect(sourceCovers("10.0.0.5", "10.0.0.5")).toBe(true);
    expect(sourceCovers("10.0.0.5", "10.0.0.6")).toBe(false);
    expect(sourceCovers("0.0.0.0/0", "1.1.1.1")).toBe(true);
    expect(sourceCovers("garbage", "1.1.1.1")).toBe(false);
  });
  it("reads port lists and ranges", () => {
    expect(portsInclude("22", 22)).toBe(true);
    expect(portsInclude("80,443", 443)).toBe(true);
    expect(portsInclude("6000:6007", 6003)).toBe(true);
    expect(portsInclude("1000-2000", 22)).toBe(false);
    expect(portsInclude("", 22)).toBe(true);
  });
});

const UFW_VERBOSE = `Status: active
Logging: on (low)
Default: deny (incoming), allow (outgoing), disabled (routed)
New profiles: skip

To                         Action      From
--                         ------      ----
22/tcp                     ALLOW IN    Anywhere
`;

const UFW_NUMBERED = `Status: active

     To                         Action      From
     --                         ------      ----
[ 1] 22/tcp                     ALLOW IN    Anywhere                   # ssh
[ 2] 80,443/tcp                 ALLOW IN    Anywhere
[ 3] Anywhere                   DENY IN     185.211.57.0/24
[ 4] 22/tcp                     LIMIT IN    10.0.0.0/8                 # office ssh
[ 5] OpenSSH (v6)               ALLOW IN    Anywhere (v6)
[ 6] 6000:6007/udp              ALLOW IN    192.168.1.5
`;

describe("ufw", () => {
  it("reads the status and the default policies", () => {
    expect(parseUfwVerbose(UFW_VERBOSE)).toEqual({ active: true, installed: true, policies: { incoming: "deny", outgoing: "allow", routed: "disabled" } });
    expect(parseUfwVerbose("Status: inactive")).toMatchObject({ active: false, installed: true });
    expect(parseUfwVerbose("")).toMatchObject({ installed: false });
  });
  it("reads the numbered rules", () => {
    const r = parseUfwNumbered(UFW_NUMBERED);
    expect(r).toHaveLength(6);
    expect(r[0]).toMatchObject({ num: 1, action: "allow", ports: "22", proto: "tcp", source: "Anywhere", comment: "ssh", direction: "in" });
    expect(r[1]).toMatchObject({ ports: "80,443", proto: "tcp" });
    expect(r[2]).toMatchObject({ action: "deny", ports: "", source: "185.211.57.0/24" });
    expect(r[3]).toMatchObject({ action: "limit", source: "10.0.0.0/8", comment: "office ssh" });
    expect(r[4]).toMatchObject({ ports: "OpenSSH", v6: true, source: "Anywhere" });
    expect(r[5]).toMatchObject({ ports: "6000:6007", proto: "udp" });
  });
  it("reads the rules queued while it is off", () => {
    const r = parseUfwAdded(`Added user rules (see 'ufw status' for running firewall):
ufw allow 22/tcp
ufw allow from 10.0.0.5 to any port 5432 proto tcp comment 'db'
ufw deny from 1.2.3.4
`);
    expect(r.map((x) => [x.action, x.ports, x.proto, x.source])).toEqual([
      ["allow", "22", "tcp", "Anywhere"],
      ["allow", "5432", "tcp", "10.0.0.5"],
      ["deny", "", "", "1.2.3.4"],
    ]);
    expect(r[1].comment).toBe("db");
  });
});

const IPTABLES = `Chain INPUT (policy DROP 120 packets, 7200 bytes)
num   pkts bytes target     prot opt in     out     source               destination
1     312K   45M ACCEPT     all  --  lo     *       0.0.0.0/0            0.0.0.0/0
2     1500  900K ACCEPT     all  --  *      *       0.0.0.0/0            0.0.0.0/0            ctstate RELATED,ESTABLISHED
3       25  1500 ACCEPT     tcp  --  *      *       0.0.0.0/0            0.0.0.0/0            tcp dpt:22 /* ssh */
4        0     0 ACCEPT     tcp  --  *      *       10.0.0.0/8           0.0.0.0/0            multiport dports 80,443
5      900 54000 DROP       all  --  *      *       185.211.57.0/24      0.0.0.0/0
6        7   400 ufw-user-input  all  --  *      *       0.0.0.0/0            0.0.0.0/0

Chain FORWARD (policy DROP 0 packets, 0 bytes)
num   pkts bytes target     prot opt in     out     source               destination

Chain OUTPUT (policy ACCEPT 5 packets, 300 bytes)
num   pkts bytes target     prot opt in     out     source               destination

Chain ufw-user-input (1 references)
num   pkts bytes target     prot opt in     out     source               destination
1        0     0 ACCEPT     tcp  --  *      *       0.0.0.0/0            0.0.0.0/0            tcp dpt:9999
`;

describe("iptables", () => {
  const p = parseIptables(IPTABLES);
  it("reads policies and counters", () => {
    expect(p.policies).toEqual({ incoming: "drop", routed: "drop", outgoing: "allow" });
    expect(p.rules[0]).toMatchObject({ num: 1, packets: 312_000, bytes: 45_000_000, action: "allow", chain: "INPUT" });
  });
  it("reads ports, sources and comments, and skips user chains", () => {
    expect(p.rules.map((r) => r.ports)).toEqual(["", "", "22", "80,443", "", ""]);
    expect(p.rules[2]).toMatchObject({ comment: "ssh", proto: "tcp", source: "Anywhere" });
    expect(p.rules[3].source).toBe("10.0.0.0/8");
    expect(p.rules[4]).toMatchObject({ action: "drop", source: "185.211.57.0/24" });
    expect(p.rules[5]).toMatchObject({ action: "other", comment: "→ ufw-user-input" });
    expect(p.rules.some((r) => r.ports === "9999")).toBe(false);
  });
});

const NFT = `table inet filter { # handle 1
	chain input { # handle 1
		type filter hook input priority filter; policy drop;
		ct state established,related accept # handle 4
		iif "lo" accept # handle 5
		ip saddr 10.0.0.0/8 tcp dport { 80, 443 } counter packets 10 bytes 600 accept # handle 7
		tcp dport 22 counter packets 55 bytes 4000 accept comment "ssh" # handle 8
		ip saddr 185.211.57.0/24 drop # handle 9
	}
	chain forward { # handle 2
		type filter hook forward priority filter; policy drop;
	}
	chain output { # handle 3
		type filter hook output priority filter; policy accept;
	}
}
`;

describe("nftables", () => {
  const p = parseNft(NFT);
  it("reads the policies and where new rules go", () => {
    expect(p.policies).toEqual({ incoming: "drop", routed: "drop", outgoing: "allow" });
    expect(p.place).toEqual({ family: "inet", table: "filter", chain: "input" });
  });
  it("reads rules with their handles, counters and comments", () => {
    expect(p.rules).toHaveLength(5);
    expect(p.rules[2]).toMatchObject({ handle: 7, ports: "80,443", proto: "tcp", source: "10.0.0.0/8", packets: 10, bytes: 600, action: "allow" });
    expect(p.rules[3]).toMatchObject({ handle: 8, ports: "22", comment: "ssh", packets: 55 });
    expect(p.rules[4]).toMatchObject({ action: "drop", source: "185.211.57.0/24", table: "inet filter" });
  });
});

const FIREWALLD = `running
public (active)
  target: default
  icmp-block-inversion: no
  interfaces: eth0
  services: dhcpv6-client ssh
  ports: 8080/tcp 5000-5010/udp
  protocols:
  forward: yes
  rich rules:
	rule family="ipv4" source address="203.0.113.7" reject
	rule family="ipv4" source address="198.51.100.0/24" port port="22" protocol="tcp" accept
`;

describe("firewalld", () => {
  it("reads services, ports and rich rules of the active zone", () => {
    const f = parseFirewalld(FIREWALLD);
    expect(f.running).toBe(true);
    expect(f.zone).toBe("public");
    expect(f.rules.map((r) => [r.action, r.ports, r.proto, r.source])).toEqual([
      ["allow", "dhcpv6-client", "", "Anywhere"],
      ["allow", "ssh", "", "Anywhere"],
      ["allow", "8080", "tcp", "Anywhere"],
      ["allow", "5000:5010", "udp", "Anywhere"],
      ["reject", "", "", "203.0.113.7"],
      ["allow", "22", "tcp", "198.51.100.0/24"],
    ]);
  });
});

const probe = (parts: Record<string, string>) => Object.entries(parts).map(([k, v]) => `@@${k}\n${v}`).join("\n") + "\n@@end\n";

describe("which firewall is in charge", () => {
  it("prefers an active ufw", () => {
    const s = parseFirewall(probe({ ufw: UFW_VERBOSE, "ufw-numbered": UFW_NUMBERED, iptables: IPTABLES }));
    expect(s).toMatchObject({ backend: "ufw", active: true, policies: { incoming: "deny" } });
    expect(s.rules).toHaveLength(6);
  });
  it("falls to nftables, then iptables, when ufw is off", () => {
    expect(parseFirewall(probe({ ufw: "Status: inactive", nft: NFT, iptables: "" }))).toMatchObject({ backend: "nftables", active: true });
    const s = parseFirewall(probe({ ufw: "Status: inactive", iptables: IPTABLES }));
    expect(s).toMatchObject({ backend: "iptables", active: true });
    expect(s.notes[0]).toMatch(/ufw is installed but disabled/);
  });
  it("reports firewalld when it runs", () => {
    expect(parseFirewall(probe({ firewalld: FIREWALLD }))).toMatchObject({ backend: "firewalld", active: true });
  });
  it("calls an open host inactive, and shows what ufw has queued", () => {
    const open = `Chain INPUT (policy ACCEPT 0 packets, 0 bytes)\nnum   pkts bytes target     prot opt in     out     source               destination\n`;
    const s = parseFirewall(probe({ ufw: "Status: inactive", "ufw-added": "ufw allow 22/tcp\n", iptables: open }));
    expect(s).toMatchObject({ backend: "ufw", active: false });
    expect(s.rules).toHaveLength(1);
    expect(parseFirewall(probe({ iptables: open }))).toMatchObject({ backend: "iptables", active: false });
    expect(parseFirewall("")).toMatchObject({ backend: "none", active: false });
  });
});

const ufwState = (): FirewallState => parseFirewall(probe({ ufw: UFW_VERBOSE, "ufw-numbered": UFW_NUMBERED }));
const ME = { clientIp: "203.0.113.9", sshPort: 22 };

describe("who gets in", () => {
  it("applies the first matching rule, then the default", () => {
    const s = ufwState();
    expect(incomingVerdict(s, 22, "203.0.113.9")).toBe("allow");
    expect(incomingVerdict(s, 9999, "203.0.113.9")).toBe("deny");
    expect(incomingVerdict(s, 443, "1.1.1.1")).toBe("allow");
    // A deny ahead of the allow wins for its source.
    const denied = { ...s, rules: [{ ...s.rules[2], ports: "" }, ...s.rules] };
    expect(incomingVerdict(denied, 22, "185.211.57.10")).toBe("deny");
  });
  it("does not delete the rule that is the way in", () => {
    const s = ufwState();
    expect(checkDelete(s, s.rules[0], ME).ok).toBe(false);
    expect(checkDelete(s, s.rules[0], ME).reason).toMatch(/203\.0\.113\.9/);
    expect(checkDelete(s, s.rules[1], ME).ok).toBe(true);
  });
  it("allows deleting one of two rules that let you in", () => {
    const s = ufwState();
    const twin = { ...s, rules: [...s.rules, { ...s.rules[0], id: "ufw-9", num: 9 }] };
    expect(checkDelete(twin, twin.rules[0], ME).ok).toBe(true);
  });
  it("does not block a source that includes you", () => {
    expect(checkBlock("203.0.113.9", ME).ok).toBe(false);
    expect(checkBlock("203.0.113.0/24", ME).ok).toBe(false);
    expect(checkBlock("198.51.100.0/24", ME).ok).toBe(true);
  });
  it("does not turn the firewall on without a way in", () => {
    const off = (rules: FirewallState["rules"]): FirewallState => ({ backend: "ufw", active: false, policies: {}, rules, notes: [] });
    expect(checkEnable(off([]), ME).ok).toBe(false);
    expect(checkEnable(off(parseUfwAdded("ufw allow 22/tcp")), ME).ok).toBe(true);
    expect(checkEnable(off(parseUfwAdded("ufw allow from 10.0.0.1 to any port 22 proto tcp")), ME).ok).toBe(false);
  });
});

describe("scripts", () => {
  const s = ufwState();
  it("refuses what is not an address, a port or a time", () => {
    const base = { ip: "1.2.3.4", port: "22", proto: "tcp" as const };
    expect(checkAllowRequest(base)).toBeNull();
    expect(checkAllowRequest({ ...base, ip: "1.2.3.4; reboot" })).not.toBeNull();
    expect(checkAllowRequest({ ...base, port: "22; id" })).not.toBeNull();
    expect(checkAllowRequest({ ...base, ttlSecs: 5 })).not.toBeNull();
    expect(checkAllowRequest({ ...base, comment: "it's" })).not.toBeNull();
    expect(() => allowScript(s, { ...base, ip: "$(id)" })).toThrow();
    expect(() => blockScript(s, "x y")).toThrow();
  });
  it("adds an allow rule, forever or for a while", () => {
    expect(allowScript(s, { ip: "1.2.3.4", port: "22", proto: "tcp" })).toBe("ufw allow from '1.2.3.4' to any port 22 proto tcp");
    const timed = allowScript(s, { ip: "1.2.3.4", port: "22", proto: "tcp", ttlSecs: 7200 });
    expect(timed).toContain("--on-active=7200s");
    expect(timed).toContain("at now + 120 minutes");
    expect(timed).toContain("exit 90");
    // The rule is added only together with its removal, so a host with no timer gets no rule.
    expect(timed.indexOf("exit 90")).toBeGreaterThan(timed.indexOf("elif"));
    expect(timed).toContain("ufw --force delete allow from");
  });
  it("writes each backend's own commands", () => {
    const ipt = parseFirewall(probe({ iptables: IPTABLES }));
    expect(allowScript(ipt, { ip: "1.2.3.4", port: "22", proto: "tcp" })).toBe("iptables -I INPUT 1 -s '1.2.3.4' -p tcp --dport 22 -j ACCEPT");
    expect(blockScript(ipt, "1.2.3.4")).toBe("iptables -I INPUT 1 -s '1.2.3.4' -j DROP");
    expect(blockScript(s, "1.2.3.4")).toBe("ufw insert 1 deny from '1.2.3.4'");
    const nft = parseFirewall(probe({ nft: NFT }));
    expect(allowScript(nft, { ip: "1.2.3.4", port: "22", proto: "tcp" })).toBe('nft insert rule inet filter input ip saddr 1.2.3.4 tcp dport 22 accept comment "sshvault"');
    const fw = parseFirewall(probe({ firewalld: FIREWALLD }));
    expect(allowScript(fw, { ip: "1.2.3.4", port: "22", proto: "tcp", ttlSecs: 3600 })).toContain("--timeout=3600");
  });
  it("deletes by the number or handle the host gave", () => {
    expect(deleteScript(s, s.rules[1])).toBe("ufw --force delete 2");
    const ipt = parseFirewall(probe({ iptables: IPTABLES }));
    expect(deleteScript(ipt, ipt.rules[4])).toBe("iptables -D INPUT 5");
    const nft = parseFirewall(probe({ nft: NFT }));
    expect(deleteScript(nft, nft.rules[3])).toBe("nft delete rule inet filter input handle 8");
    expect(() => deleteScript(ipt, { ...ipt.rules[0], chain: "x; id" })).toThrow();
  });
  it("allows ssh before it turns ufw on", () => {
    expect(enableUfwScript(22, "1.2.3.4", false)).toMatch(/^ufw allow from '1\.2\.3\.4' to any port 22 proto tcp .* && ufw --force enable$/);
    expect(enableUfwScript(2222, null, false)).toBe("ufw allow 2222/tcp && ufw --force enable");
    expect(enableUfwScript(22, "1.2.3.4", true)).toBe("ufw --force enable");
  });
});
