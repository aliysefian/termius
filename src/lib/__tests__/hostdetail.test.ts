import { describe, expect, it } from "vitest";
import {
  DETAIL_SCRIPT,
  cpuBetween,
  formatBytes,
  formatRate,
  hexToIpv4,
  hexToIpv6,
  isLoopback,
  killScript,
  mergeInterfaces,
  parseDetailOutput,
  parseIfconfig,
  parseIpAddr,
  parseIpLink,
  parseKillOutput,
  parseLsof,
  parseMeminfo,
  parseNetDev,
  parseNetstat,
  parseNetstatIbn,
  parsePorts,
  parseProcNet,
  parseProcSample,
  parsePsBsd,
  parseSections,
  parseSockstat,
  parseSs,
  prefixFromMask,
  processesBetween,
  ratesBetween,
  sortProcesses,
  splitHostPort,
  totalRate,
  type ProcessInfo,
} from "../hostdetail";
import alpine from "./hostdetail/alpine.txt?raw";
import darwin from "./hostdetail/darwin.txt?raw";
import debianFull from "./hostdetail/debian_full.txt?raw";
import debianMinimal from "./hostdetail/debian_minimal.txt?raw";
import debianNettools from "./hostdetail/debian_nettools.txt?raw";
import freebsd from "./hostdetail/freebsd.txt?raw";

const addr = (a: string) => (a.includes(":") ? `[${a}]` : a);
const ports = (d: ReturnType<typeof parseDetailOutput>) => d.ports.items.map((p) => `${p.proto} ${addr(p.address)}:${p.port} ${p.process ?? "-"}${p.pid !== null ? `(${p.pid})` : ""}`);

describe("sections", () => {
  it("splits on @@ markers and ignores what comes before the first", () => {
    const s = parseSections("# a comment\n@@A\none\ntwo\n@@B\n\n@@C\r\nx\r\n");
    expect([...s.keys()]).toEqual(["A", "B", "C"]);
    expect(s.get("A")).toEqual(["one", "two"]);
    expect(s.get("B")).toEqual([""]);
    expect(s.get("C")).toEqual(["x", ""]);
  });
});

describe("recorded: Alpine (BusyBox)", () => {
  const d = parseDetailOutput(alpine);
  it("reads the system", () => {
    expect(d.os).toBe("Linux");
    expect(d.cpuPct).toBeGreaterThan(10);
    expect(d.memPct).toBeGreaterThan(0);
    expect(d.load).toMatch(/^\d+\.\d+ \d+\.\d+ \d+\.\d+$/);
    expect(d.diskPct).toBeGreaterThan(0);
    expect(d.uptimeSecs).toBeGreaterThan(1000);
  });
  it("finds the busy loop and nginx by name and user", () => {
    const busy = sortProcesses(d.processes, "cpu", "desc")[0];
    expect(busy.name).toBe("sh");
    expect(busy.cpuPct).toBeGreaterThan(80);
    expect(d.processes.filter((p) => p.name === "nginx" && p.user === "nginx").length).toBeGreaterThanOrEqual(3);
  });
  it("reads BusyBox netstat: UDP rows have no state, IPv6 wildcards are :::port, an owner of - means unknown", () => {
    expect(ports(d)).toEqual(["udp [::]:5353 nc(41)", "tcp 0.0.0.0:8080 -"]);
    expect(d.ports.source).toBe("netstat");
    expect(d.ports.ownersMissing).toBe(true);
  });
  it("reads BusyBox ip, including a peer suffix on the name", () => {
    const eth = d.interfaces.find((i) => i.name === "eth0")!;
    expect(eth).toMatchObject({ state: "up", mtu: 1500 });
    expect(eth.mac).toMatch(/^[0-9a-f:]{17}$/);
    expect(eth.addresses).toEqual([{ family: 4, address: "172.17.0.3", prefix: 16 }]);
    expect(d.interfaces.map((i) => i.name)).toEqual(["lo", "eth0"]);
    expect(d.unavailable).toEqual([]);
  });
  it("computes throughput per interface", () => {
    expect(d.rates.map((r) => r.name)).toEqual(["eth0", "lo"]);
    expect(d.rates[0].rxBytes).toBeGreaterThan(0);
  });
});

