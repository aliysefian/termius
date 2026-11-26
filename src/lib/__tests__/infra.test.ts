import { describe, expect, it } from "vitest";
import { Inventory, NO_FILTERS, ageText, countText, freshnessOf, search, statusLine, summarize, FRESH_MS, LIVE_MS } from "../infra/inventory";
import { containerKey, engineKey, hostKey, portKey, tmuxKey, unitKey, plain } from "../infra/model";
import { containerStatus, dockerSnapshot, hostsSnapshot, systemdSnapshot, tmuxSnapshot, tmuxUnsupported, unitStatus, type HostInput } from "../infra/providers";
import { buildRows, groupKey } from "../infra/tree";
import { neighborhood, parentsOf } from "../infra/graph";
import { actionsFor } from "../infra/actions";
import { runBounded, SingleFlight } from "../infra/coordinator";
import type { ContainerInfo } from "../types";
import type { HostSnapshot } from "../tmux/monitor";

const host = (id: string, label: string, over: Partial<HostInput> = {}): HostInput => ({
  id,
  data: { label, hostname: `${label}.example`, port: 22, group: "Prod", tags: ["web"], protocol: "ssh" } as HostInput["data"],
  environment: "production",
  connected: false,
  lastConnectedAt: null,
  ...over,
});
const ctr = (id: string, name: string, over: Partial<ContainerInfo> = {}): ContainerInfo => ({ id, name, image: "nginx:1", state: "running", status: "Up 3 hours", ports: [], created: 1_700_000_000, size: null, labels: {}, command: "", pod: null, mounts: [], networks: ["bridge"], ...over });
const unit = (name: string, active = "active", sub = "running") => ({ name, load: "loaded", active, sub, description: `${name} desc`, enabled: "enabled", since: "" });
const tmuxSnap = (hostId: string, sessions: HostSnapshot["sessions"], over: Partial<HostSnapshot> = {}): HostSnapshot => ({ host_id: hostId, socket: { kind: "default" }, generation: 1, taken_at_ms: 1000, version: null, state: { kind: "ok" } as never, server: null, sessions, other_sockets: [], stale: null, ...over });
const envOf = (h: string | null) => (h === "h1" ? "production" : "staging");

describe("identity", () => {
  it("scopes every id by where the thing is", () => {
    expect(containerKey("h1", "docker", "abc")).not.toBe(containerKey("h2", "docker", "abc"));
    expect(containerKey("h1", "docker", "abc")).not.toBe(containerKey("h1", "podman", "abc"));
    expect(engineKey("h1", "docker")).not.toBe(engineKey("h1", "podman"));
    expect(unitKey("h1", "nginx.service")).not.toBe(unitKey("h2", "nginx.service"));
    expect(unitKey("h1", "x.service", "system")).not.toBe(unitKey("h1", "x.service", "user"));
    expect(tmuxKey("h1", "default", "$1")).not.toBe(tmuxKey("h1", "L:work", "$1"));
    expect(tmuxKey("h1", "default", "$1")).not.toBe(tmuxKey("h2", "default", "$1"));
  });
  it("cannot be made to collide by a separator in a name", () => {
    expect(unitKey("a:b", "c")).not.toBe(unitKey("a", "b:c"));
    expect(portKey("c", "tcp", "", 80)).not.toBe(portKey("c", "tcp", "0.0.0.0", 80));
  });
  it("shows a name as one line of plain text", () => {
    expect(plain("a\u001b[31mb\nc")).toBe("a [31mb c");
    expect(plain("x‮y")).toBe("x y");
  });
});

