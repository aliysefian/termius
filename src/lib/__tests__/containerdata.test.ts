import { describe, expect, it } from "vitest";
import {
  LogBuffer,
  composeProject,
  countByState,
  filterContainers,
  filterImages,
  formatAge,
  filterNetworks,
  filterVolumes,
  formatPorts,
  groupByProject,
  imageRef,
  isLive,
  isPullable,
  matchingLines,
  portLabels,
  pruneProjects,
  shellCommand,
  splitMatches,
  stateTone,
  summarizePrune,
  usedByText,
  healthOf,
} from "../containerdata";
import type { ContainerImage, ContainerInfo, ContainerNetwork, ContainerVolume, PortMapping, PruneItem } from "../types";

const port = (host_ip: string, host_port: string, container_port: string, proto = "tcp"): PortMapping => ({ host_ip, host_port, container_port, proto });

const box = (o: Partial<ContainerInfo>): ContainerInfo => ({
  id: "a".repeat(64),
  name: "web",
  image: "nginx:1.27",
  state: "running",
  status: "Up 2 hours",
  ports: [],
  created: 1_700_000_000,
  size: null,
  labels: {},
  command: "",
  pod: null,
  mounts: [],
  networks: [],
  ...o,
});

describe("ports", () => {
  it("shows an IPv4 and IPv6 publish of the same port once", () => {
    const l = portLabels([port("0.0.0.0", "58432", "5432"), port("::", "58432", "5432")]);
    expect(l).toEqual([{ text: "0.0.0.0:58432→5432/tcp", published: true }]);
  });
  it("keeps a specific address, and lists published before exposed", () => {
    expect(formatPorts([port("", "", "8080"), port("127.0.0.1", "58080", "8080")])).toBe("127.0.0.1:58080→8080/tcp, 8080/tcp");
  });
  it("keeps ranges and protocols apart", () => {
    expect(formatPorts([port("0.0.0.0", "8000-8002", "8000-8002"), port("", "", "53", "udp")])).toBe("0.0.0.0:8000-8002→8000-8002/tcp, 53/udp");
    expect(formatPorts([port("0.0.0.0", "53", "53", "tcp"), port("0.0.0.0", "53", "53", "udp")])).toBe("0.0.0.0:53→53/tcp, 0.0.0.0:53→53/udp");
  });
  it("treats an empty list and an empty address", () => {
    expect(formatPorts([])).toBe("");
    expect(formatPorts([port("", "80", "80")])).toBe("0.0.0.0:80→80/tcp");
  });
});

describe("age", () => {
  const now = 1_700_000_000_000;
  it("reads coarsely", () => {
    expect(formatAge(1_700_000_000 - 5, now)).toBe("5s");
    expect(formatAge(1_700_000_000 - 90, now)).toBe("1m");
    expect(formatAge(1_700_000_000 - 3 * 3600, now)).toBe("3h");
    expect(formatAge(1_700_000_000 - 2 * 86400, now)).toBe("2d");
    expect(formatAge(1_700_000_000 - 21 * 86400, now)).toBe("3w");
    expect(formatAge(1_700_000_000 - 90 * 86400, now)).toBe("3mo");
    expect(formatAge(1_700_000_000 - 800 * 86400, now)).toBe("2y");
  });
  it("copes with a missing time or a clock behind the host's", () => {
    expect(formatAge(null, now)).toBe("—");
    expect(formatAge(1_700_000_000 + 500, now)).toBe("0s");
  });
});

describe("states", () => {
  it("colours and groups them", () => {
    expect(stateTone("running")).toBe("success");
    expect(stateTone("paused")).toBe("warning");
    expect(stateTone("dead")).toBe("danger");
    expect(stateTone("exited")).toBe("neutral");
    expect(isLive("restarting")).toBe(true);
    expect(isLive("created")).toBe(false);
  });
  it("counts running and stopped", () => {
    const list = [box({ state: "running" }), box({ state: "paused" }), box({ state: "exited" }), box({ state: "created" })];
    expect(countByState(list)).toEqual({ all: 4, running: 1, stopped: 2 });
  });
});