describe("recorded: Debian with nothing installed (mawk, no ps, ss, ip, netstat)", () => {
  const d = parseDetailOutput(debianMinimal);
  it("still lists processes, from /proc alone", () => {
    expect(d.processes.length).toBeGreaterThan(3);
    expect(sortProcesses(d.processes, "cpu", "desc")[0].cpuPct).toBeGreaterThan(50);
    expect(d.processes.every((p) => p.user && p.name)).toBe(true);
  });
  it("reads real throughput from /proc/net/dev", () => {
    const eth = d.rates.find((r) => r.name === "eth0")!;
    expect(eth.rxBps).toBeGreaterThan(100_000);
    expect(totalRate(d.rates).rxBps).toBe(eth.rxBps);
  });
  it("falls back to /proc/net for listeners, with no owners, and says so", () => {
    expect(d.ports.source).toBe("proc");
    expect(ports(d)).toEqual(["udp 0.0.0.0:5353 -", "tcp 0.0.0.0:8000 -", "tcp [::]:8001 -"]);
    expect(d.ports.ownersMissing).toBe(true);
    expect(d.unavailable.find((u) => u.panel === "ports")?.reason).toMatch(/iproute2 or net-tools/);
  });
  it("falls back to /sys for interfaces, without addresses, and says so", () => {
    expect(d.interfaces.map((i) => [i.name, i.state, i.addresses.length])).toEqual([["eth0", "up", 0], ["lo", "up", 0]]);
    expect(d.interfaces[0].mac).toMatch(/^[0-9a-f:]{17}$/);
    expect(d.unavailable.find((u) => u.panel === "interfaces")?.reason).toMatch(/neither ip nor ifconfig/);
  });
});

describe("recorded: Debian with iproute2 (ss, ip)", () => {
  const d = parseDetailOutput(debianFull);
  it("reads ss with owners, once each, IPv6 included", () => {
    expect(d.ports.source).toBe("ss");
    expect(ports(d)).toEqual(["udp 0.0.0.0:5353 python3(1)", "tcp 0.0.0.0:8000 python3(1)", "tcp [::]:8001 python3(1)", "tcp 127.0.0.1:8002 python3(1)"]);
    expect(d.ports.ownersMissing).toBe(false);
    expect(d.ports.items.find((p) => p.port === 8001)!.ipv6).toBe(true);
  });
  it("reads ip addresses and prefixes", () => {
    expect(d.interfaces.find((i) => i.name === "lo")!.addresses).toEqual([
      { family: 4, address: "127.0.0.1", prefix: 8 },
      { family: 6, address: "::1", prefix: 128 },
    ]);
    expect(d.interfaces.find((i) => i.name === "eth0")!.addresses[0]).toEqual({ family: 4, address: "172.17.0.5", prefix: 16 });
  });
  it("measures the download that ran during the sample", () => {
    expect(d.rates.find((r) => r.name === "eth0")!.rxBps).toBeGreaterThan(500_000);
  });
});

describe("recorded: Debian with net-tools only (netstat, ifconfig)", () => {
  const d = parseDetailOutput(debianNettools);
  it("reads net-tools netstat, where tcp6 rows have an owner", () => {
    expect(d.ports.source).toBe("netstat");
    expect(ports(d)).toEqual(["udp 0.0.0.0:5353 python3(1)", "tcp 0.0.0.0:8000 python3(1)", "tcp [::]:8001 python3(1)", "tcp 127.0.0.1:8002 python3(1)"]);
  });
  it("reads ifconfig, whose netmask is dotted", () => {
    const eth = d.interfaces.find((i) => i.name === "eth0")!;
    expect(eth).toMatchObject({ state: "up", mtu: 1500 });
    expect(eth.addresses).toEqual([{ family: 4, address: "172.17.0.6", prefix: 16 }]);
    const lo = d.interfaces.find((i) => i.name === "lo")!;
    expect(lo.addresses).toEqual([
      { family: 4, address: "127.0.0.1", prefix: 8 },
      { family: 6, address: "::1", prefix: 128 },
    ]);
    expect(lo.state).toBe("up");
  });
});

