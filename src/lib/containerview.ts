// The Containers workspace's rules, kept apart from the screen so they can be tested: what the metrics count, how the
// status filter reads the runtime's states, which actions a container offers in each state, which columns fit, how a
// container's inspect output is summarised (and its secrets hidden), how stats readings are kept, and how a failure
// is explained. Nothing here talks to the backend, and nothing is made up: a missing value stays missing.

import { filterContainers, groupByProject, healthOf, isLive, type ProjectGroup } from "./containerdata";
import type { ContainerInfo, ContainerStat, PortMapping } from "./types";

// -- state and health ---------------------------------------------------------------------------------------------

export type StatusFilter = "all" | "running" | "stopped" | "paused" | "restarting" | "failed" | "unhealthy";

export const STATUS_FILTERS: { id: StatusFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "running", label: "Running" },
  { id: "stopped", label: "Stopped" },
  { id: "paused", label: "Paused" },
  { id: "restarting", label: "Restarting" },
  { id: "failed", label: "Failed / exited" },
  { id: "unhealthy", label: "Unhealthy" },
];

/** The exit code in "Exited (137) 2 hours ago", or null when the runtime didn't print one. */
export function exitCode(status: string): number | null {
  const m = /exited \((-?\d+)\)/i.exec(status);
  return m ? Number(m[1]) : null;
}

/** Dead, or exited with a non-zero code: it ended badly. A clean exit (0) is just stopped. */
export const isFailed = (c: ContainerInfo): boolean => c.state === "dead" || (c.state === "exited" && (exitCode(c.status) ?? 0) !== 0);

/** Only what the container's own health check says. A container without one is neither healthy nor unhealthy. */
export const health = (c: ContainerInfo) => healthOf(c.status);

export function matchesStatus(c: ContainerInfo, f: StatusFilter): boolean {
  switch (f) {
    case "all":
      return true;
    case "running":
      return c.state === "running";
    case "stopped":
      return !isLive(c.state);
    case "paused":
      return c.state === "paused";
    case "restarting":
      return c.state === "restarting";
    case "failed":
      return isFailed(c);
    case "unhealthy":
      return health(c) === "unhealthy";
  }
}

export interface Metrics {
  total: number;
  running: number;
  stopped: number;
  unhealthy: number;
}

/** From the whole list, whatever the filters show. Paused and restarting count as neither running nor stopped. */
export function metrics(list: ContainerInfo[]): Metrics {
  return {
    total: list.length,
    running: list.filter((c) => c.state === "running").length,
    stopped: list.filter((c) => !isLive(c.state)).length,
    unhealthy: list.filter((c) => health(c) === "unhealthy").length,
  };
}

export function statusCounts(list: ContainerInfo[]): Record<StatusFilter, number> {
  const out = {} as Record<StatusFilter, number>;
  for (const f of STATUS_FILTERS) out[f.id] = list.filter((c) => matchesStatus(c, f.id)).length;
  return out;
}

/** Search (name, ID, image, project, service, ports, labels, status) together with the status filter. */
export function searchContainers(list: ContainerInfo[], query: string, status: StatusFilter): ContainerInfo[] {
  return filterContainers(list, query, "all").filter((c) => matchesStatus(c, status));
}

export interface GroupSummary extends ProjectGroup {
  unhealthy: number;
}

export function groupSummaries(list: ContainerInfo[]): GroupSummary[] {
  return groupByProject(list).map((g) => ({ ...g, unhealthy: g.containers.filter((c) => health(c) === "unhealthy").length }));
}

/** A project's containers are all up, only some are, or none is. */
export function projectState(g: { running: number; containers: unknown[] }): "running" | "partial" | "stopped" {
  return g.running === 0 ? "stopped" : g.running === g.containers.length ? "running" : "partial";
}

// -- actions ---------------------------------------------------------------------------------------------------------

