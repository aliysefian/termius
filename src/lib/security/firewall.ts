// Firewalls, read and changed over a plain exec channel: which one is running, its rules as one table, and the
// scripts that add or remove a rule. The parsers take what `ufw`, `iptables`, `nft` and `firewall-cmd` print, so
// they run (and are tested) without a host.
//
// Every address, port and name that reaches a script is checked first and passed as a quoted word.
import { shq } from "$lib/ops/quote";
import { isIP, isIPv6, isSource, portsInclude, sourceCovers } from "./net";

export type Backend = "ufw" | "firewalld" | "nftables" | "iptables" | "none";
export type Action = "allow" | "deny" | "drop" | "reject" | "limit" | "other";

export interface FwRule {
  /** Unique within the listing. */
  id: string;
  /** The number the backend deletes by (`ufw delete N`, `iptables -D CHAIN N`); null where it deletes another way. */
  num: number | null;
  action: Action;
  /** `in`, `out` or `fwd`. */
  direction: "in" | "out" | "fwd";
  /** `22`, `80,443`, `6000:6007`, a service name; empty for any. */
  ports: string;
  /** `tcp`, `udp`, or empty for any. */
  proto: string;
  /** `Anywhere` or an address or subnet. */
  source: string;
  v6: boolean;
  packets: number | null;
  bytes: number | null;
  comment: string;
  /** Which chain it sits in (iptables, nftables) or zone (firewalld). */
  chain: string;
  /** nftables: `family table`, and the rule's handle, which is what `delete rule` takes. */
  table?: string;
  handle?: number;
  /** What the host printed, for the tooltip. */
  raw: string;
}

export interface Policies {
  incoming?: string;
  outgoing?: string;
  routed?: string;
}

export interface FirewallState {
  backend: Backend;
  active: boolean;
  policies: Policies;
  rules: FwRule[];
  /** Other firewalls found but not in use (`ufw` installed and disabled while iptables has rules). */
  notes: string[];
  /** nftables: where new rules go. */
  nft?: { family: string; table: string; chain: string };
}

export const EMPTY_STATE: FirewallState = { backend: "none", active: false, policies: {}, rules: [], notes: [] };

// -- reading ------------------------------------------------------------------------------------------------------

/** Run as root. Sections are marked `@@name`; a tool that isn't installed leaves its section empty. */
export const PROBE_SCRIPT = [
  "echo @@ufw",
  "command -v ufw >/dev/null 2>&1 && ufw status verbose 2>&1",
  "echo @@ufw-numbered",
  "command -v ufw >/dev/null 2>&1 && ufw status numbered 2>&1",
  "echo @@ufw-added",
  "command -v ufw >/dev/null 2>&1 && ufw show added 2>&1",
  "echo @@firewalld",
  "command -v firewall-cmd >/dev/null 2>&1 && { firewall-cmd --state 2>&1; firewall-cmd --list-all 2>&1; }",
  "echo @@nft",
  "command -v nft >/dev/null 2>&1 && nft -a list ruleset 2>&1",
  "echo @@iptables",
  "command -v iptables >/dev/null 2>&1 && iptables -L -n -v --line-numbers 2>&1",
  "echo @@end",
].join("; ");

export function sections(output: string): Map<string, string> {
  const out = new Map<string, string>();
  let name: string | null = null;
  let lines: string[] = [];
  const close = () => name && out.set(name, lines.join("\n"));
  for (const line of output.split(/\r?\n/)) {
    const m = /^@@([\w-]+)$/.exec(line);
    if (m) {
      close();
      name = m[1];
      lines = [];
    } else lines.push(line);
  }
  close();
  return out;
}

const ACTIONS: Record<string, Action> = { allow: "allow", accept: "allow", deny: "deny", drop: "drop", reject: "reject", limit: "limit" };

/** `Default: deny (incoming), allow (outgoing), disabled (routed)` and `Status: active`. */
export function parseUfwVerbose(text: string): { active: boolean; installed: boolean; policies: Policies } {
  const status = /^Status:\s*(\w+)/im.exec(text);
  const policies: Policies = {};
  const def = /^Default:\s*(.+)$/im.exec(text);
  if (def) {
    for (const m of def[1].matchAll(/(\w+)\s*\((incoming|outgoing|routed)\)/g)) policies[m[2] as keyof Policies] = m[1].toLowerCase();
  }
  return { active: status?.[1].toLowerCase() === "active", installed: !!status, policies };
}