describe("hand-written: macOS", () => {
  const d = parseDetailOutput(darwin);
  it("says what it can't show instead of guessing", () => {
    expect(d.os).toBe("Darwin");
    expect(d.cpuPct).toBeNull();
    expect(d.memPct).toBeNull();
    expect(d.uptimeSecs).toBeNull();
    expect(d.unavailable.map((u) => u.panel)).toEqual(["cpu"]);
    expect(d.load).toBe("1.84 1.62 1.50");
    expect(d.diskPct).toBe(5);
  });
  it("reads netstat -ibn from the right, skipping the address rows and a down interface's *", () => {
    expect(d.rates.map((r) => [r.name, r.rxBps, r.txBps])).toEqual([["en0", 600000, 100000], ["utun0", 0, 0], ["lo0", 1000, 1000]]);
  });
  it("reads ps with a command path that has spaces", () => {
    const chrome = d.processes.find((p) => p.pid === 1201)!;
    expect(chrome).toMatchObject({ name: "Google Chrome", user: "alice", cpuPct: 45.2, memKb: 812344 });
    expect(chrome.nameRaw).toBe("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
  });
  it("reads lsof: IPv4 and IPv6 wildcards apart, escaped spaces, no connected UDP sockets", () => {
    expect(ports(d).sort()).toEqual(
      [
        "udp [::]:5353 mDNSRespo(181)",
        "udp 0.0.0.0:5353 mDNSRespo(181)",
        "tcp [::1]:5432 postgres(877)",
        "tcp 127.0.0.1:5432 postgres(877)",
        "tcp 127.0.0.1:9222 Google Chrome(1201)",
        "tcp [::]:49152 rapportd(412)",
        "tcp 0.0.0.0:49152 rapportd(412)",
      ].sort(),
    );
    expect(d.ports.ownersMissing).toBe(false);
  });
  it("reads macOS ifconfig: hex netmasks, scoped IPv6, and status", () => {
    const by = Object.fromEntries(d.interfaces.map((i) => [i.name, i]));
    expect(by.en0).toMatchObject({ state: "up", mac: "aa:bb:cc:dd:ee:ff", mtu: 1500 });
    expect(by.en0.addresses).toContainEqual({ family: 4, address: "192.168.1.23", prefix: 24 });
    expect(by.en0.addresses).toContainEqual({ family: 6, address: "fe80::1c2d:3e4f:5a6b:7c8d", prefix: 64 });
    expect(by.en1.state).toBe("down");
    expect(by.lo0.state).toBe("up");
  });
});

describe("hand-written: FreeBSD", () => {
  const d = parseDetailOutput(freebsd);
  it("reads netstat -ibn with its extra Idrop column", () => {
    expect(d.rates.map((r) => [r.name, r.rxBps, r.txBps])).toEqual([["em0", 1000000, 100000], ["lo0", 0, 0]]);
  });
  it("reads sockstat, giving an IPv6 wildcard its own address", () => {
    expect(ports(d).sort()).toEqual(["tcp [::]:22 sshd(803)", "tcp 0.0.0.0:22 sshd(803)", "tcp 0.0.0.0:80 nginx(1030)", "udp [::]:123 ntpd(777)", "udp 0.0.0.0:123 ntpd(777)"].sort());
  });
  it("reads processes with spaces in the command", () => {
    expect(d.processes.find((p) => p.pid === 1031)!.name).toBe("nginx: master process /usr/local/sbin/nginx");
  });
  it("reads ifconfig with `metric` in the header", () => {
    expect(d.interfaces.find((i) => i.name === "em0")).toMatchObject({ state: "up", mtu: 1500, mac: "00:0c:29:aa:bb:cc" });
  });
});

describe("CPU and memory", () => {
  it("is the share of the interval spent busy, with iowait as idle", () => {
    expect(cpuBetween("cpu  100 0 100 800 0 0 0 0", "cpu  200 0 200 1600 0 0 0 0")).toBe(20);
    expect(cpuBetween("cpu  100 0 100 700 100 0 0 0", "cpu  100 0 100 800 200 0 0 0")).toBe(0);
    expect(cpuBetween("cpu  1 0 1 1 0", "cpu  1 0 1 1 0")).toBeNull();
    expect(cpuBetween(undefined, "cpu 1 1 1 1 1")).toBeNull();
    expect(cpuBetween("garbage", "garbage")).toBeNull();
  });
  it("uses MemAvailable, or the old stand-in without it", () => {
    expect(parseMeminfo(["MemTotal: 1000 kB", "MemAvailable: 250 kB", "SwapTotal: 100 kB", "SwapFree: 50 kB"])).toEqual({ totalKb: 1000, memPct: 75, swapPct: 50 });
    expect(parseMeminfo(["MemTotal: 1000 kB", "MemFree: 100 kB", "Buffers: 100 kB", "Cached: 300 kB"]).memPct).toBe(50);
    expect(parseMeminfo([]).memPct).toBeNull();
    expect(parseMeminfo(["MemTotal: 1000 kB", "MemAvailable: 1000 kB"]).swapPct).toBeNull();
  });
});

describe("network rates", () => {
  const a = parseNetDev(["Inter-|   Receive", " face |bytes    packets", "  eth0: 1000 1 0 0 0 0 0 0 5000 1 0 0 0 0 0 0", "    lo: 10 1 0 0 0 0 0 0 10 1 0 0 0 0 0 0"]);
  const b = parseNetDev(["Inter-|   Receive", " face |bytes    packets", "  eth0: 4000 1 0 0 0 0 0 0 5500 1 0 0 0 0 0 0", "    lo: 20 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0", "  new0: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0"]);
  it("divides the change by the real time between samples", () => {
    expect(ratesBetween(a, b, 2).map((r) => [r.name, r.rxBps, r.txBps])).toEqual([["eth0", 1500, 250], ["lo", 5, 5]]);
    expect(ratesBetween(a, b, 0.5)[0].rxBps).toBe(6000);
  });
  it("skips an interface that wasn't there before, and never goes negative when a counter resets", () => {
    expect(ratesBetween(a, b, 1).some((r) => r.name === "new0")).toBe(false);
    const reset = parseNetDev(["  eth0: 10 1 0 0 0 0 0 0 10 1 0 0 0 0 0 0"]);
    expect(ratesBetween(a, reset, 1)[0]).toMatchObject({ rxBps: 0, txBps: 0 });
    expect(ratesBetween(a, b, 0)[0].rxBps).toBe(3000);
  });
  it("totals everything but loopback", () => {
    expect(totalRate(ratesBetween(a, b, 1))).toEqual({ rxBps: 3000, txBps: 500 });
    for (const lo of ["lo", "lo0", "lo1"]) expect(isLoopback(lo)).toBe(true);
    for (const n of ["eth0", "local0", "docker0", "wlo1"]) expect(isLoopback(n)).toBe(false);
  });
  it("reads netstat -ibn rows from the right and ignores the rest", () => {
    const c = parseNetstatIbn(["Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll", "en0 1500 <Link#4> aa:bb:cc:dd:ee:ff 10 0 2000 5 0 3000 0", "en0 1500 192.168.1 192.168.1.2 10 - 2000 5 - 3000 -", "lo0 16384 <Link#1> 7 0 700 7 0 700 0", "bad"]);
    expect([...c]).toEqual([["en0", { rx: 2000, tx: 3000 }], ["lo0", { rx: 700, tx: 700 }]]);
  });
  it("formats sizes and rates", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1500)).toBe("1.5 kB");
    expect(formatBytes(3_034_173)).toBe("3.0 MB");
    expect(formatBytes(250_000_000)).toBe("250 MB");
    expect(formatBytes(2e12)).toBe("2.0 TB");
    expect(formatRate(126)).toBe("126 B/s");
  });
});

