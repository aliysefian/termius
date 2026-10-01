// The `ssh` command line equivalent to a saved host, for pasting into a
// shell, a script or a ticket. Never includes secrets.
import type { Host, Identity, ProxySpec, Uuid, VaultRecord } from "./types";

export interface SshCommandInput {
  host: Host;
  /** The host's own id, so a group default never makes it jump through itself. */
  hostId?: Uuid;
  hostById: Map<Uuid, VaultRecord<Host>>;
  identityById: Map<Uuid, VaultRecord<Identity>>;
  /** Credential in effect for a host, after group defaults. */
  identityFor: (h: Host) => Uuid | undefined;
  /** Jump host in effect for a host (with its id), after group defaults. */
  jumpFor: (h: Host, id?: Uuid) => Uuid | undefined;
  /** Proxy in effect for a host, after group defaults. */
  proxyFor?: (h: Host) => ProxySpec | undefined;
}

function hop(h: Host, user: string | undefined): string {
  const addr = h.hostname.includes(":") ? `[${h.hostname}]` : h.hostname;
  return `${user ? `${user}@` : ""}${addr}${h.port !== 22 ? `:${h.port}` : ""}`;
}

/** Quotes for a POSIX shell, where OpenSSH runs a ProxyCommand. */
const sq = (s: string) => (/^[\w@%:.,/=+-]+$/.test(s) ? s : `'${s.replaceAll("'", `'\\''`)}'`);

/** OpenSSH ProxyCommand equivalent to a proxy; credentials are left out. */
export function proxyCommand(p: ProxySpec): string {
  if (p.kind === "command") return p.command;
  const at = `${p.host.includes(":") ? `[${p.host}]` : p.host}:${p.port}`;
  if (p.kind === "socks5") return `nc -X 5 -x ${at} %h %p`;
  return `nc -X connect -x ${at}${p.username ? ` -P ${p.username}` : ""} %h %p`;
}

export function sshCommand(i: SshCommandInput): string {
  const user = (h: Host) => {
    const id = i.identityFor(h);
    return id ? i.identityById.get(id)?.data?.username : undefined;
  };
  // Jump chain, outermost first, stopping at loops.
  const jumps: Host[] = [];
  const seen = new Set<Uuid>(i.hostId ? [i.hostId] : []);
  let cur = i.jumpFor(i.host, i.hostId);
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const h = i.hostById.get(cur)?.data;
    if (!h) break;
    jumps.unshift(h);
    cur = i.jumpFor(h, cur);
  }
  // The proxy reaches the first hop dialled: the outermost jump (its own
  // proxy, else the target's), or the target itself.
  const proxy = i.proxyFor?.(jumps[0] ?? i.host) ?? (jumps.length ? i.proxyFor?.(i.host) : undefined);
  const parts = ["ssh"];
  if (i.host.port !== 22) parts.push("-p", String(i.host.port));
  if (proxy && jumps.length) {
    // -J and ProxyCommand can't be combined, so spell the chain out as
    // nested `ssh -W`, escaping % once per level.
    let via = proxyCommand(proxy);
    for (const j of jumps) {
      const port = j.port !== 22 ? ` -p ${j.port}` : "";
      const u = user(j);
      via = `ssh${port} -o ${sq(`ProxyCommand=${via.replaceAll("%", "%%")}`)} -W %h:%p ${u ? `${u}@` : ""}${j.hostname}`;
    }
    parts.push("-o", sq(`ProxyCommand=${via}`));
  } else if (proxy) {
    parts.push("-o", sq(`ProxyCommand=${proxyCommand(proxy)}`));
  } else if (jumps.length) {
    parts.push("-J", jumps.map((j) => hop(j, user(j))).join(","));
  }
  if (i.host.forward_agent) parts.push("-A");
  if (i.host.forward_x11) parts.push("-X");
  const u = user(i.host);
  parts.push(`${u ? `${u}@` : ""}${i.host.hostname}`);
  return parts.join(" ");
}

/** "3 minutes ago", "2 days ago". */
export function timeAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} minute${m === 1 ? "" : "s"} ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} hour${h === 1 ? "" : "s"} ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d} day${d === 1 ? "" : "s"} ago`;
  const mo = Math.round(d / 30);
  return mo < 12 ? `${mo} month${mo === 1 ? "" : "s"} ago` : `${Math.round(mo / 12)} year${mo < 18 ? "" : "s"} ago`;
}