describe("freshness is separate from status", () => {
  const inv = new Inventory();
  inv.apply(dockerSnapshot("h1", "docker", [ctr("c1", "web")], 1_000_000, {}), 1_000_000);
  const state = () => inv.scope("docker:h1");
  it("is live, then fresh, then stale as time passes", () => {
    expect(freshnessOf(state(), 1_000_000 + LIVE_MS)).toBe("live");
    expect(freshnessOf(state(), 1_000_000 + LIVE_MS + 1)).toBe("fresh");
    expect(freshnessOf(state(), 1_000_000 + FRESH_MS + 1)).toBe("stale");
  });
  it("keeps what was known when a read fails, and calls it stale at once", () => {
    const i = new Inventory();
    i.apply(dockerSnapshot("h1", "docker", [ctr("c1", "web")], 100, {}), 100);
    i.fail("docker:h1", "docker", "h1", "timed out", 200);
    const c = i.resources(200).find((r) => r.type === "container")!;
    expect(c.status).toBe("running");
    expect(c.freshness).toBe("stale");
    expect(c.lastError).toBe("timed out");
    expect(statusLine(c, 200 + 7_200_000)).toMatch(/^Last known: Up 3 hours · 2 h ago$/);
  });
  it("says unavailable when nothing was ever read and unsupported when the host cannot have it", () => {
    const i = new Inventory();
    i.fail("systemd:h1", "systemd", "h1", "boom", 5);
    expect(freshnessOf(i.scope("systemd:h1"), 10)).toBe("unavailable");
    i.markUnsupported("systemd:h1", "systemd", "h1", "no systemd here", 6);
    expect(freshnessOf(i.scope("systemd:h1"), 10)).toBe("unsupported");
  });
  it("words the age", () => {
    expect(ageText(null, 0)).toBe("never");
    expect(ageText(1000, 3000)).toBe("just now");
    expect(ageText(0, 4000 + 1000)).toBe("5 s ago");
    expect(ageText(0, 90_000)).toBe("1 min ago");
  });
});

describe("reconciling a scope", () => {
  it("drops what is no longer there instead of keeping it as live", () => {
    const i = new Inventory();
    i.apply(dockerSnapshot("h1", "docker", [ctr("a", "one"), ctr("b", "two")], 1, {}), 1);
    expect(i.resources(1).filter((r) => r.type === "container")).toHaveLength(2);
    i.apply(dockerSnapshot("h1", "docker", [ctr("a", "one")], 2, {}), 2);
    expect(i.resources(2).filter((r) => r.type === "container").map((r) => r.name)).toEqual(["one"]);
  });
  it("keeps two hosts' same-named containers apart", () => {
    const i = new Inventory();
    i.apply(dockerSnapshot("h1", "docker", [ctr("a", "web")], 1, {}), 1);
    i.apply(dockerSnapshot("h2", "docker", [ctr("a", "web")], 1, {}), 1);
    expect(i.resources(1).filter((r) => r.type === "container")).toHaveLength(2);
  });
  it("one host failing does not touch another's data", () => {
    const i = new Inventory();
    i.apply(dockerSnapshot("h1", "docker", [ctr("a", "web")], 1, {}), 1);
    i.apply(dockerSnapshot("h2", "docker", [ctr("b", "db")], 1, {}), 1);
    i.fail("docker:h1", "docker", "h1", "down", 2);
    const by = Object.fromEntries(i.resources(2).filter((r) => r.type === "container").map((r) => [r.name, r.freshness]));
    expect(by).toEqual({ web: "stale", db: "live" });
  });
  it("drops an unsupported host's earlier answer, and a deleted host entirely", () => {
    const i = new Inventory();
    i.apply(systemdSnapshot("h1", [unit("a.service")], 1), 1);
    i.markUnsupported("systemd:h1", "systemd", "h1", "no systemd", 2);
    expect(i.resources(2)).toHaveLength(0);
    i.apply(tmuxSnapshot(tmuxSnap("h1", [])), 3);
    i.forgetHost("h1");
    expect(i.scopes.size).toBe(0);
  });
  it("keeps the first report of an id", () => {
    const i = new Inventory();
    const a = systemdSnapshot("h1", [unit("a.service")], 1);
    i.apply(a, 1);
    i.apply({ ...a, scopeKey: "systemd:dup", resources: a.resources.map((r) => ({ ...r, scopeKey: "systemd:dup", name: "copy" })) }, 1);
    expect(i.resources(1)).toHaveLength(1);
    expect(i.resources(1)[0].name).toBe("a.service");
  });
});

