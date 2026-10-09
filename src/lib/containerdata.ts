// Pure helpers for the Containers view: how things are shown, filtered and
// split into log lines. Nothing here talks to the backend.
import type { BadgeTone, ContainerImage, ContainerInfo, ContainerNetwork, ContainerRuntime, ContainerState, ContainerVolume, PortMapping, PruneItem, PruneResult } from "./types";
import { SecretMasker } from "./ops/mask";

// -- state ------------------------------------------------------------------

export function stateTone(s: ContainerState): BadgeTone {
  switch (s) {
    case "running":
      return "success";
    case "paused":
    case "restarting":
      return "warning";
    case "dead":
      return "danger";
    default:
      return "neutral";
  }
}

/** Running, paused and restarting containers hold resources; the rest are stopped. */
/** What the runtime's own health check says, read from the status text ("Up 3 hours (healthy)"); null when there is none. */
export function healthOf(status: string): "healthy" | "unhealthy" | "starting" | null {
  const m = /\((healthy|unhealthy|health: starting)\)/i.exec(status);
  if (!m) return null;
  const v = m[1].toLowerCase();
  return v === "healthy" ? "healthy" : v === "unhealthy" ? "unhealthy" : "starting";
}

export const isLive = (s: ContainerState) => s === "running" || s === "paused" || s === "restarting";

export type StateFilter = "all" | "running" | "stopped";

// -- ports ------------------------------------------------------------------

/** 0.0.0.0 and :: both mean "every address"; the pair is one line for a person. */
const ANY = new Set(["", "0.0.0.0", "::", "[::]"]);

/**
 * One label per published port (IPv4 and IPv6 "any" collapse to one), then the
 * exposed-only ports. `published` is false for a port nothing listens on from outside.
 */
export function portLabels(ports: PortMapping[]): { text: string; published: boolean }[] {
  const seen = new Set<string>();
  const published: { text: string; published: boolean }[] = [];
  const exposed: { text: string; published: boolean }[] = [];
  for (const p of ports) {
    if (!p.host_port) {
      const text = `${p.container_port}/${p.proto}`;
      if (!seen.has(text)) {
        seen.add(text);
        exposed.push({ text, published: false });
      }
      continue;
    }
    const ip = ANY.has(p.host_ip) ? "0.0.0.0" : p.host_ip;
    const text = `${ip}:${p.host_port}→${p.container_port}/${p.proto}`;
    if (!seen.has(text)) {
      seen.add(text);
      published.push({ text, published: true });
    }
  }
  return [...published, ...exposed];
}

export const formatPorts = (ports: PortMapping[]) => portLabels(ports).map((l) => l.text).join(", ");

// -- time and size ----------------------------------------------------------

/** "5s", "3m", "2h", "4d", "3w", "5mo", "2y": how long ago, coarsely. */
export function formatAge(createdSecs: number | null, nowMs: number = Date.now()): string {
  if (createdSecs === null) return "—";
  const s = Math.max(0, Math.floor(nowMs / 1000 - createdSecs));
  if (s < 60) return `${s}s`;
  const units: [number, string][] = [
    [60 * 60 * 24 * 365, "y"],
    [60 * 60 * 24 * 30, "mo"],
    [60 * 60 * 24 * 7, "w"],
    [60 * 60 * 24, "d"],
    [60 * 60, "h"],
    [60, "m"],
  ];
  for (const [secs, label] of units) if (s >= secs) return `${Math.floor(s / secs)}${label}`;
  return `${s}s`;
}

// -- filtering --------------------------------------------------------------

const terms = (q: string) => q.toLowerCase().split(/\s+/).filter(Boolean);

/** Every word must match the name, image, ID, status, ports, a label, or the pod. */
export function filterContainers(list: ContainerInfo[], query: string, state: StateFilter): ContainerInfo[] {
  const ts = terms(query);
  return list.filter((c) => {
    if (state === "running" && c.state !== "running") return false;
    if (state === "stopped" && isLive(c.state)) return false;
    if (ts.length === 0) return true;
    const hay = [c.name, c.image, c.id, c.status, c.state, c.pod ?? "", formatPorts(c.ports), ...Object.entries(c.labels).map(([k, v]) => `${k}=${v}`)]
      .join("\n")
      .toLowerCase();
    return ts.every((t) => hay.includes(t));
  });
}

