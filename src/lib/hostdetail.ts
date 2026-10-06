// Reads what hostdetail.sh prints: a host's network throughput, processes,
// listening ports and interfaces. The script only prints raw counters and
// each tool's own text; everything here is arithmetic and parsing, split into
// one small function per format so each can be tested on recorded output.
import SCRIPT from "./hostdetail.sh?raw";

// A checkout with Windows line endings would make the remote shell fail to parse it.
export const DETAIL_SCRIPT: string = SCRIPT.replace(/\r\n/g, "\n");

export interface InterfaceRate {
  name: string;
  /** Bytes per second received and sent over the sample. */
  rxBps: number;
  txBps: number;
  /** Counters at the end of the sample. */
  rxBytes: number;
  txBytes: number;
}

export interface ProcessInfo {
  pid: number;
  user: string;
  /** What is shown: the program's own name. */
  name: string;
  /** What the system called it, kept to check it is the same process before signalling. */
  nameRaw: string;
  /** Percent of one CPU core over the sample (like `top`, so it can pass 100). */
  cpuPct: number;
  memKb: number;
  memPct: number | null;
}

export interface ListeningPort {
  proto: "tcp" | "udp";
  address: string;
  port: number;
  pid: number | null;
  process: string | null;
  ipv6: boolean;
}

export type PortSource = "ss" | "netstat" | "proc" | "lsof" | "sockstat" | "none";

export interface PortsReport {
  source: PortSource;
  items: ListeningPort[];
  /** Some listeners have no owner shown (the tool can't tell, or it needs root). */
  ownersMissing: boolean;
}

export type LinkState = "up" | "down" | "unknown";

export interface NetInterface {
  name: string;
  state: LinkState;
  mac: string | null;
  mtu: number | null;
  addresses: { family: 4 | 6; address: string; prefix: number | null }[];
}

export interface HostDetail {
  os: string;
  uptimeSecs: number | null;
  /** Percent busy over the sample; null where the sample can't tell. */
  cpuPct: number | null;
  memPct: number | null;
  swapPct: number | null;
  load: string | null;
  diskPct: number | null;
  rates: InterfaceRate[];
  processes: ProcessInfo[];
  ports: PortsReport;
  interfaces: NetInterface[];
  /** Panels the host couldn't fill, and why, in plain words. */
  unavailable: { panel: "cpu" | "memory" | "network" | "processes" | "ports" | "interfaces"; reason: string }[];
}

// -- sections ---------------------------------------------------------------

/** The text of each `@@NAME` section, as lines. Anything before the first marker is ignored. */
export function parseSections(stdout: string): Map<string, string[]> {
  const out = new Map<string, string[]>();
  let cur: string[] | null = null;
  for (const raw of stdout.split("\n")) {
    const line = raw.replace(/\r$/, "");
    if (line.startsWith("@@")) {
      cur = [];
      out.set(line.slice(2).trim(), cur);
    } else if (cur) cur.push(line);
  }
  return out;
}

const num = (s: string | undefined): number | null => {
  if (s === undefined || s.trim() === "") return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
};

const nonEmpty = (lines: string[] | undefined) => (lines ?? []).filter((l) => l.trim() !== "");

// -- CPU, memory, disk, load -------------------------------------------------

/** Percent busy between two `cpu ...` lines of /proc/stat (iowait counts as idle). */
export function cpuBetween(a: string | undefined, b: string | undefined): number | null {
  const read = (line: string | undefined) => {
    const f = (line ?? "").trim().split(/\s+/);
    if (f[0] !== "cpu" || f.length < 5) return null;
    const v = f.slice(1).map((x) => Number(x) || 0);
    const idle = v[3] + (v[4] ?? 0);
    // guest time is already inside user/nice, so it isn't added again
    const total = v.slice(0, 8).reduce((s, x) => s + x, 0);
    return { total, idle };
  };
  const x = read(a);
  const y = read(b);
  if (!x || !y) return null;
  const dt = y.total - x.total;
  if (dt <= 0) return null;
  const busy = dt - (y.idle - x.idle);
  return Math.max(0, Math.min(100, Math.round((100 * busy) / dt)));
}

