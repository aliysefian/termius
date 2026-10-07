// Snippet packs: the starter set that ships with the app, and snippets going out to a file and coming in
// from one. A pack is plain JSON; reading one is careful, because it comes from someone else.
import type { Snippet } from "./types";

export const PACK_FORMAT = "sshvault-snippets";
export const PACK_VERSION = 1;

export const MAX_PACK_BYTES = 1024 * 1024;
export const MAX_SNIPPETS = 500;
const MAX_LABEL = 120;
const MAX_COMMAND = 8000;
const MAX_DESCRIPTION = 500;
const MAX_FOLDER = 200;
const MAX_TAGS = 20;
const MAX_TAG = 40;

type Starter = Required<Pick<Snippet, "label" | "command" | "description" | "folder">> & { tags: string[] };

const s = (folder: string, label: string, command: string, description: string, tags: string[] = []): Starter => ({ folder, label, command, description, tags });

/** Everything here only looks: nothing deletes, stops, restarts or changes anything. A test keeps it so. */
export const STARTER_PACK: Starter[] = [
  s("Starter/Disk and memory", "Disk space by filesystem", "df -hT -x tmpfs -x devtmpfs", "How full each disk is", ["disk"]),
  s("Starter/Disk and memory", "Biggest folders here", "du -xh --max-depth=1 . 2>/dev/null | sort -rh | head -15", "Where the space in this folder went", ["disk"]),
  s("Starter/Disk and memory", "Biggest files under a folder", "find {{folder}} -xdev -type f -size +100M -exec ls -lh {} + 2>/dev/null | sort -k5 -rh | head -20", "Files over 100 MB, largest first", ["disk"]),
  s("Starter/Disk and memory", "Memory and swap", "free -h", "Used and free memory", ["memory"]),
  s("Starter/Disk and memory", "Block devices", "lsblk -o NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE", "Disks and partitions", ["disk"]),
  s("Starter/System", "Uptime and load", "uptime", "How long it has been up, and the load", ["system"]),
  s("Starter/System", "Top processes by CPU", "ps -eo pid,user,%cpu,%mem,etime,comm --sort=-%cpu | head -15", "What is using the CPU", ["system", "cpu"]),
  s("Starter/System", "Top processes by memory", "ps -eo pid,user,%mem,%cpu,etime,comm --sort=-%mem | head -15", "What is using the memory", ["system", "memory"]),
  s("Starter/System", "Kernel and OS", "uname -a; cat /etc/os-release 2>/dev/null | head -4", "Which system this is", ["system"]),
  s("Starter/System", "Who is logged in", "who; echo; last -n 10", "Current sessions and the last logins", ["users"]),
  s("Starter/Services and logs", "Failed services", "systemctl --failed --no-pager", "Units that are in a failed state", ["systemd"]),
  s("Starter/Services and logs", "Status of a service", "systemctl status {{service}} --no-pager -l", "State and recent log lines of one service", ["systemd"]),
  s("Starter/Services and logs", "Recent errors in the journal", "journalctl -p err -n 50 --no-pager", "The last 50 errors", ["logs", "systemd"]),
  s("Starter/Services and logs", "Follow a log file", "tail -n 100 -F {{file}}", "The end of a log file, as it grows", ["logs"]),
  s("Starter/Network", "Listening ports", "ss -tulpn", "What listens on which port, and which program", ["network"]),
  s("Starter/Network", "Addresses", "ip -br addr", "Interfaces and their addresses", ["network"]),
  s("Starter/Network", "Routes", "ip route", "The routing table", ["network"]),
  s("Starter/Network", "Look up a name", "getent hosts {{name}}", "What a name resolves to here", ["network", "dns"]),
  s("Starter/Network", "Fetch headers of a URL", "curl -sSI --max-time 10 {{url}}", "Status and headers, without the body", ["network", "http"]),
  s("Starter/Docker", "Containers", "docker ps -a", "All containers and their state", ["docker"]),
  s("Starter/Docker", "Docker disk use", "docker system df", "What images, containers and volumes take", ["docker", "disk"]),
  s("Starter/Docker", "Follow a container's log", "docker logs -f --tail 100 {{container}}", "The end of a container's log, as it grows", ["docker", "logs"]),
  s("Starter/Git", "Short status", "git status -sb", "Branch and changed files", ["git"]),
  s("Starter/Git", "Recent history", "git log --oneline --graph --decorate -n 20", "The last 20 commits as a graph", ["git"]),
];