/** The ports and protocol of ufw's "To" column: `22/tcp`, `80,443/tcp`, `6000:6007/udp`, `OpenSSH`, `Anywhere`. */
function ufwTo(to: string): { ports: string; proto: string; v6: boolean; any: boolean } {
  const v6 = /\(v6\)/.test(to);
  const t = to.replace(/\s*\(v6\)/, "").trim();
  if (/^anywhere$/i.test(t)) return { ports: "", proto: "", v6, any: true };
  const m = /^([\w,:.-]+?)(?:\/(tcp|udp))?$/.exec(t);
  return { ports: m ? m[1] : t, proto: m?.[2] ?? "", v6, any: false };
}

/** `ufw status numbered`: `[ 1] 22/tcp   ALLOW IN   Anywhere   # ssh`. */
export function parseUfwNumbered(text: string): FwRule[] {
  const rules: FwRule[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const m = /^\[\s*(\d+)\]\s+(.*)$/.exec(raw);
    if (!m) continue;
    let rest = m[2];
    let comment = "";
    const c = /\s+#\s?(.*)$/.exec(rest);
    if (c) {
      comment = c[1].trim();
      rest = rest.slice(0, c.index);
    }
    const cols = rest.trim().split(/\s{2,}/);
    if (cols.length < 3) continue;
    const act = /^(ALLOW|DENY|REJECT|LIMIT)(?:\s+(IN|OUT|FWD))?$/i.exec(cols[1]);
    if (!act) continue;
    const to = ufwTo(cols[0]);
    const from = cols[2].trim();
    rules.push({
      id: `ufw-${m[1]}`,
      num: Number(m[1]),
      action: ACTIONS[act[1].toLowerCase()],
      direction: (act[2]?.toLowerCase() as FwRule["direction"]) ?? "in",
      ports: to.ports,
      proto: to.proto,
      source: from.replace(/\s*\(v6\)/, ""),
      v6: to.v6 || /\(v6\)/.test(from),
      packets: null,
      bytes: null,
      comment,
      chain: "ufw",
      raw: raw.trim(),
    });
  }
  return rules;
}

/** `ufw show added`: the rules ufw will apply when enabled, in the order given to it. */
export function parseUfwAdded(text: string): FwRule[] {
  const rules: FwRule[] = [];
  let n = 0;
  for (const raw of text.split(/\r?\n/)) {
    const m = /^ufw\s+(?:insert\s+\d+\s+)?(allow|deny|reject|limit)\s*(in|out)?\s*(.*)$/i.exec(raw.trim());
    if (!m) continue;
    n++;
    let rest = m[3];
    let comment = "";
    const c = /\scomment\s+'([^']*)'/.exec(rest);
    if (c) {
      comment = c[1];
      rest = rest.replace(c[0], "");
    }
    const from = /\bfrom\s+(\S+)/.exec(rest)?.[1] ?? "any";
    const port = /\bport\s+(\S+)/.exec(rest)?.[1] ?? "";
    const proto = /\bproto\s+(tcp|udp)/.exec(rest)?.[1] ?? "";
    const simple = /^(\d[\w,:]*)(?:\/(tcp|udp))?$/.exec(rest.trim());
    rules.push({
      id: `ufw-added-${n}`,
      num: null,
      action: ACTIONS[m[1].toLowerCase()],
      direction: (m[2]?.toLowerCase() as FwRule["direction"]) ?? "in",
      ports: port || simple?.[1] || "",
      proto: proto || simple?.[2] || "",
      source: from === "any" ? "Anywhere" : from,
      v6: isIPv6(from),
      packets: null,
      bytes: null,
      comment,
      chain: "ufw",
      raw: raw.trim(),
    });
  }
  return rules;
}

/** iptables prints counters like `12345`, `3012K`, `4M`. */
function counter(s: string): number {
  const m = /^(\d+)([KMGT])?$/.exec(s);
  if (!m) return 0;
  return Number(m[1]) * ({ K: 1e3, M: 1e6, G: 1e9, T: 1e12 }[m[2] ?? ""] ?? 1);
}