describe("providers", () => {
  it("maps container states and health to a status", () => {
    expect(containerStatus({ state: "running", status: "Up 3 hours" })).toBe("running");
    expect(containerStatus({ state: "running", status: "Up 3 hours (unhealthy)" })).toBe("critical");
    expect(containerStatus({ state: "running", status: "Up (health: starting)" })).toBe("warning");
    expect(containerStatus({ state: "exited", status: "Exited (0)" })).toBe("stopped");
    expect(containerStatus({ state: "dead", status: "" })).toBe("critical");
  });
  it("publishes only ports that are published, with the engine as the parent", () => {
    const s = dockerSnapshot("h1", "docker", [ctr("c1", "api", { ports: [{ host_ip: "0.0.0.0", host_port: "8080", container_port: "80", proto: "tcp" }, { host_ip: "", host_port: "", container_port: "9000", proto: "tcp" }] })], 5, {});
    expect(s.resources.filter((r) => r.type === "port").map((r) => r.name)).toEqual(["TCP :8080"]);
    const kinds = s.relationships.map((e) => e.type);
    expect(kinds).toEqual(["contains", "contains", "publishes_port"]);
    expect(s.relationships.every((e) => e.verified && e.evidence)).toBe(true);
  });
  it("shows CPU and memory only from a real stats reading", () => {
    const none = dockerSnapshot("h1", "docker", [ctr("c1", "a")], 1, {}).resources.find((r) => r.type === "container")!;
    expect(none.attributes["CPU %"]).toBeNull();
    const some = dockerSnapshot("h1", "docker", [ctr("c1aaaaaaaaaaaaaaaa", "a")], 1, { c1aaaaaaaaaa: { id: "c1aaaaaaaaaa", name: "a", cpu_pct: 12.5, mem_usage: "10MiB / 1GiB", mem_pct: 1, net_io: "", block_io: "", pids: 1 } }).resources.find((r) => r.type === "container")!;
    expect(some.attributes["CPU %"]).toBe(12.5);
  });
  it("lists only services, with the right status", () => {
    const s = systemdSnapshot("h1", [unit("nginx.service"), unit("ssh.socket"), unit("bad.service", "failed", "failed"), unit("old.service", "inactive", "dead")], 1);
    expect(s.resources.map((r) => [r.name, r.status])).toEqual([["nginx.service", "running"], ["bad.service", "critical"], ["old.service", "stopped"]]);
    expect(unitStatus({ load: "not-found", active: "inactive", sub: "dead" })).toBe("warning");
  });
  it("counts tmux windows and panes and respects the socket", () => {
    const pane = { id: "%1" } as never;
    const s = tmuxSnapshot(tmuxSnap("h1", [{ id: "$1", name: "dev", attached: 1, created: 0, activity: 0, group: null, mouse: null, windows: [{ panes: [pane, pane] } as never, { panes: [pane] } as never] }]));
    const r = s.resources[0];
    expect([r.attributes.Windows, r.attributes.Panes, r.status]).toEqual([2, 3, "running"]);
    expect(r.id).toBe(tmuxKey("h1", "default", "$1"));
    const other = tmuxSnapshot(tmuxSnap("h1", [], { socket: { kind: "name", value: "work" } }));
    expect(other.scopeKey).not.toBe(s.scopeKey);
    expect(tmuxUnsupported(tmuxSnap("h1", [], { state: { kind: "not_installed", os: "Linux" } }))).toMatch(/not installed/);
    expect(tmuxUnsupported(tmuxSnap("h1", [], { state: { kind: "no_server" } }))).toBeNull();
  });
  it("reads saved hosts without a connection and marks the connected ones", () => {
    const s = hostsSnapshot([host("h1", "alpha", { connected: true }), host("h2", "beta")], 9);
    expect(s.resources.map((r) => [r.name, r.status, r.statusText])).toEqual([["alpha", "healthy", "Connected"], ["beta", "unknown", "Not connected"]]);
  });
});

