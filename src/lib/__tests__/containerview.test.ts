import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_COLUMNS, REDACTED, StatHistory, classifyFailure, collapseKey, exitCode, formatBytes, groupSummaries, health, isFailed, looksSecret, matchesStatus, menuItems, metrics, parseBytes, parsePair, portTargets, portUrl, projectState, redactInspect, rowPrimary, searchContainers, statusCounts, summarizeInspect, toSample, visibleColumns } from "../containerview";
import type { ContainerInfo, ContainerState } from "../types";

const c = (name: string, state: ContainerState, status: string, extra: Partial<ContainerInfo> = {}): ContainerInfo => ({
  id: name.padEnd(64, "0"),
  name,
  image: "nginx:1.27",
  state,
  status,
  ports: [],
  created: 1_700_000_000,
  size: null,
  labels: {},
  command: "",
  pod: null,
  mounts: [],
  networks: [],
  ...extra,
});

const list = [
  c("web", "running", "Up 3 hours (healthy)", { labels: { "com.docker.compose.project": "shop", "com.docker.compose.service": "web" }, ports: [{ host_ip: "0.0.0.0", host_port: "8080", container_port: "80", proto: "tcp" }] }),
  c("db", "running", "Up 3 hours (unhealthy)", { labels: { "com.docker.compose.project": "shop", "com.docker.compose.service": "db" } }),
  c("cache", "running", "Up 3 hours"),
  c("batch", "exited", "Exited (0) 2 days ago"),
  c("crashed", "exited", "Exited (137) 5 minutes ago"),
  c("frozen", "paused", "Up 1 hour (Paused)"),
  c("flapping", "restarting", "Restarting (1) 4 seconds ago"),
  c("fresh", "created", "Created"),
  c("gone", "dead", "Dead"),
];

describe("state and health", () => {
  it("reads the exit code, and calls only a non-zero exit or a dead container failed", () => {
    expect(exitCode("Exited (137) 5 minutes ago")).toBe(137);
    expect(exitCode("Exited (0) 2 days ago")).toBe(0);
    expect(exitCode("Up 3 hours")).toBeNull();
    expect(list.filter(isFailed).map((x) => x.name)).toEqual(["crashed", "gone"]);
  });

  it("never calls a container without a health check healthy", () => {
    expect(health(list[0])).toBe("healthy");
    expect(health(list[1])).toBe("unhealthy");
    expect(health(list[2])).toBeNull();
  });

  it("counts total, running, stopped and unhealthy from the real states", () => {
    expect(metrics(list)).toEqual({ total: 9, running: 3, stopped: 4, unhealthy: 1 });
    expect(metrics([])).toEqual({ total: 0, running: 0, stopped: 0, unhealthy: 0 });
  });

  it("filters by each status and counts them", () => {
    const names = (f: Parameters<typeof matchesStatus>[1]) => list.filter((x) => matchesStatus(x, f)).map((x) => x.name);
    expect(names("running")).toEqual(["web", "db", "cache"]);
    expect(names("stopped")).toEqual(["batch", "crashed", "fresh", "gone"]);
    expect(names("paused")).toEqual(["frozen"]);
    expect(names("restarting")).toEqual(["flapping"]);
    expect(names("failed")).toEqual(["crashed", "gone"]);
    expect(names("unhealthy")).toEqual(["db"]);
    expect(names("all")).toHaveLength(9);
    expect(statusCounts(list).running).toBe(3);
  });
});

describe("search", () => {
  it("finds by name, id, image, project, service, port and label, together with the status filter", () => {
    const names = (q: string, s: Parameters<typeof searchContainers>[2] = "all") => searchContainers(list, q, s).map((x) => x.name);
    expect(names("web")).toEqual(["web"]);
    expect(names("shop")).toEqual(["web", "db"]);
    expect(names("service=db")).toEqual(["db"]);
    expect(names("8080")).toEqual(["web"]);
    expect(names("nginx", "paused")).toEqual(["frozen"]);
    expect(names(list[3].id.slice(0, 12))).toEqual(["batch"]);
    expect(names("shop", "unhealthy")).toEqual(["db"]);
    expect(names("zzz")).toEqual([]);
  });
});

describe("compose groups", () => {
  it("summarises a project's running count and unhealthy containers, with the standalone ones last", () => {
    const g = groupSummaries(list);
    expect(g.map((x) => x.project)).toEqual(["shop", null]);
    expect([g[0].running, g[0].containers.length, g[0].unhealthy]).toEqual([2, 2, 1]);
    expect(projectState(g[0])).toBe("running");
    expect(projectState({ running: 1, containers: [1, 2] })).toBe("partial");
    expect(projectState({ running: 0, containers: [1] })).toBe("stopped");
  });

  it("remembers expansion per host", () => {
    expect(collapseKey("a", "shop")).not.toBe(collapseKey("b", "shop"));
    expect(collapseKey("a", null)).not.toBe(collapseKey("a", ""));
  });
});