describe("filtering", () => {
  const list = [
    box({ name: "web", image: "nginx:1.27", ports: [port("0.0.0.0", "8080", "80")], labels: { "com.docker.compose.project": "shop" } }),
    box({ name: "db", image: "postgres:16", state: "exited", status: "Exited (0) 1h ago", ports: [port("", "", "5432")], labels: { tier: "data" } }),
    box({ name: "worker", image: "shop/worker", state: "paused", pod: "jobs" }),
  ];
  it("matches name, image, port, label, status and pod, all words required", () => {
    const names = (q: string, s: "all" | "running" | "stopped" = "all") => filterContainers(list, q, s).map((c) => c.name);
    expect(names("")).toEqual(["web", "db", "worker"]);
    expect(names("nginx")).toEqual(["web"]);
    expect(names("8080")).toEqual(["web"]);
    expect(names("tier=data")).toEqual(["db"]);
    expect(names("exited")).toEqual(["db"]);
    expect(names("jobs")).toEqual(["worker"]);
    expect(names("shop")).toEqual(["web", "worker"]);
    expect(names("shop worker")).toEqual(["worker"]);
    expect(names("NGINX 1.27")).toEqual(["web"]);
    expect(names("nope")).toEqual([]);
  });
  it("narrows by state", () => {
    expect(filterContainers(list, "", "running").map((c) => c.name)).toEqual(["web"]);
    expect(filterContainers(list, "", "stopped").map((c) => c.name)).toEqual(["db"]);
    expect(filterContainers(list, "shop", "stopped")).toEqual([]);
  });
  it("filters images", () => {
    const imgs: ContainerImage[] = [
      { id: "abc123", repository: "nginx", tag: "1.27", size_text: "73MB", size_bytes: 73e6, created: null, containers: 1 },
      { id: "def456", repository: "postgres", tag: "16", size_text: "642MB", size_bytes: 642e6, created: null, containers: 0 },
    ];
    expect(filterImages(imgs, "postgres:16")).toHaveLength(1);
    expect(filterImages(imgs, "def4")).toHaveLength(1);
    expect(filterImages(imgs, "")).toHaveLength(2);
  });
  it("finds a compose project", () => {
    expect(composeProject(list[0])).toBe("shop");
    expect(composeProject(list[1])).toBeNull();
  });
});

describe("shell command", () => {
  it("is bash if present, else sh", () => {
    expect(shellCommand("docker", "fd4c24")).toBe("docker exec -it fd4c24 sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'");
    expect(shellCommand("podman", "web")?.startsWith("podman exec -it web ")).toBe(true);
  });
  it("never types a reference that could carry another command", () => {
    for (const bad of ["", "a b", "a;b", "$(x)", "`x`", "-it", "a\nb", "a'b", "x&&y", "x|y", "a".repeat(201)]) expect(shellCommand("docker", bad)).toBeNull();
  });
});