function world() {
  const inv = new Inventory();
  inv.apply(hostsSnapshot([host("h1", "alpha"), host("h2", "beta")], 100), 100);
  inv.apply(dockerSnapshot("h1", "docker", [ctr("c1", "backend-api", { image: "node:20", ports: [{ host_ip: "0.0.0.0", host_port: "8080", container_port: "80", proto: "tcp" }] }), ctr("c2", "redis", { state: "exited", status: "Exited (0)" })], 100, {}), 100);
  inv.apply(systemdSnapshot("h1", [unit("nginx.service")], 100), 100);
  inv.apply(tmuxSnapshot(tmuxSnap("h1", [{ id: "$1", name: "monitoring", attached: 0, created: 0, activity: 0, group: null, mouse: null, windows: [] }], { taken_at_ms: 100 })), 100);
  return inv;
}

describe("search and filters", () => {
  const inv = world();
  const all = inv.resources(100);
  const find = (f: Partial<typeof NO_FILTERS>) => search(all, { ...NO_FILTERS, ...f }, envOf).map((r) => r.name);
  it("finds by name, image, host, tag, type and unit", () => {
    expect(find({ query: "backend" })).toEqual(["backend-api", "TCP :8080"]); // its published port is found by its container too
    expect(find({ query: "node:20" })).toEqual(["backend-api"]);
    expect(find({ query: "nginx" })).toEqual(["nginx.service", "redis"]); // the second runs the nginx image
    expect(find({ query: "monitoring tmux" })).toEqual(["monitoring"]);
    expect(find({ query: "web" }).sort()).toEqual(["alpha", "beta"]);
  });
  it("needs every word and ranks a name that starts with the query first", () => {
    expect(find({ query: "redis node" })).toEqual([]);
    expect(find({ query: "re" })[0]).toBe("redis");
  });
  it("filters by type, status, host and environment", () => {
    expect(find({ types: ["container"] }).sort()).toEqual(["backend-api", "redis"]);
    expect(find({ types: ["container"], statuses: ["stopped"] })).toEqual(["redis"]);
    expect(find({ hostId: "h2" })).toEqual(["beta"]);
    expect(find({ environment: "staging", types: ["host"] })).toEqual(["beta"]);
  });
});

describe("counts say what they cover", () => {
  it("never presents a partial discovery as complete", () => {
    const inv = world();
    const s = summarize(inv, 100, ["h1", "h2"]);
    expect(s.containers.known).toBe(2);
    expect(s.containers.hostsObserved).toBe(1);
    expect(countText(s.containers)).toBe("2 known · 1 of 2 hosts read");
    inv.fail("docker:h2", "docker", "h2", "x", 101);
    expect(countText(summarize(inv, 101, ["h1", "h2"]).containers)).toContain("1 failed");
  });
  it("counts stale ones", () => {
    const inv = world();
    inv.fail("docker:h1", "docker", "h1", "gone", 200);
    expect(summarize(inv, 200, ["h1"]).containers.stale).toBe(2);
  });
});