const IPT_ACTIONS: Record<string, Action> = { ACCEPT: "allow", DROP: "drop", REJECT: "reject", RETURN: "other" };

/** `iptables -L -n -v --line-numbers`: INPUT, FORWARD and OUTPUT (the user chains are what those jump to). */
export function parseIptables(text: string): { policies: Policies; rules: FwRule[]; hasRules: boolean } {
  const policies: Policies = {};
  const rules: FwRule[] = [];
  let chain = "";
  let hasRules = false;
  for (const raw of text.split(/\r?\n/)) {
    const head = /^Chain (\S+)(?: \(policy (\w+)(?: [^)]*)?\))?/.exec(raw);
    if (head) {
      chain = head[1];
      if (head[2]) {
        const p = head[2].toLowerCase() === "accept" ? "allow" : head[2].toLowerCase();
        if (chain === "INPUT") policies.incoming = p;
        else if (chain === "OUTPUT") policies.outgoing = p;
        else if (chain === "FORWARD") policies.routed = p;
      }
      continue;
    }
    const m = /^\s*(\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s*(.*)$/.exec(raw);
    if (!m || !["INPUT", "OUTPUT", "FORWARD"].includes(chain)) continue;
    hasRules = true;
    const [, num, pkts, bytes, target, protoRaw, , , , source, , extra] = m;
    const proto = protoRaw === "all" || protoRaw === "0" ? "" : protoRaw;
    const comment = /\/\*\s*(.*?)\s*\*\//.exec(extra)?.[1] ?? "";
    const ports = /dports?\s+(\S+)/.exec(extra)?.[1] ?? /dpts?:(\S+)/.exec(extra)?.[1] ?? "";
    rules.push({
      id: `ipt-${chain}-${num}`,
      num: Number(num),
      action: IPT_ACTIONS[target] ?? "other",
      direction: chain === "INPUT" ? "in" : chain === "OUTPUT" ? "out" : "fwd",
      ports,
      proto,
      source: source === "0.0.0.0/0" || source === "::/0" ? "Anywhere" : source,
      v6: false,
      packets: counter(pkts),
      bytes: counter(bytes),
      comment: comment || (IPT_ACTIONS[target] ? "" : `→ ${target}`),
      chain,
      raw: raw.trim(),
    });
  }
  return { policies, rules, hasRules };
}

/** `nft -a list ruleset`: only chains attached to a hook; a rule is one line ending in a verdict and `# handle N`. */
export function parseNft(text: string): { policies: Policies; rules: FwRule[]; place?: { family: string; table: string; chain: string } } {
  const policies: Policies = {};
  const rules: FwRule[] = [];
  let place: { family: string; table: string; chain: string } | undefined;
  let family = "";
  let table = "";
  let chain = "";
  let hook = "";
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    const t = /^table\s+(\S+)\s+(\S+)\s*\{/.exec(line);
    if (t) {
      family = t[1];
      table = t[2];
      continue;
    }
    const c = /^chain\s+(\S+)\s*\{/.exec(line);
    if (c) {
      chain = c[1];
      hook = "";
      continue;
    }
    const h = /^type\s+\w+\s+hook\s+(\w+)\s+priority\s+[^;]+;(?:\s*policy\s+(\w+);)?/.exec(line);
    if (h) {
      hook = h[1];
      if (h[2]) {
        const p = h[2] === "accept" ? "allow" : h[2];
        if (hook === "input") policies.incoming = p;
        else if (hook === "output") policies.outgoing = p;
        else if (hook === "forward") policies.routed = p;
      }
      if (hook === "input" && !place && family !== "bridge") place = { family, table, chain };
      continue;
    }
    if (!hook || !["input", "output", "forward"].includes(hook)) continue;
    const handle = /#\s*handle\s+(\d+)\s*$/.exec(line);
    if (!handle) continue;
    const body = line.slice(0, handle.index).trim();
    const verdict = /\b(accept|drop|reject|jump\s+\S+|goto\s+\S+|return|queue)\s*$/.exec(body.replace(/\bcomment\s+"[^"]*"\s*$/, "").trim());
    if (!verdict) continue;
    const word = verdict[1].split(/\s+/)[0];
    const cnt = /counter\s+packets\s+(\d+)\s+bytes\s+(\d+)/.exec(body);
    const dport = /\b(tcp|udp)\s+dport\s+(\{[^}]*\}|\S+)/.exec(body);
    const saddr = /\b(?:ip|ip6)\s+saddr\s+(\{[^}]*\}|\S+)/.exec(body);
    const comment = /comment\s+"([^"]*)"/.exec(body)?.[1] ?? "";
    rules.push({
      id: `nft-${family}-${table}-${handle[1]}`,
      num: null,
      action: ACTIONS[word] ?? "other",
      direction: hook === "input" ? "in" : hook === "output" ? "out" : "fwd",
      ports: dport ? dport[2].replace(/[{}\s]/g, "") : "",
      proto: dport?.[1] ?? "",
      source: saddr ? saddr[1].replace(/[{}\s]/g, "") : "Anywhere",
      v6: /\bip6\b/.test(body),
      packets: cnt ? Number(cnt[1]) : null,
      bytes: cnt ? Number(cnt[2]) : null,
      comment: comment || (ACTIONS[word] ? "" : body.replace(/\s*counter.*$/, "")),
      chain,
      table: `${family} ${table}`,
      handle: Number(handle[1]),
      raw: line,
    });
  }
  return { policies, rules, place };
}