// -- a pack on disk -------------------------------------------------------------------------------

export interface Pack {
  format: typeof PACK_FORMAT;
  version: typeof PACK_VERSION;
  snippets: Snippet[];
}

export function toPack(snippets: Snippet[]): string {
  const pack: Pack = {
    format: PACK_FORMAT,
    version: PACK_VERSION,
    snippets: snippets.map((x) => ({ label: x.label, command: x.command, description: x.description ?? "", folder: x.folder ?? "", tags: x.tags ?? [] })),
  };
  return JSON.stringify(pack, null, 2) + "\n";
}

/** A snippet as a pack holds it: every field present. */
export interface Entry {
  label: string;
  command: string;
  description: string;
  folder: string;
  tags: string[];
}

export interface Read {
  snippets: Entry[];
  /** Entries that were left out, and why. */
  skipped: { label: string; why: string }[];
  /** The file as a whole is unusable. */
  error: string | null;
}

// Control characters other than tab and newline have no business in a command or a name.
// eslint-disable-next-line no-control-regex
const CONTROL = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/;

const text = (v: unknown, max: number): string | null => (typeof v === "string" && v.length <= max && !CONTROL.test(v) ? v : null);

export function parsePack(raw: string): Read {
  const fail = (error: string): Read => ({ snippets: [], skipped: [], error });
  if (raw.length > MAX_PACK_BYTES) return fail("That file is too large to be a snippet pack.");
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return fail("That isn't a snippet pack: it isn't valid JSON.");
  }
  if (!data || typeof data !== "object" || (data as Pack).format !== PACK_FORMAT) return fail("That isn't a SSHVault snippet pack.");
  const version = (data as Pack).version;
  if (typeof version !== "number" || version > PACK_VERSION) return fail("That pack was made by a newer version of SSHVault.");
  const list = (data as Pack).snippets;
  if (!Array.isArray(list)) return fail("The pack has no snippets in it.");
  if (list.length > MAX_SNIPPETS) return fail(`A pack can hold at most ${MAX_SNIPPETS} snippets.`);

  const snippets: Entry[] = [];
  const skipped: Read["skipped"] = [];
  for (const entry of list as unknown[]) {
    const e = (entry && typeof entry === "object" ? entry : {}) as Record<string, unknown>;
    const label = text(e.label, MAX_LABEL)?.trim();
    const shown = typeof e.label === "string" ? e.label.slice(0, 60) : "(no name)";
    const command = text(e.command, MAX_COMMAND);
    if (!label) {
      skipped.push({ label: shown, why: "it has no usable name" });
      continue;
    }
    if (!command || !command.trim()) {
      skipped.push({ label, why: "it has no usable command" });
      continue;
    }
    const description = e.description === undefined ? "" : text(e.description, MAX_DESCRIPTION);
    if (description === null) {
      skipped.push({ label, why: "its description is not usable" });
      continue;
    }
    const folder = e.folder === undefined ? "" : text(e.folder, MAX_FOLDER);
    // A folder is a path of names; "..", empty parts and a leading slash would only be confusing.
    if (folder === null || folder.split("/").some((p) => p === ".." || p === ".") || folder.startsWith("/") || folder.includes("//")) {
      skipped.push({ label, why: "its folder is not usable" });
      continue;
    }
    const tagsIn = e.tags === undefined ? [] : e.tags;
    if (!Array.isArray(tagsIn) || tagsIn.length > MAX_TAGS || tagsIn.some((t) => text(t, MAX_TAG) === null)) {
      skipped.push({ label, why: "its tags are not usable" });
      continue;
    }
    snippets.push({ label, command, description, folder, tags: (tagsIn as string[]).map((t) => t.trim()).filter(Boolean) });
  }
  return { snippets, skipped, error: null };
}

/** What of `incoming` isn't already there (the same name and the same command). */
export function newOnes<T extends { label: string; command: string }>(existing: { label: string; command: string }[], incoming: T[]): { fresh: T[]; already: number } {
  const have = new Set(existing.map((x) => `${x.label}\n${x.command}`));
  const fresh: T[] = [];
  for (const x of incoming) {
    const key = `${x.label}\n${x.command}`;
    if (have.has(key)) continue;
    have.add(key);
    fresh.push(x);
  }
  return { fresh, already: incoming.length - fresh.length };
}