describe("the tree", () => {
  const inv = world();
  const rows = (expanded: string[], f = {}) => buildRows({ inv, views: inv.resources(100), filters: { ...NO_FILTERS, ...f }, envOf, expanded: new Set(expanded), now: 100 });
  it("shows hosts closed by default and does not connect to read them", () => {
    expect(rows([]).map((r) => [r.kind, r.label])).toEqual([["host", "alpha"], ["host", "beta"]]);
  });
  it("groups what is under a host, engine before its containers (and counts containers only)", () => {
    const r = rows([hostKey("h1"), groupKey("h1", "docker"), groupKey("h1", "systemd")]);
    expect(r.map((x) => `${x.depth}:${x.label}`)).toEqual(["0:alpha", "1:Docker", "2:Docker", "3:backend-api", "3:redis", "1:systemd", "2:nginx.service", "1:Tmux", "0:beta"]);
  });
  it("counts a Docker group's containers, not its engine", () => {
    const r = rows([hostKey("h1")]);
    expect(r.find((x) => x.key === groupKey("h1", "docker"))?.count).toBe(2);
  });
  it("says a host that was not read is not read, not empty", () => {
    const r = rows([hostKey("h2")]);
    expect(r.filter((x) => x.hostId === "h2" && x.kind === "group").map((x) => x.text)).toEqual(["Not read yet", "Not read yet", "Not read yet"]);
  });
  it("calls a group that never read 'unavailable' and one that read before 'stale'", () => {
    const i = world();
    i.fail("docker:h2", "docker", "h2", "connection refused", 100);
    i.fail("systemd:h1", "systemd", "h1", "timed out", 101);
    const r = buildRows({ inv: i, views: i.resources(101), filters: NO_FILTERS, envOf, expanded: new Set([hostKey("h1"), hostKey("h2")]), now: 101 });
    expect(r.find((x) => x.key === groupKey("h2", "docker"))).toMatchObject({ freshness: "unavailable", text: "Could not read: connection refused" });
    expect(r.find((x) => x.key === groupKey("h1", "systemd"))?.freshness).toBe("stale");
  });
  it("shows an unsupported provider's reason", () => {
    const i = world();
    i.markUnsupported("systemd:h1", "systemd", "h1", "systemd is not running here", 100);
    const r = buildRows({ inv: i, views: i.resources(100), filters: NO_FILTERS, envOf, expanded: new Set([hostKey("h1")]), now: 100 });
    expect(r.find((x) => x.key === groupKey("h1", "systemd"))?.text).toBe("systemd is not running here");
  });
  it("opens the way to a match and hides the rest", () => {
    const r = rows([], { query: "redis" });
    expect(r.map((x) => x.label)).toEqual(["alpha", "Docker", "redis"]);
  });
  it("has stable keys across a refresh", () => {
    const a = rows([hostKey("h1"), groupKey("h1", "docker")]).map((r) => r.key);
    const i = world();
    i.apply(dockerSnapshot("h1", "docker", [ctr("c1", "backend-api")], 300, {}), 300);
    const b = buildRows({ inv: i, views: i.resources(300), filters: NO_FILTERS, envOf, expanded: new Set([hostKey("h1"), groupKey("h1", "docker")]), now: 300 }).map((r) => r.key);
    expect(b.filter((k) => a.includes(k)).length).toBeGreaterThanOrEqual(4);
  });
});

describe("the graph", () => {
  const inv = world();
  const views = inv.resources(100);
  const edges = inv.relationships(100);
  it("holds only verified edges between things that exist", () => {
    const i = new Inventory();
    const s = dockerSnapshot("h1", "docker", [ctr("c1", "a")], 1, {});
    i.apply({ ...s, relationships: [...s.relationships, { from: "container:ghost", to: "x", type: "contains", source: "docker", observedAt: 1, verified: true, evidence: "e" }, { ...s.relationships[0], verified: false, to: "y" }] }, 1);
    expect(i.relationships(1).every((e) => e.verified)).toBe(true);
    expect(i.relationships(1).some((e) => e.from === "container:ghost")).toBe(false);
    // The host is not in this inventory, so the host → engine edge has no end and is not drawn.
    expect(i.relationships(1).some((e) => e.from === hostKey("h1"))).toBe(false);
  });
  it("lays host, engine, container, port in columns from a container", () => {
    const c = views.find((v) => v.name === "backend-api")!;
    const g = neighborhood(views, edges, c.id, 2);
    const col = Object.fromEntries(g.nodes.map((n) => [n.resource.type, n.x]));
    expect(col.host).toBeLessThan(col.docker_engine);
    expect(col.docker_engine).toBeLessThan(col.container);
    expect(col.container).toBeLessThan(col.port);
    expect(g.edges.length).toBe(4); // and the engine's other container, two steps away
    expect(g.truncated).toBe(false);
  });
  it("does not reach past its depth or past its size", () => {
    const h = views.find((v) => v.type === "host" && v.name === "alpha")!;
    expect(neighborhood(views, edges, h.id, 1).nodes.map((n) => n.resource.type).sort()).toEqual(["docker_engine", "host", "systemd_service", "tmux_session"]);
    const big = neighborhood(views, edges, h.id, 3, 3);
    expect(big.nodes.length).toBe(3);
    expect(big.truncated).toBe(true);
  });
  it("returns nothing for an id it does not hold", () => {
    expect(neighborhood(views, edges, "container:forged").nodes).toEqual([]);
  });
  it("names a resource's ancestors", () => {
    const p = views.find((v) => v.type === "port")!;
    expect(parentsOf(p.id, edges).map((id) => id.split(":")[0])).toEqual(["host", "engine", "container"]);
  });
});

