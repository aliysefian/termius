import { describe, expect, it } from "vitest";
import {
  LogBuffer,
  composeProject,
  countByState,
  filterContainers,
  filterImages,
  formatAge,
  formatPorts,
  isLive,
  matchingLines,
  portLabels,
  shellCommand,
  splitMatches,
  stateTone,
} from "../containerdata";
import type { ContainerImage, ContainerInfo, PortMapping } from "../types";

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