describe("processes", () => {
  const mk = (rows: string[]) => parseProcSample(rows);
  it("computes CPU as a share of one core over the real interval", () => {
    const a = mk(["10\t100\t1000\troot\tnginx", "20\t0\t500\talice\tsleep"]);
    const b = mk(["10\t250\t1000\troot\tnginx", "20\t0\t500\talice\tsleep", "30\t5\t10\tbob\tnew"]);
    const r = processesBetween(a, b, 1, 100, 100_000);
    expect(r.find((p) => p.pid === 10)).toMatchObject({ cpuPct: 150, memKb: 1000, memPct: 1 });
    expect(r.find((p) => p.pid === 20)!.cpuPct).toBe(0);
    expect(r.find((p) => p.pid === 30)!.cpuPct).toBe(0);
    expect(processesBetween(a, b, 2, 100, null)[0].cpuPct).toBe(75);
    expect(processesBetween(a, b, 1, 100, null)[0].memPct).toBeNull();
  });
  it("treats a pid whose name changed as a different process", () => {
    const a = mk(["10\t100\t1\troot\told"]);
    const b = mk(["10\t5000\t1\troot\tnew"]);
    expect(processesBetween(a, b, 1, 100, null)[0].cpuPct).toBe(0);
  });
  it("keeps names with spaces and parentheses, and skips broken lines", () => {
    const m = mk(["5\t1\t2\talice\tnode (vitest 2)", "garbage", "x\ty", "6\t1\t2\talice"]);
    expect([...m.keys()]).toEqual([5]);
    expect(m.get(5)!.name).toBe("node (vitest 2)");
  });
  it("reads ps output with a command that is a path", () => {
    const p = parsePsBsd(["  1 root 0.0 100 /sbin/launchd", " 20 bob 2.5 2048 /Applications/My App.app/Contents/MacOS/My App", "junk"]);
    expect(p.map((x) => x.name)).toEqual(["launchd", "My App"]);
    expect(p[1].cpuPct).toBe(2.5);
  });
  describe("sorting", () => {
    const list: ProcessInfo[] = [
      { pid: 3, user: "bob", name: "zsh", nameRaw: "zsh", cpuPct: 0, memKb: 10, memPct: null },
      { pid: 1, user: "root", name: "Init", nameRaw: "Init", cpuPct: 5, memKb: 500, memPct: null },
      { pid: 2, user: "alice", name: "node", nameRaw: "node", cpuPct: 5, memKb: 900, memPct: null },
      { pid: 4, user: "bob", name: "idle", nameRaw: "idle", cpuPct: 0, memKb: 10, memPct: null },
    ];
    const ids = (k: Parameters<typeof sortProcesses>[1], d: "asc" | "desc") => sortProcesses(list, k, d).map((p) => p.pid);
    it("by CPU, ties broken by memory", () => expect(ids("cpu", "desc")).toEqual([2, 1, 3, 4]));
    it("by memory", () => expect(ids("mem", "desc")).toEqual([2, 1, 3, 4]));
    it("by pid, either way", () => {
      expect(ids("pid", "asc")).toEqual([1, 2, 3, 4]);
      expect(ids("pid", "desc")).toEqual([4, 3, 2, 1]);
    });
    it("by name without regard to case, and by user", () => {
      expect(ids("name", "asc")).toEqual([4, 1, 2, 3]);
      expect(ids("user", "asc")).toEqual([2, 3, 4, 1]);
    });
    it("leaves its input alone", () => {
      ids("pid", "desc");
      expect(list.map((p) => p.pid)).toEqual([3, 1, 2, 4]);
    });
  });
});