/** `firewall-cmd --list-all`: services, ports and rich rules of the active zone, as allow rules. */
export function parseFirewalld(text: string): { running: boolean; zone: string; policies: Policies; rules: FwRule[] } {
  const running = /^running\b/m.test(text);
  const zone = /^(\S+)\s+\(active\)/m.exec(text)?.[1] ?? "";
  const field = (name: string) => new RegExp(`^\\s+${name}:\\s*(.*)$`, "m").exec(text)?.[1].trim() ?? "";
  const target = field("target");
  const policies: Policies = { incoming: target === "ACCEPT" ? "allow" : target === "DROP" ? "drop" : "reject" };
  const rules: FwRule[] = [];
  let n = 0;
  const add = (r: Partial<FwRule> & { raw: string }) => rules.push({ id: `fwd-${++n}`, num: null, action: "allow", direction: "in", ports: "", proto: "", source: "Anywhere", v6: false, packets: null, bytes: null, comment: "", chain: zone, ...r });
  for (const s of field("services").split(/\s+/).filter(Boolean)) add({ ports: s, comment: "service", raw: `service ${s}` });
  for (const p of field("ports").split(/\s+/).filter(Boolean)) {
    const m = /^([\d-]+)\/(tcp|udp)$/.exec(p);
    if (m) add({ ports: m[1].replace("-", ":"), proto: m[2], raw: `port ${p}` });
  }
  const richStart = text.search(/^\s+rich rules:/m);
  if (richStart >= 0) {
    for (const line of text.slice(richStart).split(/\r?\n/).slice(1)) {
      const r = line.trim();
      if (!r.startsWith("rule ")) continue;
      const verdict = /\b(accept|reject|drop)\s*$/.exec(r.replace(/\blog\b[^)]*$/, "").trim());
      add({
        action: verdict ? ACTIONS[verdict[1]] : "other",
        source: /source address="([^"]+)"/.exec(r)?.[1] ?? "Anywhere",
        ports: /port port="([^"]+)"/.exec(r)?.[1] ?? "",
        proto: /protocol="(tcp|udp)"/.exec(r)?.[1] ?? "",
        v6: /family="ipv6"/.test(r),
        raw: r,
      });
    }
  }
  return { running, zone, policies, rules };
}