export type MenuId = "start" | "stop" | "restart" | "pause" | "resume" | "logs" | "terminal" | "inspect" | "copy-id" | "copy-name" | "delete";

export interface MenuItem {
  id: MenuId;
  label: string;
  enabled: boolean;
  /** Why it is not available, shown as a tooltip. */
  why?: string;
  danger?: boolean;
  /** A separator goes before this item. */
  group: 1 | 2 | 3;
}

/**
 * Every action for a container in its state: the ones that make no sense are present but off, with the reason, so
 * the menu keeps its shape. `busy` is an operation already underway on it.
 */
export function menuItems(c: ContainerInfo, busy: boolean): MenuItem[] {
  const live = isLive(c.state);
  const paused = c.state === "paused";
  const wait = busy ? "Another operation is in progress" : undefined;
  return [
    { id: "start", label: "Start", enabled: !live && !busy && c.state !== "removing", why: wait ?? (live ? "Already running" : undefined), group: 1 },
    { id: "stop", label: "Stop", enabled: live && !busy, why: wait ?? (!live ? "Not running" : undefined), group: 1 },
    { id: "restart", label: "Restart", enabled: live && !paused && !busy, why: wait ?? (!live ? "Not running" : paused ? "Resume it first" : undefined), group: 1 },
    { id: "pause", label: "Pause", enabled: c.state === "running" && !busy, why: wait ?? (c.state !== "running" ? "Only a running container can be paused" : undefined), group: 1 },
    { id: "resume", label: "Resume", enabled: paused && !busy, why: wait ?? (!paused ? "Not paused" : undefined), group: 1 },
    { id: "logs", label: "Open Logs", enabled: true, group: 2 },
    { id: "terminal", label: "Open Terminal", enabled: c.state === "running" && !busy, why: c.state !== "running" ? "Only a running container has a shell to open" : wait, group: 2 },
    { id: "inspect", label: "Inspect", enabled: true, group: 2 },
    { id: "copy-id", label: "Copy Container ID", enabled: true, group: 3 },
    { id: "copy-name", label: "Copy Container Name", enabled: true, group: 3 },
    { id: "delete", label: "Delete…", enabled: !busy && c.state !== "removing", why: wait, danger: true, group: 3 },
  ];
}

/** The few actions a row always shows: stop (or start), logs, and the rest in a menu. */
export function rowPrimary(c: ContainerInfo): "start" | "stop" | "resume" {
  return c.state === "paused" ? "resume" : isLive(c.state) ? "stop" : "start";
}

// -- columns and width ------------------------------------------------------------------------------------------------

export type ColumnId = "image" | "ports" | "cpu" | "memory" | "age" | "created" | "size";

export const COLUMNS: { id: ColumnId; label: string; /** Narrowest table card (px) that still has room for it beside the name, state and actions. */ minWidth: number; needs?: "monitoring" | "sizes" }[] = [
  { id: "image", label: "Image", minWidth: 840 },
  { id: "ports", label: "Ports", minWidth: 1060 },
  { id: "age", label: "Age", minWidth: 1140 },
  { id: "cpu", label: "CPU", minWidth: 1220, needs: "monitoring" },
  { id: "memory", label: "Memory", minWidth: 1320, needs: "monitoring" },
  { id: "size", label: "Size", minWidth: 1400, needs: "sizes" },
  { id: "created", label: "Created", minWidth: 1480 },
];

export const DEFAULT_COLUMNS: ColumnId[] = ["image", "ports", "cpu", "memory", "age"];

/** The chosen columns that fit a table this wide and whose data is being read, in display order. */
export function visibleColumns(chosen: ColumnId[], width: number, monitoring: boolean, sizes: boolean): ColumnId[] {
  return COLUMNS.filter((c) => chosen.includes(c.id) && width >= c.minWidth && (c.needs !== "monitoring" || monitoring) && (c.needs !== "sizes" || sizes)).map((c) => c.id);
}

// -- stats ------------------------------------------------------------------------------------------------------------