describe("listening ports, one tool at a time", () => {
  it("splits addresses from ports", () => {
    expect(splitHostPort("127.0.0.1:5432")).toEqual({ address: "127.0.0.1", port: 5432 });
    expect(splitHostPort("[::1]:631")).toEqual({ address: "::1", port: 631 });
    expect(splitHostPort(":::5353")).toEqual({ address: "::", port: 5353 });
    expect(splitHostPort("[::]:22")).toEqual({ address: "::", port: 22 });
    expect(splitHostPort("*:80")).toEqual({ address: "*", port: 80 });
    expect(splitHostPort("*:80", 4)).toEqual({ address: "0.0.0.0", port: 80 });
    expect(splitHostPort("*:80", 6)).toEqual({ address: "::", port: 80 });
    expect(splitHostPort(":80")).toEqual({ address: "0.0.0.0", port: 80 });
    expect(splitHostPort(":80", 6)).toEqual({ address: "::", port: 80 });
    expect(splitHostPort("0.0.0.0%lo:53")).toEqual({ address: "0.0.0.0", port: 53 });
    expect(splitHostPort("fe80::1%eth0:123")).toEqual({ address: "fe80::1", port: 123 });
    expect(splitHostPort("nonsense")).toBeNull();
    expect(splitHostPort("1.2.3.4:99999")).toBeNull();
    expect(splitHostPort("1.2.3.4:abc")).toBeNull();
  });
  it("reads ss: several owners take the first, no owner column is fine, the glued header is skipped", () => {
    const rows = parseSs([
      "Netid State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess",
      'tcp   LISTEN 0 128 *:22 *:* users:(("sshd",pid=900,fd=3),("sshd",pid=901,fd=3))',
      "udp   UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:*",
      'tcp   LISTEN 0 4096 [::1]:5432 [::]:* users:(("pg worker",pid=7,fd=1))',
    ]);
    expect(rows.map((p) => [p.proto, p.address, p.port, p.pid, p.process])).toEqual([
      ["tcp", "*", 22, 900, "sshd"],
      ["udp", "127.0.0.53", 53, null, null],
      ["tcp", "::1", 5432, 7, "pg worker"],
    ]);
    expect(rows[2].ipv6).toBe(true);
  });
  it("reads netstat: only LISTEN tcp, a process title with spaces", () => {
    const rows = parseNetstat([
      "Active Internet connections (only servers)",
      "Proto Recv-Q Send-Q Local Address Foreign Address State PID/Program name",
      "tcp 0 0 0.0.0.0:22 0.0.0.0:* LISTEN 900/sshd",
      "tcp 0 0 10.0.0.2:22 10.0.0.9:5000 ESTABLISHED 902/sshd",
      "tcp6 0 0 :::80 :::* LISTEN 12/nginx: master",
      "udp 0 0 0.0.0.0:68 0.0.0.0:* 456/dhclient",
      "udp6 0 0 :::123 :::* -",
    ]);
    expect(rows.map((p) => [p.proto, p.address, p.port, p.pid, p.process, p.ipv6])).toEqual([
      ["tcp", "0.0.0.0", 22, 900, "sshd", false],
      ["tcp", "::", 80, 12, "nginx: master", true],
      ["udp", "0.0.0.0", 68, 456, "dhclient", false],
      ["udp", "::", 123, null, null, true],
    ]);
  });
  it("converts /proc's little-endian hex addresses", () => {
    expect(hexToIpv4("0100007F")).toBe("127.0.0.1");
    expect(hexToIpv4("0F02000A")).toBe("10.0.2.15");
    expect(hexToIpv6("00000000000000000000000000000000")).toBe("::");
    expect(hexToIpv6("00000000000000000000000001000000")).toBe("::1");
    expect(hexToIpv6("B80D0120000000000000000001000000")).toBe("2001:db8::1");
    expect(hexToIpv6("0000000000000000FFFF00000100007F")).toBe("::ffff:7f00:1");
    expect(hexToIpv6("short")).toBe("short");
  });
  it("reads /proc/net, taking listening TCP and unconnected UDP only", () => {
    const rows = parseProcNet([
      "file tcp",
      "  sl  local_address rem_address   st tx_queue rx_queue",
      "   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 111 1",
      "   1: 0100007F:C350 0100007F:1538 01 00000000:00000000 00:00000000 00000000     0        0 112 1",
      "file udp",
      " 6607: 00000000:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 113 2",
      " 6608: 0100007F:E000 0100007F:0035 01 00000000:00000000 00:00000000 00000000     0        0 114 2",
      "file tcp6",
      "   0: 00000000000000000000000001000000:0277 00000000000000000000000000000000:0000 0A 0 0 0 0 0 0 0 115 1",
    ]);
    expect(rows.map((p) => [p.proto, p.address, p.port])).toEqual([["tcp", "127.0.0.1", 5432], ["udp", "0.0.0.0", 5353], ["tcp", "::1", 631]]);
    expect(rows.every((p) => p.pid === null)).toBe(true);
  });
  it("reads lsof and sockstat, and refuses a report with no tool", () => {
    expect(parseLsof(["COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME", "node 5 al 20u IPv6 0xa 0t0 TCP *:3000 (LISTEN)", "node 5 al 21u IPv4 0xb 0t0 TCP 1.2.3.4:5->6.7.8.9:10 (ESTABLISHED)", "a\\x20b 6 al 4u IPv4 0xc 0t0 UDP *:9"]).map((p) => [p.process, p.address, p.port])).toEqual([["node", "::", 3000], ["a b", "0.0.0.0", 9]]);
    expect(parseSockstat(["USER COMMAND PID FD PROTO LOCAL ADDRESS FOREIGN ADDRESS", "root sshd 803 3 tcp6 *:22 *:*", "root sshd x 3 tcp6 *:23 *:*", "root dhclient 5 6 udp4 *:68 *:*"]).map((p) => [p.address, p.port, p.proto])).toEqual([["::", 22, "tcp"], ["0.0.0.0", 68, "udp"]]);
    expect(parsePorts(["tool none"]).source).toBe("none");
    expect(parsePorts([]).source).toBe("none");
    expect(parsePorts(["tool wat", "x"]).items).toEqual([]);
  });
  it("lists one of each, in port order", () => {
    const r = parsePorts(["tool ss", 'tcp LISTEN 0 1 0.0.0.0:90 0.0.0.0:* users:(("a",pid=1,fd=1))', 'tcp LISTEN 0 1 0.0.0.0:90 0.0.0.0:* users:(("a",pid=1,fd=2))', 'tcp LISTEN 0 1 0.0.0.0:22 0.0.0.0:* users:(("s",pid=2,fd=1))']);
    expect(r.items.map((p) => p.port)).toEqual([22, 90]);
  });
});