export function parseMeminfo(lines: string[]): { totalKb: number | null; memPct: number | null; swapPct: number | null } {
  const m = new Map<string, number>();
  for (const l of lines) {
    const mm = /^(\w+):\s+(\d+)/.exec(l);
    if (mm) m.set(mm[1], Number(mm[2]));
  }
  const total = m.get("MemTotal");
  if (!total) return { totalKb: null, memPct: null, swapPct: null };
  // Old kernels have no MemAvailable; free + buffers + cached is the usual stand-in.
  const avail = m.get("MemAvailable") ?? (m.get("MemFree") ?? 0) + (m.get("Buffers") ?? 0) + (m.get("Cached") ?? 0);
  const swapTotal = m.get("SwapTotal") ?? 0;
  return {
    totalKb: total,
    memPct: Math.max(0, Math.min(100, Math.round((100 * (total - avail)) / total))),
    swapPct: swapTotal > 0 ? Math.round((100 * (swapTotal - (m.get("SwapFree") ?? 0))) / swapTotal) : null,
  };
}

/** `df -P /` last line: the use percentage. */
export function parseDfPct(lines: string[]): number | null {
  const l = nonEmpty(lines).pop();
  const m = l && /\s(\d+)%\s/.exec(l + " ");
  return m ? Number(m[1]) : null;
}

// -- network counters ---------------------------------------------------------------

export type Counters = Map<string, { rx: number; tx: number }>;

/** /proc/net/dev: bytes received and sent per interface. */
export function parseNetDev(lines: string[]): Counters {
  const out: Counters = new Map();
  for (const l of lines) {
    const i = l.indexOf(":");
    if (i < 0 || l.includes("|")) continue;
    const f = l.slice(i + 1).trim().split(/\s+/);
    const rx = num(f[0]);
    const tx = num(f[8]);
    if (rx !== null && tx !== null) out.set(l.slice(0, i).trim(), { rx, tx });
  }
  return out;
}

/**
 * `netstat -ibn` on macOS and the BSDs: the `<Link#n>` row of each interface.
 * The byte counts are read from the right, because the Address column can be
 * empty and FreeBSD has an extra Idrop column that macOS doesn't.
 */
export function parseNetstatIbn(lines: string[]): Counters {
  const out: Counters = new Map();
  for (const l of lines) {
    const f = l.trim().split(/\s+/);
    if (f.length < 8 || !f[2]?.startsWith("<Link#")) continue;
    const rx = num(f[f.length - 5]);
    const tx = num(f[f.length - 2]);
    if (rx !== null && tx !== null) out.set(f[0].replace(/\*$/, ""), { rx, tx });
  }
  return out;
}

export function ratesBetween(a: Counters, b: Counters, seconds: number): InterfaceRate[] {
  const dt = seconds > 0 ? seconds : 1;
  const out: InterfaceRate[] = [];
  for (const [name, y] of b) {
    const x = a.get(name);
    if (!x) continue;
    // A counter that went backwards wrapped or was reset: show 0, not a huge negative.
    const rate = (from: number, to: number) => (to >= from ? Math.round((to - from) / dt) : 0);
    out.push({ name, rxBps: rate(x.rx, y.rx), txBps: rate(x.tx, y.tx), rxBytes: y.rx, txBytes: y.tx });
  }
  return out.sort((p, q) => Number(isLoopback(p.name)) - Number(isLoopback(q.name)) || p.name.localeCompare(q.name));
}

export const isLoopback = (name: string) => /^lo\d*$/.test(name);

/** Total throughput of everything that isn't loopback. */
export function totalRate(rates: InterfaceRate[]): { rxBps: number; txBps: number } {
  return rates.filter((r) => !isLoopback(r.name)).reduce((s, r) => ({ rxBps: s.rxBps + r.rxBps, txBps: s.txBps + r.txBps }), { rxBps: 0, txBps: 0 });
}