describe("actions", () => {
  const on = (x: ContainerInfo, busy = false) => Object.fromEntries(menuItems(x, busy).map((m) => [m.id, m.enabled]));

  it("offers only what makes sense in each state, and says why not", () => {
    expect(on(list[2])).toMatchObject({ start: false, stop: true, restart: true, pause: true, resume: false, terminal: true, delete: true, logs: true, inspect: true });
    expect(on(list[3])).toMatchObject({ start: true, stop: false, restart: false, pause: false, resume: false, terminal: false });
    expect(on(list[5])).toMatchObject({ start: false, stop: true, restart: false, pause: false, resume: true, terminal: false });
    expect(menuItems(list[3], false).find((m) => m.id === "stop")?.why).toBe("Not running");
    expect(menuItems(list[2], false).find((m) => m.id === "start")?.why).toBe("Already running");
  });

  it("switches everything that acts off while an operation is underway, but not logs or copying", () => {
    const m = on(list[2], true);
    expect([m.start, m.stop, m.restart, m.pause, m.terminal, m.delete]).toEqual([false, false, false, false, false, false]);
    expect([m.logs, m["copy-id"], m["copy-name"], m.inspect]).toEqual([true, true, true, true]);
  });

  it("puts delete last, as the one danger", () => {
    const items = menuItems(list[0], false);
    expect(items.at(-1)).toMatchObject({ id: "delete", danger: true });
    expect(items.filter((i) => i.danger)).toHaveLength(1);
  });

  it("shows stop for live, resume for paused and start for stopped as the row's main action", () => {
    expect([rowPrimary(list[2]), rowPrimary(list[5]), rowPrimary(list[3]), rowPrimary(list[6])]).toEqual(["stop", "resume", "start", "stop"]);
  });
});

describe("columns", () => {
  it("drops the least important columns first as the table narrows", () => {
    const all = COLUMNS.map((x) => x.id);
    expect(visibleColumns(all, 1600, true, true)).toEqual(["image", "ports", "age", "cpu", "memory", "size", "created"]);
    expect(visibleColumns(all, 1250, true, true)).toEqual(["image", "ports", "age", "cpu"]);
    expect(visibleColumns(all, 900, true, true)).toEqual(["image"]);
    expect(visibleColumns(all, 700, true, true)).toEqual([]);
  });

  it("shows CPU and memory only while they are being read, and size only when asked for", () => {
    expect(visibleColumns(DEFAULT_COLUMNS, 1600, false, false)).toEqual(["image", "ports", "age"]);
    expect(visibleColumns(["cpu", "size"], 1600, true, false)).toEqual(["cpu"]);
    expect(visibleColumns(["image"], 1600, false, false)).toEqual(["image"]);
  });
});

describe("stats", () => {
  it("reads sizes in either family of units, and leaves the unreadable as unknown", () => {
    expect(parseBytes("12MiB")).toBe(12 * 1024 ** 2);
    expect(parseBytes("1.5kB")).toBe(1500);
    expect(parseBytes("0B")).toBe(0);
    expect(parseBytes("--")).toBeNull();
    expect(parsePair("1.2kB / 3MB")).toEqual([1200, 3e6]);
    expect(parsePair("--")).toEqual([null, null]);
    expect(formatBytes(null)).toBe("–");
    expect(formatBytes(1536)).toBe("1.5 KiB");
  });

  it("turns a reading into a sample without inventing what the runtime didn't say", () => {
    const s = toSample({ id: "a", name: "a", cpu_pct: null, mem_usage: "10MiB / 1GiB", mem_pct: 0.9, net_io: "--", block_io: "1kB / 2kB", pids: null }, 5);
    expect(s).toMatchObject({ at: 5, cpu: null, memUsed: 10 * 1024 ** 2, memLimit: 1024 ** 3, netRx: null, blockWrite: 2000, pids: null });
  });

  it("keeps a bounded history per container and forgets the ones that are gone", () => {
    const h = new StatHistory(3);
    const sample = (at: number) => toSample({ id: "a", name: "a", cpu_pct: at, mem_usage: "1B / 2B", mem_pct: 1, net_io: "0B / 0B", block_io: "0B / 0B", pids: 1 }, at);
    for (let i = 0; i < 10; i++) h.push("a", sample(i));
    h.push("b", sample(1));
    expect(h.get("a").map((s) => s.at)).toEqual([7, 8, 9]);
    h.keep(new Set(["a"]));
    expect(h.get("b")).toEqual([]);
    expect(h.get("nope")).toEqual([]);
  });
});