describe("interfaces, one tool at a time", () => {
  it("reads masks in both notations", () => {
    expect(prefixFromMask("255.255.0.0")).toBe(16);
    expect(prefixFromMask("0xffffff00")).toBe(24);
    expect(prefixFromMask("255.255.255.255")).toBe(32);
    expect(prefixFromMask("0.0.0.0")).toBe(0);
    expect(prefixFromMask("255.0.255.0")).toBeNull();
    expect(prefixFromMask("nope")).toBeNull();
  });
  it("reads ip link: state from the word, else the flags", () => {
    const l = parseIpLink([
      "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT\\    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00",
      "3: wlan0: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN mode DEFAULT\\    link/ether aa:bb:cc:dd:ee:ff brd ff:ff:ff:ff:ff:ff",
      "4: veth1@if3: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 state UP\\    link/ether 02:00:00:00:00:01 brd ff:ff:ff:ff:ff:ff",
      "5: tun0: <POINTOPOINT,UP,LOWER_UP> mtu 1400 state UNKNOWN\\    link/none",
    ]);
    expect(l.map((i) => [i.name, i.state, i.mac, i.mtu])).toEqual([["lo", "up", null, 65536], ["wlan0", "down", "aa:bb:cc:dd:ee:ff", 1500], ["veth1", "up", "02:00:00:00:00:01", 1500], ["tun0", "up", null, 1400]]);
  });
  it("reads ip addr, grouping by interface and trimming the peer suffix", () => {
    const m = parseIpAddr(["2: eth0    inet 10.0.0.5/24 brd 10.0.0.255 scope global eth0\\       valid_lft forever", "2: eth0    inet6 fe80::1/64 scope link \\       valid_lft forever", "4: veth1@if3    inet 169.254.1.1 scope link veth1"]);
    expect(m.get("eth0")).toEqual([{ family: 4, address: "10.0.0.5", prefix: 24 }, { family: 6, address: "fe80::1", prefix: 64 }]);
    expect(m.get("veth1")).toEqual([{ family: 4, address: "169.254.1.1", prefix: null }]);
  });
  it("reads old-style net-tools ifconfig loosely", () => {
    const i = parseIfconfig([
      "eth0      Link encap:Ethernet  HWaddr 52:54:00:12:34:56",
      "          inet addr:10.0.2.15  Bcast:10.0.2.255  Mask:255.255.255.0",
      "          inet6 addr: fe80::5054:ff:fe12:3456/64 Scope:Link",
      "          UP BROADCAST RUNNING MULTICAST  MTU:1500  Metric:1",
      "",
      "lo        Link encap:Local Loopback",
      "          inet addr:127.0.0.1  Mask:255.0.0.0",
      "          UP LOOPBACK RUNNING  MTU:65536  Metric:1",
    ]);
    expect(i[0]).toMatchObject({ name: "eth0", mac: "52:54:00:12:34:56", mtu: 1500, state: "up" });
    expect(i[0].addresses).toEqual([{ family: 4, address: "10.0.2.15", prefix: 24 }, { family: 6, address: "fe80::5054:ff:fe12:3456", prefix: 64 }]);
    expect(i[1]).toMatchObject({ name: "lo", mtu: 65536, state: "up" });
  });
  it("prefers ip, then ifconfig, then /sys, filling gaps from the rest", () => {
    const sys = [{ name: "eth0", state: "up" as const, mac: "aa:aa:aa:aa:aa:aa", mtu: 1500, addresses: [] }];
    const ifc = [{ name: "eth0", state: "unknown" as const, mac: null, mtu: null, addresses: [{ family: 4 as const, address: "1.2.3.4", prefix: 8 }] }];
    expect(mergeInterfaces({ link: [], addr: new Map(), ifconfig: ifc, sys })[0]).toMatchObject({ state: "up", mac: "aa:aa:aa:aa:aa:aa", mtu: 1500, addresses: [{ address: "1.2.3.4" }] });
    expect(mergeInterfaces({ link: [], addr: new Map(), ifconfig: [], sys })[0].name).toBe("eth0");
    expect(mergeInterfaces({ link: [], addr: new Map(), ifconfig: [], sys: [] })).toEqual([]);
  });
});