export function formatBytes(n: number): string {
  if (n < 1000) return `${Math.round(n)} B`;
  const units = ["kB", "MB", "GB", "TB"];
  let v = n / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export const formatRate = (bps: number) => `${formatBytes(bps)}/s`;

// -- processes ----------------------------------------------------------------------

interface ProcSample {
  ticks: number;
  rssKb: number;
  user: string;
  name: string;
}

/** The tab-separated lines of the Linux sampler: pid, ticks, resident kB, user, name. */
export function parseProcSample(lines: string[]): Map<number, ProcSample> {
  const out = new Map<number, ProcSample>();
  for (const l of lines) {
    const f = l.split("\t");
    const pid = num(f[0]);
    const ticks = num(f[1]);
    if (pid === null || ticks === null || f.length < 5) continue;
    out.set(pid, { ticks, rssKb: num(f[2]) ?? 0, user: f[3], name: f.slice(4).join("\t") });
  }
  return out;
}

/** CPU over the sample from two process samples. A pid whose name changed is a different process, so it is skipped. */
export function processesBetween(a: Map<number, ProcSample>, b: Map<number, ProcSample>, seconds: number, clkTck: number, memTotalKb: number | null): ProcessInfo[] {
  const dt = seconds > 0 ? seconds : 1;
  const hz = clkTck > 0 ? clkTck : 100;
  const out: ProcessInfo[] = [];
  for (const [pid, y] of b) {
    const x = a.get(pid);
    const dticks = x && x.name === y.name ? Math.max(0, y.ticks - x.ticks) : 0;
    out.push({
      pid,
      user: y.user,
      name: y.name,
      nameRaw: y.name,
      cpuPct: Math.round((1000 * dticks) / (hz * dt)) / 10,
      memKb: y.rssKb,
      memPct: memTotalKb ? Math.round((1000 * y.rssKb) / memTotalKb) / 10 : null,
    });
  }
  return out;
}

/** `ps -axo pid=,user=,pcpu=,rss=,comm=` on macOS and the BSDs (CPU is the system's own average). */
export function parsePsBsd(lines: string[], memTotalKb: number | null = null): ProcessInfo[] {
  const out: ProcessInfo[] = [];
  for (const l of lines) {
    const m = /^\s*(\d+)\s+(\S+)\s+([\d.]+)\s+(\d+)\s+(.+?)\s*$/.exec(l);
    if (!m) continue;
    const raw = m[5];
    // macOS gives a full path (which may contain spaces): the program is its last part. A process
    // title such as "nginx: master process /usr/local/sbin/nginx" is kept whole.
    const name = raw.startsWith("/") ? (raw.split("/").pop() ?? raw) : raw;
    out.push({ pid: Number(m[1]), user: m[2], name, nameRaw: raw, cpuPct: Number(m[3]), memKb: Number(m[4]), memPct: memTotalKb ? Math.round((1000 * Number(m[4])) / memTotalKb) / 10 : null });
  }
  return out;
}

export type ProcessSort = "cpu" | "mem" | "pid" | "name" | "user";

export function sortProcesses(list: ProcessInfo[], key: ProcessSort, dir: "asc" | "desc"): ProcessInfo[] {
  const sign = dir === "asc" ? 1 : -1;
  const val: Record<ProcessSort, (p: ProcessInfo) => number | string> = {
    cpu: (p) => p.cpuPct,
    mem: (p) => p.memKb,
    pid: (p) => p.pid,
    name: (p) => p.name.toLowerCase(),
    user: (p) => p.user.toLowerCase(),
  };
  const f = val[key];
  return [...list].sort((a, b) => {
    const x = f(a);
    const y = f(b);
    const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
    // Ties fall back to CPU then memory, so a list of idle processes still has a sensible order.
    return c * sign || b.cpuPct - a.cpuPct || b.memKb - a.memKb || a.pid - b.pid;
  });
}

// -- listening ports -----------------------------------------------------------------

/**
 * `host:port`, `[v6]:port`, `*:port`, `:::port`, `host%iface:port`: the last colon splits the port.
 * A bare `*` or an empty host means every address: `0.0.0.0` or `::` when the socket's family is
 * known, and `*` when it isn't (`ss` uses it for both families at once).
 */
export function splitHostPort(s: string, family?: 4 | 6): { address: string; port: number } | null {
  const i = s.lastIndexOf(":");
  if (i < 0) return null;
  const port = Number(s.slice(i + 1));
  if (!Number.isInteger(port) || port < 0 || port > 65535) return null;
  let address = s.slice(0, i).replace(/^\[|\]$/g, "").replace(/%.*$/, "");
  const any = family === 6 ? "::" : "0.0.0.0";
  if (address === "") address = any;
  else if (address === "*" && family) address = any;
  return { address, port };
}

const owner = (pid: number | null, name: string | null): Pick<ListeningPort, "pid" | "process"> => ({ pid, process: name });

/** `ss -lntup`. The header's last two columns run together, so rows are recognised by their first word. */
export function parseSs(lines: string[]): ListeningPort[] {
  const out: ListeningPort[] = [];
  for (const l of lines) {
    const f = l.trim().split(/\s+/);
    if (f[0] !== "tcp" && f[0] !== "udp") continue;
    const hp = splitHostPort(f[4] ?? "");
    if (!hp) continue;
    const who = /\(\("([^"]*)",pid=(\d+)/.exec(l);
    out.push({ proto: f[0], ...hp, ...owner(who ? Number(who[2]) : null, who ? who[1] : null), ipv6: hp.address.includes(":") });
  }
  return out;
}

/** `netstat -lntup` (net-tools and BusyBox). UDP rows have no State column; a `-` owner means none was shown. */
export function parseNetstat(lines: string[]): ListeningPort[] {
  const out: ListeningPort[] = [];
  for (const l of lines) {
    const f = l.trim().split(/\s+/);
    const m = /^(tcp|udp)(6?)$/.exec(f[0] ?? "");
    if (!m) continue;
    const isTcp = m[1] === "tcp";
    if (isTcp && f[5] !== "LISTEN") continue;
    const hp = splitHostPort(f[3] ?? "", m[2] === "6" ? 6 : 4);
    if (!hp) continue;
    const ownerText = f.slice(isTcp ? 6 : 5).join(" ");
    const who = /^(\d+)\/(.+)$/.exec(ownerText);
    out.push({ proto: m[1] as "tcp" | "udp", ...hp, ...owner(who ? Number(who[1]) : null, who ? who[2] : null), ipv6: m[2] === "6" || hp.address.includes(":") });
  }
  return out;
}

/** An IPv4 address from /proc's little-endian hex (`0100007F` is 127.0.0.1). */
export function hexToIpv4(hex: string): string {
  const b = hex.match(/../g) ?? [];
  return b.length === 4 ? b.reverse().map((x) => parseInt(x, 16)).join(".") : hex;
}

/** An IPv6 address from /proc's hex: four 32-bit words, each little-endian. */
export function hexToIpv6(hex: string): string {
  if (!/^[0-9a-fA-F]{32}$/.test(hex)) return hex;
  const bytes: number[] = [];
  for (let w = 0; w < 4; w++) {
    const word = (hex.slice(w * 8, w * 8 + 8).match(/../g) ?? []).map((x) => parseInt(x, 16));
    bytes.push(...word.reverse());
  }
  const groups: number[] = [];
  for (let i = 0; i < 16; i += 2) groups.push((bytes[i] << 8) | bytes[i + 1]);
  // Compress the longest run of zero groups (two or more) into "::".
  let best = { at: -1, len: 0 };
  for (let i = 0; i < 8; ) {
    if (groups[i] !== 0) {
      i++;
      continue;
    }
    let j = i;
    while (j < 8 && groups[j] === 0) j++;
    if (j - i > best.len) best = { at: i, len: j - i };
    i = j;
  }
  const hexs = groups.map((g) => g.toString(16));
  if (best.len < 2) return hexs.join(":");
  return `${hexs.slice(0, best.at).join(":")}::${hexs.slice(best.at + best.len).join(":")}`;
}

/** The /proc/net/{tcp,tcp6,udp,udp6} fallback: addresses and ports, no owner (that needs the socket's inode). */
export function parseProcNet(lines: string[]): ListeningPort[] {
  const out: ListeningPort[] = [];
  let file = "";
  for (const l of lines) {
    const mark = /^file (tcp6?|udp6?)$/.exec(l.trim());
    if (mark) {
      file = mark[1];
      continue;
    }
    const f = l.trim().split(/\s+/);
    // Rows look like "0: 0100007F:1F40 00000000:0000 0A ..."; the header has "sl" first.
    if (!file || !/^\d+:$/.test(f[0] ?? "")) continue;
    const [addr, port] = (f[1] ?? "").split(":");
    const remote = f[2] ?? "";
    const state = f[3];
    const tcp = file.startsWith("tcp");
    // Listening TCP is state 0A; a bound UDP socket with no peer is state 07.
    if (tcp ? state !== "0A" : state !== "07" || !/^0+:0+$/.test(remote)) continue;
    const portNum = parseInt(port ?? "", 16);
    if (!addr || Number.isNaN(portNum)) continue;
    const v6 = file.endsWith("6");
    out.push({ proto: tcp ? "tcp" : "udp", address: v6 ? hexToIpv6(addr) : hexToIpv4(addr), port: portNum, ...owner(null, null), ipv6: v6 });
  }
  return out;
}

/** lsof's names escape spaces as `\x20`. */
const unescapeLsof = (s: string) => s.replace(/\\x([0-9a-fA-F]{2})/g, (_, h) => String.fromCharCode(parseInt(h, 16)));

/** macOS `lsof -nP -iTCP -sTCP:LISTEN` and `-iUDP`. A UDP row with `->` is a connected socket, not a listener. */
export function parseLsof(lines: string[]): ListeningPort[] {
  const out: ListeningPort[] = [];
  for (const l of lines) {
    const m = /^(.+?)\s+(\d+)\s+(\S+)\s+\S+\s+IPv([46])\s+\S+\s+\S+\s+(TCP|UDP)\s+(\S+)(\s+\(LISTEN\))?\s*$/.exec(l);
    if (!m) continue;
    const proto = m[5] === "TCP" ? "tcp" : "udp";
    if (proto === "tcp" && !m[7]) continue;
    if (m[6].includes("->")) continue;
    const hp = splitHostPort(m[6], m[4] === "6" ? 6 : 4);
    if (!hp) continue;
    out.push({ proto, ...hp, ...owner(Number(m[2]), unescapeLsof(m[1])), ipv6: m[4] === "6" });
  }
  return out;
}

/** FreeBSD `sockstat -l`: USER COMMAND PID FD PROTO LOCAL FOREIGN. */
export function parseSockstat(lines: string[]): ListeningPort[] {
  const out: ListeningPort[] = [];
  for (const l of lines) {
    const f = l.trim().split(/\s+/);
    const m = /^(tcp|udp)([46])$/.exec(f[4] ?? "");
    if (!m || !/^\d+$/.test(f[2] ?? "")) continue;
    const hp = splitHostPort(f[5] ?? "", m[2] === "6" ? 6 : 4);
    if (!hp) continue;
    out.push({ proto: m[1] as "tcp" | "udp", ...hp, ...owner(Number(f[2]), f[1]), ipv6: m[2] === "6" });
  }
  return out;
}

export function parsePorts(lines: string[]): PortsReport {
  const tool = /^tool (\w+)/.exec(lines[0] ?? "")?.[1] as PortSource | undefined;
  const body = lines.slice(1);
  const parsers: Partial<Record<PortSource, (l: string[]) => ListeningPort[]>> = {
    ss: parseSs,
    netstat: parseNetstat,
    proc: parseProcNet,
    lsof: parseLsof,
    sockstat: parseSockstat,
  };
  const parse = tool && parsers[tool];
  if (!parse) return { source: "none", items: [], ownersMissing: false };
  // The same socket can show twice (IPv4 and IPv6 views): keep one of each.
  const seen = new Set<string>();
  const items = parse(body)
    .filter((p) => {
      const k = `${p.proto}|${p.address}|${p.port}|${p.pid ?? ""}`;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto) || a.address.localeCompare(b.address));
  return { source: tool, items, ownersMissing: items.some((p) => p.pid === null) };
}

// -- interfaces ----------------------------------------------------------------------------

function stateFrom(word: string | undefined, flags: string): LinkState {
  const w = (word ?? "").toUpperCase();
  if (w === "UP") return "up";
  if (w === "DOWN" || w === "LOWERLAYERDOWN" || w === "NOTPRESENT") return "down";
  // Loopback and tunnels report UNKNOWN while working: the flags say more.
  if (flags.includes("LOWER_UP") || (flags.split(",").includes("UP") && flags.split(",").includes("RUNNING"))) return "up";
  return "unknown";
}

/** `ip -o link show`. A peer suffix (`eth0@if284`) is not part of the name. */
export function parseIpLink(lines: string[]): NetInterface[] {
  const out: NetInterface[] = [];
  for (const l of lines) {
    const m = /^\d+:\s+([^:\s@]+)(?:@\S+)?:\s+<([^>]*)>\s+mtu\s+(\d+)(.*)$/.exec(l);
    if (!m) continue;
    const state = /\bstate\s+(\S+)/.exec(m[4])?.[1];
    const mac = /link\/\S+\s+([0-9a-f]{2}(?::[0-9a-f]{2}){5})/i.exec(m[4])?.[1] ?? null;
    out.push({ name: m[1], state: stateFrom(state, m[2]), mac: mac && mac !== "00:00:00:00:00:00" ? mac : null, mtu: Number(m[3]), addresses: [] });
  }
  return out;
}

/** `ip -o addr show`: one address per line. */
export function parseIpAddr(lines: string[]): Map<string, NetInterface["addresses"]> {
  const out = new Map<string, NetInterface["addresses"]>();
  for (const l of lines) {
    const m = /^\d+:\s+([^\s@]+)(?:@\S+)?\s+(inet6?)\s+([^\s/]+)(?:\/(\d+))?/.exec(l);
    if (!m) continue;
    const list = out.get(m[1]) ?? [];
    list.push({ family: m[2] === "inet6" ? 6 : 4, address: m[3], prefix: m[4] ? Number(m[4]) : null });
    out.set(m[1], list);
  }
  return out;
}

/** A netmask as a prefix length: `255.255.0.0` or `0xffffff00`. */
export function prefixFromMask(mask: string): number | null {
  let bits: number;
  if (/^0x[0-9a-f]{8}$/i.test(mask)) bits = parseInt(mask.slice(2), 16);
  else if (/^\d+\.\d+\.\d+\.\d+$/.test(mask)) bits = mask.split(".").reduce((s, p) => s * 256 + Number(p), 0);
  else return null;
  const bin = (bits >>> 0).toString(2).padStart(32, "0");
  if (!/^1*0*$/.test(bin)) return null;
  return bin.includes("0") ? bin.indexOf("0") : 32;
}

/** `ifconfig -a`: the BSD and macOS layout, current net-tools, and (loosely) old net-tools. */
export function parseIfconfig(lines: string[]): NetInterface[] {
  const out: NetInterface[] = [];
  let cur: NetInterface | null = null;
  let flags = "";
  const finish = () => {
    if (cur && cur.state === "unknown") cur.state = stateFrom(undefined, flags);
  };
  for (const l of lines) {
    const head = /^([^\s:]+):\s+flags=\d+<([^>]*)>(?:\s+metric\s+\d+)?\s+mtu\s+(\d+)/.exec(l);
    const old = !head && /^(\S+)\s+Link encap:(\S+)(?:\s+HWaddr\s+(\S+))?/.exec(l);
    if (head || old) {
      finish();
      flags = head ? head[2] : "";
      cur = { name: head ? head[1] : (old as RegExpExecArray)[1], state: "unknown", mac: old ? ((old as RegExpExecArray)[3] ?? null) : null, mtu: head ? Number(head[3]) : null, addresses: [] };
      out.push(cur);
      continue;
    }
    if (!cur) continue;
    const t = l.trim();
    let m: RegExpExecArray | null;
    if ((m = /^ether\s+(\S+)/.exec(t))) cur.mac = m[1];
    else if ((m = /^status:\s+(\w+)/.exec(t))) cur.state = m[1] === "active" ? "up" : "down";
    else if ((m = /^inet\s+(\S+)\s+netmask\s+(\S+)/.exec(t))) cur.addresses.push({ family: 4, address: m[1], prefix: prefixFromMask(m[2]) });
    else if ((m = /^inet addr:(\S+).*?Mask:(\S+)/.exec(t))) cur.addresses.push({ family: 4, address: m[1], prefix: prefixFromMask(m[2]) });
    else if ((m = /^inet6\s+(\S+?)(?:%\S+)?\s+prefixlen\s+(\d+)/.exec(t))) cur.addresses.push({ family: 6, address: m[1], prefix: Number(m[2]) });
    else if ((m = /^inet6 addr:\s*(\S+?)\/(\d+)/.exec(t))) cur.addresses.push({ family: 6, address: m[1], prefix: Number(m[2]) });
    else if ((m = /\bMTU:(\d+)/.exec(t))) {
      cur.mtu = Number(m[1]);
      flags = t.replace(/\s+MTU.*$/, "").split(/\s+/).join(",");
      if (/\bUP\b/.test(t) && /\bRUNNING\b/.test(t)) cur.state = "up";
    }
  }
  finish();
  return out;
}

/** The /sys/class/net fallback: `name state mac mtu`, any of the last three possibly empty. */
export function parseSysNet(lines: string[]): NetInterface[] {
  const out: NetInterface[] = [];
  for (const l of lines) {
    if (!l.trim()) continue;
    const f = l.split(" ");
    const st = (f[1] ?? "").toLowerCase();
    const mac = f[2] && f[2] !== "00:00:00:00:00:00" ? f[2] : null;
    out.push({ name: f[0], state: st === "up" ? "up" : st === "down" ? "down" : "unknown", mac, mtu: num(f[3]), addresses: [] });
  }
  return out;
}

/**
 * One list from whatever the host could tell: `ip` first, `ifconfig` next, the
 * /sys listing to fill what is missing (and as the only source on a bare host).
 */
export function mergeInterfaces(parts: { link: NetInterface[]; addr: Map<string, NetInterface["addresses"]>; ifconfig: NetInterface[]; sys: NetInterface[] }): NetInterface[] {
  const base = parts.link.length ? parts.link : parts.ifconfig.length ? parts.ifconfig : parts.sys;
  const sysBy = new Map(parts.sys.map((s) => [s.name, s]));
  return base.map((i) => {
    const s = sysBy.get(i.name);
    const state = i.state === "unknown" && s ? s.state : i.state;
    return {
      ...i,
      // A loopback interface reports "unknown" while working; if it is listed at all it is up.
      state: state === "unknown" && isLoopback(i.name) ? "up" : state,
      mac: i.mac ?? s?.mac ?? null,
      mtu: i.mtu ?? s?.mtu ?? null,
      addresses: i.addresses.length ? i.addresses : (parts.addr.get(i.name) ?? []),
    };
  });
}

// -- the whole output ----------------------------------------------------------------------

export function parseDetailOutput(stdout: string): HostDetail {
  const sec = parseSections(stdout);
  const os = (sec.get("OS")?.[0] ?? "").trim() || "unknown";
  const unavailable: HostDetail["unavailable"] = [];
  const first = (name: string) => nonEmpty(sec.get(name))[0];
  const linux = os === "Linux";

  const ta = num(first("TA"));
  const tb = num(first("TB"));
  const seconds = ta !== null && tb !== null && tb > ta ? tb - ta : 1;

  let cpuPct: number | null = null;
  let memPct: number | null = null;
  let swapPct: number | null = null;
  let memTotal: number | null = null;
  let rates: InterfaceRate[] = [];
  let processes: ProcessInfo[] = [];

  if (linux) {
    cpuPct = cpuBetween(first("CPUA"), first("CPUB"));
    const mem = parseMeminfo(sec.get("MEM") ?? []);
    memPct = mem.memPct;
    swapPct = mem.swapPct;
    memTotal = mem.totalKb;
    rates = ratesBetween(parseNetDev(sec.get("NETA") ?? []), parseNetDev(sec.get("NETB") ?? []), seconds);
    const clk = num(first("CLK")) ?? 100;
    const a = parseProcSample(sec.get("PROCA") ?? []);
    const b = parseProcSample(sec.get("PROCB") ?? []);
    processes = processesBetween(a, b, seconds, clk, memTotal);
  } else {
    unavailable.push({ panel: "cpu", reason: `CPU and memory use come from the summary on ${os}; the detail view doesn't sample them yet.` });
    rates = ratesBetween(parseNetstatIbn(sec.get("NETA") ?? []), parseNetstatIbn(sec.get("NETB") ?? []), seconds);
    processes = parsePsBsd(sec.get("PROCPS") ?? []);
  }
  if (rates.length === 0) unavailable.push({ panel: "network", reason: "This host's network counters couldn't be read." });
  if (processes.length === 0) unavailable.push({ panel: "processes", reason: "This host's process list couldn't be read." });

  const ports = parsePorts(sec.get("PORTS") ?? []);
  if (ports.source === "none") unavailable.push({ panel: "ports", reason: linux ? "Neither ss nor netstat is installed, and /proc/net couldn't be read." : `Neither lsof nor sockstat is installed on this ${os} host.` });
  else if (ports.source === "proc") unavailable.push({ panel: "ports", reason: "ss and netstat aren't installed, so the listeners are shown without the process that owns each (install iproute2 or net-tools)." });

  const interfaces = mergeInterfaces({
    link: parseIpLink(sec.get("IPLINK") ?? []),
    addr: parseIpAddr(sec.get("IPADDR") ?? []),
    ifconfig: parseIfconfig(sec.get("IFCONFIG") ?? []),
    sys: parseSysNet(sec.get("SYSNET") ?? []),
  });
  if (interfaces.length === 0) unavailable.push({ panel: "interfaces", reason: "No interface list could be read." });
  else if (interfaces.every((i) => i.addresses.length === 0)) unavailable.push({ panel: "interfaces", reason: "Addresses aren't shown: neither ip nor ifconfig is installed here." });

  return {
    os,
    uptimeSecs: linux && ta !== null ? Math.floor(ta) : null,
    cpuPct,
    memPct,
    swapPct,
    load: (nonEmpty(sec.get("LOAD"))[0] ?? "").trim().split(/\s+/).slice(0, 3).join(" ") || null,
    diskPct: parseDfPct(sec.get("DISK") ?? []),
    rates,
    processes,
    ports,
    interfaces,
    unavailable,
  };
}

// -- signalling a process -----------------------------------------------------------------

const sq = (s: string) => `'${s.replace(/'/g, "'\\''")}'`;

/**
 * The script that signals one process, after checking the pid still belongs to
 * the process that was on screen (pids are reused). It prints OK, CHANGED
 * (a different process now has that pid) or FAILED with the reason.
 */
export function killScript(os: string, pid: number, nameRaw: string, signal: "TERM" | "KILL"): string {
  if (!Number.isInteger(pid) || pid <= 1) throw new Error("refusing to signal that process id");
  const name = sq(nameRaw);
  const current = os === "Linux" ? `$(cat /proc/${pid}/comm 2>/dev/null)` : `$(ps -p ${pid} -o comm= 2>/dev/null)`;
  return [
    `cur="${current}"`,
    `if [ -z "$cur" ]; then echo GONE`,
    `elif [ "$cur" != ${name} ]; then echo CHANGED`,
    `elif err=$(kill -${signal} ${pid} 2>&1); then echo OK`,
    `else echo "FAILED $err"; fi`,
  ].join("; ");
}

export type KillOutcome = { kind: "ok" } | { kind: "gone" } | { kind: "changed" } | { kind: "failed"; reason: string };

export function parseKillOutput(stdout: string, stderr: string, code: number | null): KillOutcome {
  const line = stdout.trim().split("\n").pop()?.trim() ?? "";
  if (line === "OK") return { kind: "ok" };
  if (line === "GONE") return { kind: "gone" };
  if (line === "CHANGED") return { kind: "changed" };
  if (line.startsWith("FAILED")) return { kind: "failed", reason: line.slice(6).trim() || "the host refused" };
  return { kind: "failed", reason: stderr.trim() || `the command ended with status ${code ?? "?"}` };
}