/** Which firewall is in charge, from the probe's output: ufw, firewalld, then whatever nftables or iptables hold. */
export function parseFirewall(output: string): FirewallState {
  const s = sections(output);
  const notes: string[] = [];
  const ufw = parseUfwVerbose(s.get("ufw") ?? "");
  const fwd = parseFirewalld(s.get("firewalld") ?? "");
  const nft = parseNft(s.get("nft") ?? "");
  const ipt = parseIptables(s.get("iptables") ?? "");

  if (ufw.active) {
    return { backend: "ufw", active: true, policies: ufw.policies, rules: parseUfwNumbered(s.get("ufw-numbered") ?? ""), notes };
  }
  if (fwd.running) return { backend: "firewalld", active: true, policies: fwd.policies, rules: fwd.rules, notes };

  const nftActive = nft.rules.length > 0 || Object.values(nft.policies).some((p) => p === "drop" || p === "reject");
  const iptActive = ipt.hasRules || ipt.policies.incoming === "drop" || ipt.policies.incoming === "reject";
  if (ufw.installed) notes.push("ufw is installed but disabled.");
  if (nftActive) return { backend: "nftables", active: true, policies: nft.policies, rules: nft.rules, notes, nft: nft.place };
  if (iptActive) return { backend: "iptables", active: true, policies: ipt.policies, rules: ipt.rules, notes };

  // Nothing is filtering. Say which tool would do it, and what ufw has queued up for when it is turned on.
  if (ufw.installed) return { backend: "ufw", active: false, policies: ufw.policies, rules: parseUfwAdded(s.get("ufw-added") ?? ""), notes };
  if (s.get("firewalld")?.trim()) return { backend: "firewalld", active: false, policies: {}, rules: [], notes };
  if (s.get("nft")?.trim() || nft.place) return { backend: "nftables", active: false, policies: nft.policies, rules: [], notes, nft: nft.place };
  if ((s.get("iptables") ?? "").includes("Chain INPUT")) return { backend: "iptables", active: false, policies: ipt.policies, rules: ipt.rules, notes };
  return { ...EMPTY_STATE, notes: ["No firewall tool was found on this host."] };
}

// -- who gets in --------------------------------------------------------------------------------------------------

/** Whether `rule` lets a TCP connection to `port` from `ip` through (or turns it away). */
function matches(rule: FwRule, port: number, ip: string): boolean {
  if (rule.direction !== "in") return false;
  if (rule.proto && rule.proto !== "tcp") return false;
  if (rule.action === "other") return false;
  // An IPv6 rule doesn't speak for an IPv4 client.
  if (rule.v6 && !ip.includes(":")) return false;
  // A service name (firewalld, ufw app profile) isn't a number we can compare.
  if (rule.ports && /[a-z]/i.test(rule.ports.replace(/[,:\d-]/g, "")) && !/^\d/.test(rule.ports)) {
    return rule.ports.toLowerCase() === "ssh" || rule.ports.toLowerCase() === "openssh" ? port === 22 : false;
  }
  if (!portsInclude(rule.ports, port)) return false;
  return sourceCovers(rule.source, ip);
}

/** First matching rule wins, then the default. Where the rules can't say (names, user chains) it answers `unknown`. */
export function incomingVerdict(state: FirewallState, port: number, ip: string, skip?: string): "allow" | "deny" | "unknown" {
  if (!state.active) return "allow";
  for (const r of state.rules) {
    if (r.id === skip || !matches(r, port, ip)) continue;
    return r.action === "allow" || r.action === "limit" ? "allow" : "deny";
  }
  const d = state.policies.incoming;
  if (d === "allow") return "allow";
  if (d === "deny" || d === "drop" || d === "reject") return "deny";
  return "unknown";
}

export interface SessionContext {
  /** The address the host sees this connection come from. */
  clientIp: string | null;
  sshPort: number;
}

export interface Verdict {
  ok: boolean;
  /** Why not, in words for the person. */
  reason?: string;
}

/** Deleting `rule` must leave SSH reachable from here. */
export function checkDelete(state: FirewallState, rule: FwRule, ctx: SessionContext): Verdict {
  if (!state.active || !ctx.clientIp) return { ok: true };
  if (incomingVerdict(state, ctx.sshPort, ctx.clientIp) !== "allow") return { ok: true };
  if (incomingVerdict(state, ctx.sshPort, ctx.clientIp, rule.id) === "allow") return { ok: true };
  return { ok: false, reason: `This is the rule that lets you in on port ${ctx.sshPort} from ${ctx.clientIp}. Deleting it would cut this connection off, and any new one.` };
}

/** Blocking `source` must not take this connection with it. */
export function checkBlock(source: string, ctx: SessionContext): Verdict {
  if (ctx.clientIp && sourceCovers(source, ctx.clientIp)) return { ok: false, reason: `${source} includes your own address (${ctx.clientIp}); blocking it would lock you out.` };
  return { ok: true };
}

