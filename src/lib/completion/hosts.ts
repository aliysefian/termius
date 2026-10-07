// `ssh we` offers the saved hosts whose name matches: the app knows every host name, and the line gets what
// the command needs to reach it (user@hostname, and the port when it isn't 22). Pure.
import { fuzzyScore } from "$lib/fuzzy";
import { parseLine } from "./command";
import { matchPositions, type MenuItem } from "./menu";

export interface HostLike {
  id: string;
  label: string;
  hostname: string;
  port: number;
  /** The login name from the host's identity, when it has one. */
  username?: string;
  /** "ssh" (or empty), "telnet", "rdp", "ftp": only SSH hosts are offered. */
  protocol?: string;
}

/** What each command takes a destination from, and how it spells the port and the target. */
const COMMANDS: Record<string, { port: string; scp: boolean }> = {
  ssh: { port: "-p", scp: false },
  mosh: { port: "-p", scp: false },
  sftp: { port: "-P", scp: false },
  scp: { port: "-P", scp: true },
};

export const MAX_HOST_ITEMS = 6;

/** Options that take a value of their own, so the word after them is not the destination. */
const TAKES_VALUE = new Set(["-p", "-P", "-i", "-l", "-o", "-F", "-J", "-L", "-R", "-D", "-b", "-c", "-E", "-S", "-B", "-W"]);

/** A host name safe to put on a line as it is. */
const PLAIN = /^[A-Za-z0-9._-]+$/;

export function hostItems(text: string, hosts: HostLike[]): MenuItem[] {
  const parsed = parseLine(text);
  if (parsed.quoted || parsed.redirect) return [];
  // `sudo ssh` and `env X=1 ssh` are still ssh.
  const words = [...parsed.words];
  while (words[0] === "sudo" || words[0] === "time") words.shift();
  const cmd = words[0];
  const how = cmd ? COMMANDS[cmd] : undefined;
  if (!how) return [];
  const typed = parsed.current;
  if (typed.startsWith("-") || typed.includes("/") || typed.startsWith(".") || typed.startsWith("~")) return [];
  // Words already on the line that are neither options nor an option's value: the destination, or files.
  const before = words.slice(1);
  let positionals = 0;
  for (let i = 0; i < before.length; i++) {
    if (TAKES_VALUE.has(before[i])) i++;
    else if (!before[i].startsWith("-")) positionals++;
  }
  // After an option that wants a value, the word is that value (a key file, a port), not a host.
  if (before.length && TAKES_VALUE.has(before[before.length - 1])) return [];
  const firstPositional = positionals === 0;
  // ssh has one destination; scp's later words may be hosts too (`scp f1 host:`).
  if (!how.scp && !firstPositional) return [];
  if (typed.includes(":")) return [];
  const query = typed.includes("@") ? typed.slice(typed.indexOf("@") + 1) : typed;

  const scored: { item: MenuItem; s: number }[] = [];
  for (const h of hosts) {
    if (h.protocol && h.protocol !== "ssh") continue;
    if (!PLAIN.test(h.hostname)) continue;
    const byLabel = fuzzyScore(query, h.label);
    const byName = fuzzyScore(query, h.hostname);
    if (byLabel === null && byName === null) continue;
    const user = h.username && PLAIN.test(h.username) ? h.username : "";
    const target = `${user ? `${user}@` : ""}${h.hostname}`;
    // The port goes in front when this word is the first thing after the command, where an option is allowed.
    const portPart = h.port !== 22 && firstPositional ? `${how.port} ${h.port} ` : "";
    const insert = portPart + target + (how.scp ? ":" : " ");
    const labelWins = byLabel !== null && byLabel + 50 >= (byName ?? -Infinity);
    scored.push({
      s: labelWins ? byLabel! + 50 : byName!,
      item: {
        id: `host:${h.id}`,
        kind: "value",
        group: "Hosts",
        label: h.label,
        labelHit: labelWins ? matchPositions(query, h.label) : [],
        detail: `${target}${h.port !== 22 ? ` · port ${h.port}` : ""}`,
        detailHit: [],
        insert,
        variables: false,
        erase: parsed.raw.length,
      },
    });
  }
  scored.sort((a, b) => b.s - a.s || a.item.label.localeCompare(b.item.label));
  return scored.slice(0, MAX_HOST_ITEMS).map((x) => x.item);
}
