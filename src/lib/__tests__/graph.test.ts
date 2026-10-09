import { describe, expect, it } from "vitest";
import { COLUMN, ROW, bounds, buildGraph, connectedOnly, focus, layout, type GraphInput } from "../resources/graph";

const host = (id: string, extra: Partial<GraphInput["hosts"][number]> = {}) => ({ id, label: id, hostname: `${id}.example`, port: 22, env: "", tags: [], group: "", ...extra });
const input = (over: Partial<GraphInput> = {}): GraphInput => ({ hosts: [], proxies: [], forwards: [], databases: [], ...over });
const edgeText = (g: ReturnType<typeof buildGraph>) => g.edges.map((e) => `${e.from}>${e.to}:${e.kind}`).sort();

describe("what is linked to what", () => {
  it("draws a jump chain, a proxy at the start of it, and what rides the last host", () => {
    const g = buildGraph(
      input({
        hosts: [host("bastion"), host("app", { jumpId: "bastion", proxyId: "corp" }), host("lone")],
        proxies: [{ id: "corp", name: "Corporate", kind: "socks5" }],
        forwards: [{ id: "f1", label: "web", hostId: "app", summary: "8080 -> db:5432" }],
        databases: [{ id: "d1", name: "orders", engine: "postgres", env: "production", sshHostId: "app" }],
      }),
    );
    expect(edgeText(g)).toEqual(["host:app>database:d1:database", "host:app>tunnel:f1:tunnel", "host:bastion>host:app:jump", "proxy:corp>host:bastion:proxy"]);
    expect(g.nodes.map((n) => n.id).sort()).toEqual(["database:d1", "host:app", "host:bastion", "host:lone", "proxy:corp", "tunnel:f1"]);
  });

  it("does not invent links: a missing host, proxy or reference leaves no line and no dangling end", () => {
    const g = buildGraph(
      input({
        hosts: [host("a", { jumpId: "gone", proxyId: "ghost" })],
        forwards: [{ id: "f", label: "x", hostId: "gone", summary: "" }],
        databases: [{ id: "d", name: "n", engine: "mysql", env: "", sshHostId: "gone" }, { id: "e", name: "direct", engine: "mysql", env: "" }],
      }),
    );
    expect(g.edges).toEqual([]);
    expect(g.nodes.map((n) => n.id)).toEqual(["host:a"]);
  });

  it("survives a jump loop and a host that jumps through itself", () => {
    const g = buildGraph(input({ hosts: [host("a", { jumpId: "b", proxyId: "p" }), host("b", { jumpId: "a" }), host("c", { jumpId: "c" })], proxies: [{ id: "p", name: "P", kind: "http" }] }));
    expect(edgeText(g).filter((e) => e.includes(":jump"))).toEqual(["host:a>host:b:jump", "host:b>host:a:jump"]);
    expect(layout(g).size).toBe(g.nodes.length);
  });

  it("states each link once, and the address with its port when it is not 22", () => {
    const g = buildGraph(input({ hosts: [host("a", { jumpId: "b", port: 2222 }), host("b")] }));
    expect(g.edges).toHaveLength(1);
    expect(g.nodes.find((n) => n.id === "host:a")?.sub).toBe("a.example:2222");
    expect(g.nodes.find((n) => n.id === "host:b")?.sub).toBe("b.example");
  });

  it("hides what stands alone, and focuses a search on its match with its neighbours", () => {
    const g = buildGraph(input({ hosts: [host("bastion"), host("app", { jumpId: "bastion" }), host("lone"), host("db", { jumpId: "bastion", tags: ["prod"] })] }));
    expect(connectedOnly(g).nodes.map((n) => n.id).sort()).toEqual(["host:app", "host:bastion", "host:db"]);
    const f = focus(g, (n) => n.tags.includes("prod"));
    expect(f.nodes.map((n) => n.id).sort()).toEqual(["host:bastion", "host:db"]);
    expect(f.edges.map((e) => e.id)).toEqual(["host:bastion>host:db:jump"]);
    expect(focus(g, () => false).nodes).toEqual([]);
  });
});

describe("laying it out", () => {
  it("puts every link left to right, in columns, one row per node", () => {
    const g = buildGraph(
      input({
        hosts: [host("b1"), host("b2", { jumpId: "b1" }), host("app", { jumpId: "b2", proxyId: "p" })],
        proxies: [{ id: "p", name: "P", kind: "socks5" }],
        forwards: [{ id: "f", label: "t", hostId: "app", summary: "" }],
      }),
    );
    const at = layout(g);
    for (const e of g.edges) expect(at.get(e.from)!.layer, e.id).toBeLessThan(at.get(e.to)!.layer);
    expect(at.get("proxy:p")).toMatchObject({ layer: 0, x: 0 });
    expect(at.get("host:app")!.x).toBe(at.get("host:app")!.layer * COLUMN);
    expect(at.get("tunnel:f")!.layer).toBe(at.get("host:app")!.layer + 1);
    const col = [...at.values()].filter((p) => p.layer === 0).map((p) => p.y);
    expect(new Set(col).size).toBe(col.length);
    expect(col.every((y) => y % ROW === 0)).toBe(true);
    const b = bounds(at);
    expect(b.width).toBeGreaterThan(3 * COLUMN);
    expect(bounds(new Map())).toEqual({ width: 0, height: 0 });
  });
});