describe("log buffer", () => {
  it("splits chunks into lines and holds the unfinished one", () => {
    const b = new LogBuffer();
    expect(b.push("one\ntw")).toBe(1);
    expect(b.lines).toEqual(["one"]);
    expect(b.partial).toBe("tw");
    expect(b.push("o\nthree\n")).toBe(2);
    expect(b.lines).toEqual(["one", "two", "three"]);
    expect(b.partial).toBe("");
    b.push("tail");
    expect(b.all()).toEqual(["one", "two", "three", "tail"]);
  });
  it("hides secrets in complete lines only when asked, and drops a key block whole", () => {
    const off = new LogBuffer();
    off.push("password=hunter2\n");
    expect(off.lines).toEqual(["password=hunter2"]);
    const on = new LogBuffer();
    on.mask = true;
    on.push("db password=hun");
    on.push("ter2\nok\n-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\ndone\n");
    expect(on.lines).toEqual(["db password=[hidden]", "ok", "[private key hidden]", "done"]);
  });
  it("reads a terminal's CRLF as one line break", () => {
    const b = new LogBuffer();
    b.push("a\r\nb\r\n");
    expect(b.lines).toEqual(["a", "b"]);
    // A CR at the end of one chunk and the LF at the start of the next.
    const c = new LogBuffer();
    c.push("x\r");
    c.push("\ny\n");
    expect(c.lines).toEqual(["x", "y"]);
  });
  it("lets a carriage return overwrite, so progress reads as its last state", () => {
    const b = new LogBuffer();
    b.push("10%\r50%\r100%\ndone\n");
    expect(b.lines).toEqual(["100%", "done"]);
  });
  it("removes colour codes and keeps the text", () => {
    const b = new LogBuffer();
    b.push("\x1b[31mERROR\x1b[0m something \x1b[1;32mok\x1b[0m\n");
    expect(b.lines).toEqual(["ERROR something ok"]);
  });
  it("keeps only the newest lines and says how many it dropped", () => {
    const b = new LogBuffer(3);
    b.push("1\n2\n3\n4\n5\n");
    expect(b.lines).toEqual(["3", "4", "5"]);
    expect(b.dropped).toBe(2);
    b.clear();
    expect(b.lines).toEqual([]);
    expect(b.dropped).toBe(0);
  });
  it("handles empty lines, empty chunks and unicode", () => {
    const b = new LogBuffer();
    b.push("");
    b.push("\n\nhé✓\n");
    expect(b.lines).toEqual(["", "", "hé✓"]);
  });
  it("copes with a very large burst", () => {
    const b = new LogBuffer(1000);
    const t = performance.now();
    b.push(Array.from({ length: 200_000 }, (_, i) => `line ${i}`).join("\n") + "\n");
    expect(performance.now() - t).toBeLessThan(2000);
    expect(b.lines).toHaveLength(1000);
    expect(b.lines[999]).toBe("line 199999");
    expect(b.dropped).toBe(199_000);
  });
});

describe("log search", () => {
  const lines = ["GET /health 200", "ERROR db timeout", "get /users 404", "error again"];
  it("finds lines case-insensitively", () => {
    expect(matchingLines(lines, "error")).toEqual([1, 3]);
    expect(matchingLines(lines, "GET")).toEqual([0, 2]);
    expect(matchingLines(lines, "")).toEqual([]);
    expect(matchingLines(lines, "zzz")).toEqual([]);
  });
  it("splits a line around its matches", () => {
    expect(splitMatches("a error b ERROR c", "error")).toEqual([
      { text: "a ", hit: false },
      { text: "error", hit: true },
      { text: " b ", hit: false },
      { text: "ERROR", hit: true },
      { text: " c", hit: false },
    ]);
    expect(splitMatches("abc", "")).toEqual([{ text: "abc", hit: false }]);
    expect(splitMatches("abc", "x")).toEqual([{ text: "abc", hit: false }]);
    expect(splitMatches("", "x")).toEqual([{ text: "", hit: false }]);
    expect(splitMatches("aaa", "a").filter((p) => p.hit)).toHaveLength(3);
  });
});

describe("compose groups", () => {
  const svc = (name: string, project: string | null, service: string, state: ContainerInfo["state"] = "running") =>
    box({
      name,
      state,
      labels: project
        ? { "com.docker.compose.project": project, "com.docker.compose.service": service, "com.docker.compose.project.working_dir": "/srv/" + project, "com.docker.compose.project.config_files": `/srv/${project}/compose.yaml` }
        : {},
    });
  const list = [svc("shop-web-1", "shop", "web"), svc("loose", null, ""), svc("blog-db-1", "blog", "db", "exited"), svc("shop-cache-1", "shop", "cache"), svc("also-loose", null, "")];

  it("gathers by project, projects first and alphabetical, the rest last", () => {
    const g = groupByProject(list);
    expect(g.map((x) => x.project)).toEqual(["blog", "shop", null]);
    expect(g[1].containers.map((c) => c.name)).toEqual(["shop-cache-1", "shop-web-1"]);
    expect(g[2].containers.map((c) => c.name)).toEqual(["loose", "also-loose"]);
  });
  it("counts running and reads where Compose was run", () => {
    const g = groupByProject(list);
    expect(g[0]).toMatchObject({ running: 0, workingDir: "/srv/blog", configFiles: "/srv/blog/compose.yaml" });
    expect(g[1].running).toBe(2);
    expect(g[2]).toMatchObject({ workingDir: null, configFiles: null });
  });
  it("copes with nothing and with everything in one group", () => {
    expect(groupByProject([])).toEqual([]);
    expect(groupByProject([svc("a", null, "")]).map((x) => x.project)).toEqual([null]);
  });
});