/** Turning the firewall on needs a rule that keeps SSH open, or the next packet ends this session. */
export function checkEnable(state: FirewallState, ctx: SessionContext): Verdict {
  if (state.active) return { ok: true };
  const probe = { ...state, active: true, policies: { ...state.policies, incoming: state.policies.incoming ?? "deny" } };
  if (ctx.clientIp && incomingVerdict(probe, ctx.sshPort, ctx.clientIp) === "allow") return { ok: true };
  return { ok: false, reason: `No rule allows SSH on port ${ctx.sshPort}${ctx.clientIp ? ` from ${ctx.clientIp}` : ""}. With the firewall on, this connection would be dropped.` };
}

/** A rule that changes port 22 (or the connected port) is held to the same test: it must not be a deny that wins. */
export function checkAddDeny(state: FirewallState, ports: string, source: string, ctx: SessionContext): Verdict {
  if (!ctx.clientIp) return { ok: true };
  const touchesSsh = portsInclude(ports, ctx.sshPort) && sourceCovers(source, ctx.clientIp);
  return touchesSsh ? { ok: false, reason: `That rule would block SSH on port ${ctx.sshPort} from ${ctx.clientIp}, which is you.` } : { ok: true };
}

// -- changing -----------------------------------------------------------------------------------------------------

export interface AllowRequest {
  ip: string;
  /** Empty: every port. */
  port: string;
  proto: "tcp" | "udp";
  /** Remove the rule again after this many seconds (`at` or `systemd-run` does it on the host). */
  ttlSecs?: number;
  comment?: string;
}