export function filterImages(list: ContainerImage[], query: string): ContainerImage[] {
  const ts = terms(query);
  if (ts.length === 0) return list;
  return list.filter((i) => {
    const hay = `${i.repository}:${i.tag} ${i.id}`.toLowerCase();
    return ts.every((t) => hay.includes(t));
  });
}

export function countByState(list: ContainerInfo[]): { all: number; running: number; stopped: number } {
  const running = list.filter((c) => c.state === "running").length;
  const stopped = list.filter((c) => !isLive(c.state)).length;
  return { all: list.length, running, stopped };
}

/** The Compose project a container belongs to, if the labels say so. */
export const composeProject = (c: ContainerInfo): string | null =>
  c.labels["com.docker.compose.project"] ?? c.labels["io.podman.compose.project"] ?? null;

// -- shell ------------------------------------------------------------------

const REF = /^[A-Za-z0-9][A-Za-z0-9_.:/@-]*$/;

/**
 * The line to type to get a shell in a container: bash if it has one, else sh.
 * Null for a reference that isn't a plain name or ID (it is never typed).
 */
export function shellCommand(runtime: ContainerRuntime, id: string): string | null {
  if (!REF.test(id) || id.length > 200) return null;
  return `${runtime} exec -it ${id} sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'`;
}

// -- logs -------------------------------------------------------------------

// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07/g;

/**
 * Text arriving in chunks, kept as lines. A chunk may end mid-line, so the
 * unfinished line waits for its rest. Colour codes are removed, and a
 * carriage return overwrites the line so far (progress bars read as their
 * final state). Past `max` lines the oldest are dropped.
 */
export class LogBuffer {
  lines: string[] = [];
  /** Lines dropped from the top to stay within `max`. */
  dropped = 0;
  #partial = "";
  #masker = new SecretMasker();
  /** Hide passwords, tokens and keys in lines as they complete (new lines only). Off unless a view asks for it. */
  mask = false;

  constructor(readonly max = 20_000) {}