describe("volumes and networks", () => {
  const vol = (name: string, used_by: string[] = [], labels: Record<string, string> = {}): ContainerVolume => ({ name, driver: "local", mountpoint: "/x", scope: "local", labels, size: null, used_by });
  const net = (name: string, used_by: string[] = []): ContainerNetwork => ({ id: "abc123def456", name, driver: "bridge", scope: "local", internal: false, ipv6: false, created: null, labels: {}, predefined: name === "bridge", used_by });

  it("searches name, driver, users and labels", () => {
    const vols = [vol("pgdata", ["db"], { "com.docker.compose.project": "shop" }), vol("cache")];
    expect(filterVolumes(vols, "pg").map((v) => v.name)).toEqual(["pgdata"]);
    expect(filterVolumes(vols, "db").map((v) => v.name)).toEqual(["pgdata"]);
    expect(filterVolumes(vols, "project=shop").map((v) => v.name)).toEqual(["pgdata"]);
    expect(filterVolumes(vols, "").length).toBe(2);
    const nets = [net("shop_default", ["web"]), net("bridge")];
    expect(filterNetworks(nets, "web").map((n) => n.name)).toEqual(["shop_default"]);
    expect(filterNetworks(nets, "abc123").length).toBe(2);
    expect(filterNetworks(nets, "zzz")).toEqual([]);
  });
  it("says who uses something", () => {
    expect(usedByText([])).toBe("unused");
    expect(usedByText(["a"])).toBe("a");
    expect(usedByText(["a", "b", "c", "d"])).toBe("a, b +2");
  });
});

describe("pruning", () => {
  const item = (id: string, project: string | null = null): PruneItem => ({ id, label: id, detail: "", project });
  it("names the Compose projects a prune would break up", () => {
    expect(pruneProjects([item("a", "shop"), item("b"), item("c", "shop"), item("d", "blog")])).toEqual(["blog", "shop"]);
    expect(pruneProjects([item("a")])).toEqual([]);
    expect(pruneProjects([])).toEqual([]);
  });
  it("separates what went from what didn't", () => {
    const r = summarizePrune([
      { id: "a", ok: true, error: null },
      { id: "b", ok: false, error: "in use" },
      { id: "c", ok: true, error: null },
    ]);
    expect(r.removed).toBe(2);
    expect(r.failed).toEqual([{ id: "b", ok: false, error: "in use" }]);
    expect(summarizePrune([])).toEqual({ removed: 0, failed: [] });
  });
  it("only pulls plain image references", () => {
    for (const ok of ["nginx", "nginx:1.27", " nginx:1.27 ", "ghcr.io/org/app@sha256:abc", "localhost:5000/team/app:v2"]) expect(isPullable(ok), ok).toBe(true);
    for (const bad of ["", "  ", "--all-tags", "-q", "a b", "a;b", "$(x)", "a\nb", "a".repeat(201)]) expect(isPullable(bad), bad).toBe(false);
  });
});

describe("image references", () => {
  const img = (repository: string, tag: string): ContainerImage => ({ id: "abc123def456", repository, tag, size_text: "1MB", size_bytes: 1e6, created: null, containers: 0 });
  it("names a tagged image by its tag and an untagged one by its ID", () => {
    expect(imageRef(img("nginx", "1.27"))).toBe("nginx:1.27");
    expect(imageRef(img("localhost:5000/team/app", "v2"))).toBe("localhost:5000/team/app:v2");
    expect(imageRef(img("<none>", "<none>"))).toBe("abc123def456");
    expect(imageRef(img("", ""))).toBe("abc123def456");
    expect(imageRef(img("nginx", "<none>"))).toBe("abc123def456");
  });
});

describe("health", () => {
  it("reads the container's own health check from its status text", () => {
    expect(healthOf("Up 3 hours (healthy)")).toBe("healthy");
    expect(healthOf("Up 3 hours (unhealthy)")).toBe("unhealthy");
    expect(healthOf("Up 5 seconds (health: starting)")).toBe("starting");
    expect(healthOf("Up 3 hours (Paused)")).toBeNull();
    expect(healthOf("Exited (0) 2 days ago")).toBeNull();
    expect(healthOf("")).toBeNull();
  });
});
