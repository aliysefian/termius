// The `ssh` command line equivalent to a saved host, for pasting into a
// shell, a script or a ticket. Never includes secrets.
import type { Host, Identity, Uuid, VaultRecord } from "./types";

export interface SshCommandInput {
  host: Host;
  hostById: Map<Uuid, VaultRecord<Host>>;
  identityById: Map<Uuid, VaultRecord<Identity>>;
  /** Credential in effect for a host, after group defaults. */
  identityFor: (h: Host) => Uuid | undefined;
  jumpFor: (h: Host) => Uuid | undefined;
}

function hop(h: Host, user: string | undefined): string {
  const addr = h.hostname.includes(":") ? `[${h.hostname}]` : h.hostname;
  return `${user ? `${user}@` : ""}${addr}${h.port !== 22 ? `:${h.port}` : ""}`;
}

export function sshCommand(i: SshCommandInput): string {
  const user = (h: Host) => {
    const id = i.identityFor(h);
    return id ? i.identityById.get(id)?.data?.username : undefined;
  };
  // Jump chain, outermost first, stopping at loops.
  const jumps: Host[] = [];
  const seen = new Set<Uuid>();
  let cur = i.jumpFor(i.host);
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const h = i.hostById.get(cur)?.data;
    if (!h) break;
    jumps.unshift(h);
    cur = i.jumpFor(h);
  }
  const parts = ["ssh"];
  if (i.host.port !== 22) parts.push("-p", String(i.host.port));
  if (jumps.length) parts.push("-J", jumps.map((j) => hop({ ...j, port: j.port }, user(j))).join(","));
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