describe("inspect", () => {
  const raw = [
    {
      Created: "2026-10-01T10:00:00Z",
      State: { Status: "running", Running: true, ExitCode: 0, StartedAt: "2026-10-01T10:00:05Z", FinishedAt: "0001-01-01T00:00:00Z", Health: { Status: "healthy" }, Error: "" },
      Config: { Entrypoint: ["/docker-entrypoint.sh"], Cmd: ["nginx", "-g", "daemon off;"], WorkingDir: "/app", User: "", Env: ["PATH=/usr/bin", "DB_PASSWORD=hunter2", "API_TOKEN=abc", "LOG_LEVEL=info"], ExposedPorts: { "80/tcp": {}, "9000/tcp": {} }, Labels: { "com.docker.compose.project": "shop", "com.docker.compose.service": "web" } },
      HostConfig: { RestartPolicy: { Name: "on-failure", MaximumRetryCount: 3 } },
      NetworkSettings: { Ports: { "80/tcp": [{ HostIp: "0.0.0.0", HostPort: "8080" }], "9000/tcp": null }, Networks: { shop_default: { IPAddress: "172.18.0.2", Gateway: "172.18.0.1", Aliases: ["web"] } } },
      Mounts: [{ Type: "volume", Name: "data", Source: "/var/lib/docker/volumes/data/_data", Destination: "/data", RW: true }, { Type: "bind", Source: "/srv/conf", Destination: "/etc/conf", RW: false }],
    },
  ];

  it("summarises what the Overview shows, leaving a missing value missing", () => {
    const s = summarizeInspect(raw);
    expect(s).toMatchObject({ startedAt: "2026-10-01T10:00:05Z", finishedAt: null, exitCode: null, healthStatus: "healthy", restartPolicy: "on-failure (max 3)", entrypoint: "/docker-entrypoint.sh", command: "nginx -g daemon off;", workingDir: "/app", user: null, envCount: 4, composeProject: "shop", composeService: "web" });
    expect(s.published).toEqual([{ hostIp: "0.0.0.0", hostPort: "8080", containerPort: "80", proto: "tcp" }]);
    expect(s.exposed).toEqual(["9000/tcp"]);
    expect(s.networks).toEqual([{ name: "shop_default", ip: "172.18.0.2", gateway: "172.18.0.1", aliases: ["web"] }]);
    expect(s.mounts.map((m) => [m.type, m.name, m.destination, m.rw])).toEqual([["volume", "data", "/data", true], ["bind", null, "/etc/conf", false]]);
    expect(summarizeInspect({}).envCount).toBe(0);
    expect(summarizeInspect(null).published).toEqual([]);
  });

  it("hides secret-looking environment values by default, and changes nothing else", () => {
    const hidden = redactInspect(raw, false);
    const env = (hidden as typeof raw)[0].Config.Env;
    expect(env).toEqual(["PATH=/usr/bin", `DB_PASSWORD=${REDACTED}`, `API_TOKEN=${REDACTED}`, "LOG_LEVEL=info"]);
    expect(JSON.stringify(hidden)).not.toContain("hunter2");
    expect({ ...(hidden as typeof raw)[0], Config: { ...(hidden as typeof raw)[0].Config, Env: [] } }).toEqual({ ...raw[0], Config: { ...raw[0].Config, Env: [] } });
    expect(raw[0].Config.Env[1]).toBe("DB_PASSWORD=hunter2");
    expect(redactInspect(raw, true)).toBe(raw);
    expect(looksSecret("DB_PASSWORD") && looksSecret("aws_secret_access_key") && !looksSecret("LOG_LEVEL")).toBe(true);
  });
});

describe("ports", () => {
  it("offers published TCP ports, web ones first, and reaches 'any address' on the host's loopback", () => {
    const t = portTargets([
      { host_ip: "0.0.0.0", host_port: "5432", container_port: "5432", proto: "tcp" },
      { host_ip: "::", host_port: "5432", container_port: "5432", proto: "tcp" },
      { host_ip: "0.0.0.0", host_port: "8080", container_port: "80", proto: "tcp" },
      { host_ip: "10.0.0.5", host_port: "9100", container_port: "9100", proto: "tcp" },
      { host_ip: "0.0.0.0", host_port: "53", container_port: "53", proto: "udp" },
      { host_ip: "", host_port: "", container_port: "9000", proto: "tcp" },
    ]);
    expect(t.map((x) => [x.hostPort, x.address, x.web])).toEqual([[8080, "127.0.0.1", true], [5432, "127.0.0.1", false], [9100, "10.0.0.5", false]]);
    expect(portUrl(8080, 80)).toBe("http://127.0.0.1:8080");
    expect(portUrl(8443, 443)).toBe("https://127.0.0.1:8443");
  });
});

describe("failures", () => {
  it("explains what is wrong and what to try", () => {
    expect(classifyFailure("No container runtime was found here").kind).toBe("no-runtime");
    expect(classifyFailure("Got permission denied while trying to connect to the Docker daemon socket").kind).toBe("permission");
    expect(classifyFailure("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?").kind).toBe("daemon");
    expect(classifyFailure("no_session").kind).toBe("disconnected");
    expect(classifyFailure("the command timed out").kind).toBe("timeout");
    const other = classifyFailure("something odd");
    expect([other.kind, other.text]).toEqual(["other", "something odd"]);
    expect(classifyFailure("Cannot connect to the Docker daemon").actions).toContain("reconnect");
  });
});