  /** Adds text; returns how many complete lines it produced. */
  push(text: string): number {
    const parts = (this.#partial + text.replace(/\r\n/g, "\n")).split("\n");
    this.#partial = parts.pop() ?? "";
    for (const raw of parts) {
      const line = this.mask ? this.#masker.line(clean(raw)) : clean(raw);
      if (line !== null) this.lines.push(line);
    }
    const over = this.lines.length - this.max;
    if (over > 0) {
      this.lines.splice(0, over);
      this.dropped += over;
    }
    return parts.length;
  }

  /** The line still being written, if any. */
  get partial(): string {
    return clean(this.#partial);
  }

  /** Every line including the unfinished one. */
  all(): string[] {
    return this.#partial ? [...this.lines, this.partial] : this.lines;
  }

  clear() {
    this.lines = [];
    this.#partial = "";
    this.dropped = 0;
    this.#masker = new SecretMasker();
  }
}

function clean(line: string): string {
  const stripped = line.replace(ANSI, "");
  // After a lone \r the rest replaces what came before it.
  return stripped.includes("\r") ? (stripped.split("\r").filter(Boolean).pop() ?? "") : stripped;
}

/** Positions of the lines containing `query` (case-insensitive), in order. */
export function matchingLines(lines: string[], query: string): number[] {
  const q = query.toLowerCase();
  if (!q) return [];
  const out: number[] = [];
  for (let i = 0; i < lines.length; i++) if (lines[i].toLowerCase().includes(q)) out.push(i);
  return out;
}

/** Splits a line around each case-insensitive match, for highlighting. */
export function splitMatches(line: string, query: string): { text: string; hit: boolean }[] {
  const q = query.toLowerCase();
  if (!q) return [{ text: line, hit: false }];
  const out: { text: string; hit: boolean }[] = [];
  const lower = line.toLowerCase();
  let at = 0;
  for (;;) {
    const i = lower.indexOf(q, at);
    if (i < 0) break;
    if (i > at) out.push({ text: line.slice(at, i), hit: false });
    out.push({ text: line.slice(i, i + q.length), hit: true });
    at = i + q.length;
  }
  if (at < line.length) out.push({ text: line.slice(at), hit: false });
  return out.length ? out : [{ text: line, hit: false }];
}

// -- Compose projects -------------------------------------------------------

export interface ProjectGroup {
  /** `null` for containers that belong to no project. */
  project: string | null;
  containers: ContainerInfo[];
  running: number;
  /** Where Compose was run, from its labels (on the host, not here). */
  workingDir: string | null;
  configFiles: string | null;
}

/**
 * Containers gathered by Compose project: projects by name, then the rest.
 * A project's order inside is by service name, then container name.
 */
export function groupByProject(list: ContainerInfo[]): ProjectGroup[] {
  const by = new Map<string | null, ContainerInfo[]>();
  for (const c of list) {
    const key = composeProject(c);
    by.set(key, [...(by.get(key) ?? []), c]);
  }
  const service = (c: ContainerInfo) => c.labels["com.docker.compose.service"] ?? c.name;
  const groups: ProjectGroup[] = [...by.entries()].map(([project, containers]) => {
    const sorted = project === null ? containers : [...containers].sort((a, b) => service(a).localeCompare(service(b)) || a.name.localeCompare(b.name));
    return {
      project,
      containers: sorted,
      running: sorted.filter((c) => c.state === "running").length,
      workingDir: sorted.find((c) => c.labels["com.docker.compose.project.working_dir"])?.labels["com.docker.compose.project.working_dir"] ?? null,
      configFiles: sorted.find((c) => c.labels["com.docker.compose.project.config_files"])?.labels["com.docker.compose.project.config_files"] ?? null,
    };
  });
  return groups.sort((a, b) => (a.project === null ? 1 : 0) - (b.project === null ? 1 : 0) || (a.project ?? "").localeCompare(b.project ?? ""));
}

// -- volumes, networks, pruning -------------------------------------------------

export function filterVolumes(list: ContainerVolume[], query: string): ContainerVolume[] {
  const ts = terms(query);
  if (ts.length === 0) return list;
  return list.filter((v) => {
    const hay = [v.name, v.driver, ...v.used_by, ...Object.entries(v.labels).map(([k, val]) => `${k}=${val}`)].join("\n").toLowerCase();
    return ts.every((t) => hay.includes(t));
  });
}

export function filterNetworks(list: ContainerNetwork[], query: string): ContainerNetwork[] {
  const ts = terms(query);
  if (ts.length === 0) return list;
  return list.filter((n) => {
    const hay = [n.name, n.driver, n.scope, n.id, ...n.used_by, ...Object.entries(n.labels).map(([k, val]) => `${k}=${val}`)].join("\n").toLowerCase();
    return ts.every((t) => hay.includes(t));
  });
}

/** "3 containers" / "none": who uses a volume or network, for a table cell. */
export function usedByText(users: string[], max = 2): string {
  if (users.length === 0) return "unused";
  const shown = users.slice(0, max).join(", ");
  return users.length > max ? `${shown} +${users.length - max}` : shown;
}

/** The Compose projects a set of items belongs to, so a prune can say what it breaks up. */
export function pruneProjects(items: PruneItem[]): string[] {
  return [...new Set(items.map((i) => i.project).filter((p): p is string => !!p))].sort();
}

/** What a prune actually did: how many went, and the ones that didn't with the reason. */
export function summarizePrune(results: PruneResult[]): { removed: number; failed: PruneResult[] } {
  return { removed: results.filter((r) => r.ok).length, failed: results.filter((r) => !r.ok) };
}

/** Whether `ref` can be pulled: a plain image reference, never anything that could be an option. */
export const isPullable = (ref: string) => REF.test(ref.trim()) && ref.trim().length <= 200;

/** How an image is named to remove it: its tag, or its ID when it has none. */
export function imageRef(i: ContainerImage): string {
  const untagged = !i.repository || !i.tag || i.repository === "<none>" || i.tag === "<none>";
  return untagged ? i.id : `${i.repository}:${i.tag}`;
}