describe("actions", () => {
  const views = world().resources(100);
  const of = (type: string) => views.find((v) => v.type === type);
  it("offers only what the owning module can open", () => {
    expect(actionsFor(of("container"), "ssh").map((a) => a.id)).toEqual(["open-container"]);
    expect(actionsFor(of("systemd_service"), "ssh").map((a) => a.id)).toEqual(["open-service", "view-logs"]);
    expect(actionsFor(of("tmux_session"), "ssh").map((a) => a.id)).toEqual(["open-tmux"]);
    expect(actionsFor(of("host"), "ssh").map((a) => a.id)).toEqual(["connect", "host-metrics"]);
  });
  it("says why one is unavailable and gives nothing for an unknown resource", () => {
    const rdp = actionsFor(of("host"), "rdp");
    expect(rdp.every((a) => !a.enabled && a.reason)).toBe(true);
    expect(actionsFor(undefined, "ssh")).toEqual([]);
  });
  it("never offers a lifecycle action", () => {
    const all = views.flatMap((v) => actionsFor(v, "ssh")).map((a) => a.id);
    for (const bad of ["start", "stop", "restart", "kill", "remove", "delete"]) expect(all.some((id) => id.includes(bad))).toBe(false);
  });
});

describe("discovery", () => {
  it("never runs more than the limit at once and carries on past a failure", async () => {
    let live = 0;
    let peak = 0;
    const done: number[] = [];
    const r = await runBounded([1, 2, 3, 4, 5, 6, 7], 3, async (n) => {
      live++;
      peak = Math.max(peak, live);
      await new Promise((res) => setTimeout(res, 5));
      live--;
      if (n === 4) throw new Error("slow host failed");
      done.push(n);
    }, new AbortController().signal);
    expect(peak).toBe(3);
    expect(r).toMatchObject({ started: 7, finished: 7, failed: 1, cancelled: false });
    expect(done.sort()).toEqual([1, 2, 3, 5, 6, 7]);
  });
  it("starts nothing new once cancelled", async () => {
    const ac = new AbortController();
    const ran: number[] = [];
    const r = await runBounded([1, 2, 3, 4, 5, 6], 1, async (n) => {
      ran.push(n);
      if (n === 2) ac.abort();
    }, ac.signal);
    expect(ran).toEqual([1, 2]);
    expect(r.cancelled).toBe(true);
  });
  it("does not run the same discovery twice at once", async () => {
    const f = new SingleFlight();
    let runs = 0;
    const work = async () => {
      runs++;
      await new Promise((r) => setTimeout(r, 5));
    };
    await Promise.all([f.run("h1", work), f.run("h1", work), f.run("h2", work)]);
    expect(runs).toBe(2);
    expect(f.has("h1")).toBe(false);
  });
});