describe("the whole output", () => {
  it("copes with nothing, and with a host that isn't Linux and has no tools", () => {
    const empty = parseDetailOutput("");
    expect(empty.os).toBe("unknown");
    expect(empty.processes).toEqual([]);
    expect(empty.unavailable.map((u) => u.panel).sort()).toEqual(["cpu", "interfaces", "network", "ports", "processes"].sort());
    const bsd = parseDetailOutput("@@OS\nOpenBSD\n@@PORTS\ntool none\n");
    expect(bsd.unavailable.find((u) => u.panel === "ports")?.reason).toMatch(/lsof nor sockstat/);
  });
  it("is the script that ships, not a copy", () => {
    expect(DETAIL_SCRIPT).toContain("@@PROCA");
    expect(DETAIL_SCRIPT).toContain("uname -s");
  });
});

describe("signalling a process", () => {
  it("checks the name first, then signals, on Linux", () => {
    const s = killScript("Linux", 1234, "nginx", "TERM");
    expect(s).toContain("/proc/1234/comm");
    expect(s).toContain("!= 'nginx'");
    expect(s).toContain("kill -TERM 1234");
    expect(killScript("Linux", 99, "x", "KILL")).toContain("kill -KILL 99");
  });
  it("uses ps on the BSDs and macOS", () => {
    expect(killScript("Darwin", 55, "/usr/sbin/cupsd", "TERM")).toContain("ps -p 55 -o comm=");
  });
  it("quotes any name, so it can't carry a command", () => {
    const s = killScript("Linux", 10, "x'; reboot; echo '", "TERM");
    expect(s).toContain("'x'\\''; reboot; echo '\\'''");
    expect(s.match(/reboot/g)).toHaveLength(1);
  });
  it("won't signal init, a negative pid, or anything that isn't a whole number", () => {
    for (const pid of [0, 1, -1, -1234, 1.5, NaN]) expect(() => killScript("Linux", pid, "x", "TERM")).toThrow();
  });
  it("reads each outcome", () => {
    expect(parseKillOutput("OK\n", "", 0)).toEqual({ kind: "ok" });
    expect(parseKillOutput("GONE\n", "", 0)).toEqual({ kind: "gone" });
    expect(parseKillOutput("CHANGED\n", "", 0)).toEqual({ kind: "changed" });
    expect(parseKillOutput("FAILED sh: kill: (1): Operation not permitted\n", "", 0)).toEqual({ kind: "failed", reason: "sh: kill: (1): Operation not permitted" });
    expect(parseKillOutput("", "boom", 1)).toEqual({ kind: "failed", reason: "boom" });
    expect(parseKillOutput("", "", 127)).toEqual({ kind: "failed", reason: "the command ended with status 127" });
    expect(parseKillOutput("FAILED", "", 0)).toEqual({ kind: "failed", reason: "the host refused" });
  });
});