const UNITS: Record<string, number> = { b: 1, kb: 1e3, mb: 1e6, gb: 1e9, tb: 1e12, kib: 1024, mib: 1024 ** 2, gib: 1024 ** 3, tib: 1024 ** 4 };

/** "12.3MiB" → bytes; null for "--" or anything unreadable. */
export function parseBytes(text: string): number | null {
  const m = /^\s*([\d.]+)\s*([A-Za-z]*)\s*$/.exec(text);
  if (!m) return null;
  const unit = UNITS[(m[2] || "b").toLowerCase()];
  const n = Number(m[1]);
  return unit && Number.isFinite(n) ? n * unit : null;
}

/** "1.2kB / 3.4MB" as the two numbers; each null when unreadable. */
export function parsePair(text: string): [number | null, number | null] {
  const [a, b] = text.split("/");
  return [a === undefined ? null : parseBytes(a), b === undefined ? null : parseBytes(b)];
}

export function formatBytes(n: number | null): string {
  if (n === null) return "–";
  if (n < 1024) return `${Math.round(n)} B`;
  const u = ["KiB", "MiB", "GiB", "TiB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

export interface Sample {
  at: number;
  cpu: number | null;
  memUsed: number | null;
  memLimit: number | null;
  memPct: number | null;
  netRx: number | null;
  netTx: number | null;
  blockRead: number | null;
  blockWrite: number | null;
  pids: number | null;
}

export function toSample(s: ContainerStat, at: number): Sample {
  const [memUsed, memLimit] = parsePair(s.mem_usage);
  const [netRx, netTx] = parsePair(s.net_io);
  const [blockRead, blockWrite] = parsePair(s.block_io);
  return { at, cpu: s.cpu_pct, memUsed, memLimit, memPct: s.mem_pct, netRx, netTx, blockRead, blockWrite, pids: s.pids };
}

/** The most recent readings of each container, bounded, so a long session keeps a fixed amount of memory. */
export class StatHistory {
  #by = new Map<string, Sample[]>();
  constructor(readonly max = 60) {}

  push(id: string, s: Sample) {
    const list = this.#by.get(id) ?? [];
    list.push(s);
    if (list.length > this.max) list.splice(0, list.length - this.max);
    this.#by.set(id, list);
  }

  get(id: string): Sample[] {
    return this.#by.get(id) ?? [];
  }

  /** Forget what is not in `ids` (containers that are gone). */
  keep(ids: Set<string>) {
    for (const k of this.#by.keys()) if (!ids.has(k)) this.#by.delete(k);
  }

  clear() {
    this.#by.clear();
  }
}

// -- inspect -----------------------------------------------------------------------------------------------------------

type Json = Record<string, unknown>;
const obj = (v: unknown): Json => (v && typeof v === "object" && !Array.isArray(v) ? (v as Json) : {});
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string | null => (typeof v === "string" && v !== "" ? v : null);
/** Docker prints "0001-01-01T00:00:00Z" for a time that never happened. */
const when = (v: unknown): string | null => {
  const s = str(v);
  return s && !s.startsWith("0001-01-01") ? s : null;
};

export interface InspectSummary {
  created: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  exitCode: number | null;
  error: string | null;
  healthStatus: string | null;
  restartPolicy: string | null;
  entrypoint: string | null;
  command: string | null;
  workingDir: string | null;
  user: string | null;
  envCount: number;
  published: { hostIp: string; hostPort: string; containerPort: string; proto: string }[];
  exposed: string[];
  networks: { name: string; ip: string | null; gateway: string | null; aliases: string[] }[];
  mounts: { type: string; name: string | null; source: string; destination: string; rw: boolean }[];
  composeProject: string | null;
  composeService: string | null;
}

/** What the Overview tab shows, read from `inspect` JSON (one container, or a one-element array). */
export function summarizeInspect(raw: unknown): InspectSummary {
  const root = obj(Array.isArray(raw) ? raw[0] : raw);
  const state = obj(root.State);
  const config = obj(root.Config);
  const host = obj(root.HostConfig);
  const net = obj(root.NetworkSettings);
  const labels = obj(config.Labels);
  const join = (v: unknown) => {
    const parts = arr(v).filter((x): x is string => typeof x === "string");
    return parts.length ? parts.join(" ") : null;
  };
  const policy = obj(host.RestartPolicy);
  const policyName = str(policy.Name);
  const published: InspectSummary["published"] = [];
  for (const [key, bindings] of Object.entries(obj(net.Ports))) {
    const [port, proto = "tcp"] = key.split("/");
    for (const b of arr(bindings)) published.push({ hostIp: str(obj(b).HostIp) ?? "", hostPort: str(obj(b).HostPort) ?? "", containerPort: port, proto });
  }
  const publishedKeys = new Set(published.map((p) => `${p.containerPort}/${p.proto}`));
  const exposed = Object.keys(obj(config.ExposedPorts)).filter((k) => !publishedKeys.has(k));
  const networks = Object.entries(obj(net.Networks)).map(([name, n]) => ({
    name,
    ip: str(obj(n).IPAddress),
    gateway: str(obj(n).Gateway),
    aliases: arr(obj(n).Aliases).filter((a): a is string => typeof a === "string"),
  }));
  const mounts = arr(root.Mounts).map((m) => ({ type: str(obj(m).Type) ?? "", name: str(obj(m).Name), source: str(obj(m).Source) ?? "", destination: str(obj(m).Destination) ?? "", rw: obj(m).RW !== false }));
  const code = state.ExitCode;
  return {
    created: when(root.Created),
    startedAt: when(state.StartedAt),
    finishedAt: when(state.FinishedAt),
    exitCode: typeof code === "number" && state.Running !== true ? code : null,
    error: str(state.Error),
    healthStatus: str(obj(state.Health).Status),
    restartPolicy: policyName ? (typeof policy.MaximumRetryCount === "number" && policy.MaximumRetryCount > 0 ? `${policyName} (max ${policy.MaximumRetryCount})` : policyName) : null,
    entrypoint: join(config.Entrypoint),
    command: join(config.Cmd),
    workingDir: str(config.WorkingDir),
    user: str(config.User),
    envCount: arr(config.Env).length,
    published,
    exposed,
    networks,
    mounts,
    composeProject: str(labels["com.docker.compose.project"]) ?? str(labels["io.podman.compose.project"]),
    composeService: str(labels["com.docker.compose.service"]) ?? str(labels["io.podman.compose.service"]),
  };
}

const SECRET_KEY = /(pass(word|wd)?|secret|token|api[_-]?key|private[_-]?key|credential|auth|cert|dsn|connection[_-]?string)/i;
export const REDACTED = "••••••••";

/** True for an environment variable that probably holds a secret. */
export const looksSecret = (name: string) => SECRET_KEY.test(name);

/**
 * `inspect` JSON with environment values that look like secrets replaced, unless `reveal`. Only `Config.Env` is
 * touched: the output is otherwise exactly what the runtime printed.
 */
export function redactInspect<T>(raw: T, reveal: boolean): T {
  if (reveal) return raw;
  const copy = JSON.parse(JSON.stringify(raw)) as unknown;
  for (const item of Array.isArray(copy) ? copy : [copy]) {
    const config = obj(obj(item).Config);
    if (Array.isArray(config.Env)) {
      config.Env = config.Env.map((e) => {
        if (typeof e !== "string") return e;
        const at = e.indexOf("=");
        return at > 0 && looksSecret(e.slice(0, at)) ? `${e.slice(0, at)}=${REDACTED}` : e;
      });
    }
  }
  return copy as T;
}

// -- ports -------------------------------------------------------------------------------------------------------------

/** Ports that usually serve a web page; others can still be opened, but are not offered first. */
const WEB_PORTS = new Set([80, 443, 3000, 3001, 4000, 4200, 5000, 5173, 8000, 8008, 8080, 8081, 8088, 8443, 8888, 9000, 9090]);

export interface PortTarget {
  /** Where to connect, as seen from the container host. */
  address: string;
  hostPort: number;
  containerPort: number;
  /** Probably a web service. */
  web: boolean;
}

/** The published TCP ports of a container, to open or tunnel. Exposed-only and UDP ports are not offered. */
export function portTargets(ports: PortMapping[]): PortTarget[] {
  const seen = new Set<number>();
  const out: PortTarget[] = [];
  for (const p of ports) {
    if (!p.host_port || p.proto !== "tcp") continue;
    const hostPort = Number(p.host_port);
    const containerPort = Number(p.container_port);
    if (!Number.isInteger(hostPort) || seen.has(hostPort)) continue;
    seen.add(hostPort);
    // "Any address" is reached on the host's own loopback; a specific bind address is where it listens.
    const ip = ["", "0.0.0.0", "::", "[::]"].includes(p.host_ip) ? "127.0.0.1" : p.host_ip;
    out.push({ address: ip, hostPort, containerPort, web: WEB_PORTS.has(containerPort) || WEB_PORTS.has(hostPort) });
  }
  return out.sort((a, b) => Number(b.web) - Number(a.web) || a.hostPort - b.hostPort);
}

/** The URL for a port that is reached on this computer (this computer's own runtime, or a tunnel to the host). */
export function portUrl(localPort: number, containerPort: number): string {
  return `${containerPort === 443 || localPort === 443 || containerPort === 8443 ? "https" : "http"}://127.0.0.1:${localPort}`;
}

// -- errors -------------------------------------------------------------------------------------------------------------

export type FailureKind = "no-runtime" | "daemon" | "permission" | "disconnected" | "timeout" | "other";

export interface Failure {
  kind: FailureKind;
  title: string;
  text: string;
  /** What the person can try. */
  actions: ("retry" | "reconnect")[];
}

/** A useful sentence for what the runtime or the connection said, and what to try. */
export function classifyFailure(message: string): Failure {
  const m = message.toLowerCase();
  if (m.includes("no container runtime was found") || (m.includes("not installed") && !m.includes("compose")))
    return { kind: "no-runtime", title: "No container runtime found", text: "Docker, Podman or nerdctl isn't installed here, or isn't on this user's PATH.", actions: ["retry"] };
  if (m.includes("permission denied"))
    return { kind: "permission", title: "Permission denied", text: "This user isn't allowed to use the container runtime. Add it to the docker group (then sign in again), or use rootless Docker or Podman.", actions: ["retry"] };
  if (m.includes("cannot connect to the docker daemon") || m.includes("daemon") || m.includes("podman.sock") || m.includes("cannot connect to podman"))
    return { kind: "daemon", title: "The runtime isn't answering", text: "Unable to communicate with the container daemon on this host. It may not be running.", actions: ["retry", "reconnect"] };
  if (m.includes("no_session") || m.includes("no session") || m.includes("connection") && (m.includes("lost") || m.includes("closed") || m.includes("refused") || m.includes("reset")) || m.includes("disconnected") || m.includes("broken pipe"))
    return { kind: "disconnected", title: "The host is disconnected", text: "The SSH connection to this host was lost.", actions: ["reconnect"] };
  if (m.includes("timed out") || m.includes("timeout")) return { kind: "timeout", title: "The host took too long to answer", text: "The command timed out. The host may be busy or unreachable.", actions: ["retry", "reconnect"] };
  return { kind: "other", title: "Couldn't read the containers", text: message, actions: ["retry"] };
}

/** The Compose project's key for remembering whether it is expanded: per host, so two hosts don't share it. */
export const collapseKey = (sourceKey: string, project: string | null) => `${sourceKey}\u0000${project === null ? "\u0001none" : `p:${project}`}`;