export function checkAllowRequest(r: AllowRequest): string | null {
  if (!isSource(r.ip)) return "That isn't an address or subnet.";
  if (r.port && !/^\d{1,5}(?:[:]\d{1,5})?$/.test(r.port)) return "A port is a number, or a range like 6000:6007.";
  if (r.ttlSecs !== undefined && (r.ttlSecs < 60 || r.ttlSecs > 30 * 86400)) return "A temporary rule lasts from a minute to 30 days.";
  if (r.comment && /[\r\n']/.test(r.comment)) return "A comment can't have quotes or new lines.";
  return null;
}

const MARK = "sshvault";

/** The commands for one rule, as a pair: how to add it and how to take it away again. */
function ruleCommands(state: FirewallState, r: AllowRequest, verb: "allow" | "deny"): { add: string; remove: string } {
  const portPart = r.port ? ` port ${r.port} proto ${r.proto}` : "";
  const comment = r.comment ? ` comment ${shq(r.comment)}` : "";
  switch (state.backend) {
    case "ufw": {
      const from = `from ${shq(r.ip)} to any${portPart}`;
      return verb === "allow" ? { add: `ufw allow ${from}${comment}`, remove: `ufw --force delete allow ${from}` } : { add: `ufw insert 1 deny from ${shq(r.ip)}${comment}`, remove: `ufw --force delete deny from ${shq(r.ip)}` };
    }
    case "firewalld": {
      const fam = isIPv6(r.ip.split("/")[0]) ? "ipv6" : "ipv4";
      const rich = `rule family="${fam}" source address="${r.ip}"${r.port ? ` port port="${r.port.replace(":", "-")}" protocol="${r.proto}"` : ""} ${verb === "allow" ? "accept" : "reject"}`;
      return { add: `firewall-cmd --add-rich-rule=${shq(rich)}`, remove: `firewall-cmd --remove-rich-rule=${shq(rich)}` };
    }
    case "nftables": {
      const n = state.nft ?? { family: "inet", table: "filter", chain: "input" };
      const ip = isIPv6(r.ip.split("/")[0]) ? "ip6" : "ip";
      const match = `${ip} saddr ${r.ip}${r.port ? ` ${r.proto} dport ${r.port.replace(":", "-")}` : ""}`;
      const rule = `${match} ${verb === "allow" ? "accept" : "drop"} comment "${MARK}"`;
      const place = `${n.family} ${n.table} ${n.chain}`;
      // The rule is found again by its comment and match when it is time to remove it.
      return {
        add: `nft insert rule ${place} ${rule}`,
        remove: `h=$(nft -a list chain ${place} | grep ${shq(match)} | grep ${shq(MARK)} | sed -n 's/.*# handle //p' | head -n 1); [ -n "$h" ] && nft delete rule ${place} handle "$h"`,
      };
    }
    default: {
      const tool = isIPv6(r.ip.split("/")[0]) ? "ip6tables" : "iptables";
      const spec = `-s ${shq(r.ip)}${r.port ? ` -p ${r.proto} --dport ${r.port}` : ""} -j ${verb === "allow" ? "ACCEPT" : "DROP"}`;
      return { add: `${tool} -I INPUT 1 ${spec}`, remove: `${tool} -D INPUT ${spec}` };
    }
  }
}

/** Exit status when the host has nothing to run a timed removal with. */
export const NO_TIMER = 90;

/**
 * The script that adds an allow rule, and with a lifetime also arranges its removal. The timer is set up first:
 * a rule that was meant to expire never gets added if nothing can expire it.
 */
export function allowScript(state: FirewallState, r: AllowRequest): string {
  const bad = checkAllowRequest(r);
  if (bad) throw new Error(bad);
  const { add, remove } = ruleCommands(state, r, "allow");
  if (r.ttlSecs === undefined) return `${add}${state.backend === "firewalld" ? " --permanent && firewall-cmd --reload" : ""}`;
  if (state.backend === "firewalld") {
    // firewalld times its own runtime rules.
    return `${ruleCommands(state, r, "allow").add} --timeout=${Math.floor(r.ttlSecs)}`;
  }
  const unit = `${MARK}-fw-${Date.now().toString(36)}`;
  const secs = Math.floor(r.ttlSecs);
  const minutes = Math.max(1, Math.ceil(secs / 60));
  return [
    "if command -v systemd-run >/dev/null 2>&1; then",
    `  ${add} && systemd-run --quiet --on-active=${secs}s --unit=${unit} sh -c ${shq(remove)};`,
    "elif command -v at >/dev/null 2>&1; then",
    `  ${add} && printf '%s\\n' ${shq(remove)} | at now + ${minutes} minutes 2>/dev/null;`,
    "else",
    `  echo 'no systemd-run or at on this host to remove the rule later' >&2; exit ${NO_TIMER};`,
    "fi",
  ].join(" ");
}

export function blockScript(state: FirewallState, ip: string, comment = ""): string {
  const req: AllowRequest = { ip, port: "", proto: "tcp", comment };
  const bad = checkAllowRequest(req);
  if (bad) throw new Error(bad);
  return ruleCommands(state, req, "deny").add + (state.backend === "firewalld" ? " --permanent && firewall-cmd --reload" : "");
}

export function deleteScript(state: FirewallState, rule: FwRule): string {
  switch (state.backend) {
    case "ufw":
      if (rule.num === null) throw new Error("This rule has no number to delete by.");
      // ufw renumbers after each delete, and asks; --force answers the question.
      return `ufw --force delete ${rule.num}`;
    case "iptables":
      if (rule.num === null || !/^(INPUT|OUTPUT|FORWARD)$/.test(rule.chain)) throw new Error("This rule can't be deleted by number.");
      return `iptables -D ${rule.chain} ${rule.num}`;
    case "nftables": {
      if (rule.handle === undefined || !rule.table || !/^[\w-]+ [\w-]+$/.test(rule.table) || !/^[\w-]+$/.test(rule.chain)) throw new Error("This rule has no handle to delete by.");
      return `nft delete rule ${rule.table} ${rule.chain} handle ${rule.handle}`;
    }
    case "firewalld": {
      if (/^service /.test(rule.raw)) return `firewall-cmd --permanent --remove-service=${shq(rule.raw.slice(8))} && firewall-cmd --reload`;
      if (/^port /.test(rule.raw)) return `firewall-cmd --permanent --remove-port=${shq(rule.raw.slice(5))} && firewall-cmd --reload`;
      return `firewall-cmd --permanent --remove-rich-rule=${shq(rule.raw)} && firewall-cmd --reload`;
    }
    default:
      throw new Error("There is no firewall to change.");
  }
}

/** ufw: allow SSH first, then turn it on; the order keeps the connection alive. */
export function enableUfwScript(sshPort: number, clientIp: string | null, haveAllow: boolean): string {
  const allow = haveAllow ? "" : clientIp && isIP(clientIp) ? `ufw allow from ${shq(clientIp)} to any port ${sshPort} proto tcp comment 'sshvault: this connection' && ` : `ufw allow ${sshPort}/tcp && `;
  return `${allow}ufw --force enable`;
}

export const refreshAfter = (script: string) => script;
