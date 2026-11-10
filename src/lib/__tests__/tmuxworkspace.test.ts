// The redesigned Tmux workspace's pure logic: what changed between readings (events), the numbers over the page, the small
// charts' series, where tmux put each pane of a window, and the output filter.
import { describe, expect, it } from "vitest";
import { layoutChecksum } from "$lib/tmux/tmuxLayoutParser";
import {
  DEFAULT_SOCKET, diffEvents, filterOutput, paneRects, pushHistory, serverKey, sparkPoints, summarize,
  type HostEntry, type HostSnapshot, type Pane, type Resources, type Session, type Window,
} from "$lib/tmux/monitor";

const pane = (id: string, over: Partial<Pane> = {}): Pane => ({ id, index: 0, active: true, title: "", command: "bash", path: "/", pid: 1, width: 80, height: 24, dead: false, in_mode: false, ...over });
const win = (id: string, index: number, name: string, panes: Pane[], over: Partial<Window> = {}): Window => ({ id, index, name, active: index === 0, layout: "", zoomed: false, activity: false, bell: false, panes, ...over });
const sess = (id: string, name: string, windows: Window[], over: Partial<Session> = {}): Session => ({ id, name, attached: 0, created: 1, activity: 0, group: null, mouse: false, windows, ...over });
const snap = (sessions: Session[], over: Partial<HostSnapshot> = {}): HostSnapshot => ({
  host_id: "h", socket: DEFAULT_SOCKET, generation: 1, taken_at_ms: 1, version: null, state: { kind: "ready" }, server: { pid: 1, started: 1, socket_path: "/x" }, sessions, other_sockets: [], stale: null, ...over,
});
const text = (e: { text: string }[]) => e.map((x) => x.text);

describe("events between readings", () => {
  const base = snap([sess("$0", "backend", [win("@0", 0, "dev", [pane("%1")])])]);

  it("reports nothing on a first reading, an unreadable one, or when nothing changed", () => {
    expect(diffEvents(undefined, base, 1)).toEqual([]);
    expect(diffEvents(base, snap([], { state: { kind: "failed", message: "x" } }), 1)).toEqual([]);
    expect(diffEvents(snap([], { state: { kind: "no_server" } }), base, 1)).toEqual([]);
    expect(diffEvents(base, structuredClone(base), 1)).toEqual([]);
  });

  it("reports sessions, windows and panes that appeared or ended, and renames", () => {
    const next = snap([
      sess("$0", "backend2", [win("@0", 0, "dev", [pane("%1"), pane("%2", { command: "cargo" })]), win("@1", 1, "logs", [pane("%3")])]),
      sess("$1", "db", [win("@2", 0, "psql", [pane("%4")])]),
    ]);
    const t = text(diffEvents(base, next, 5));
    expect(t).toContain("Session backend renamed to backend2.");
    expect(t).toContain("Pane %2 (cargo) opened in backend2:dev.");
    expect(t).toContain("Window 1: logs created in backend2.");
    expect(t).toContain("Session db created (1 window).");
    const gone = text(diffEvents(next, base, 6));
    expect(gone).toContain("Session db ended.");
    expect(gone).toContain("Window 1: logs closed in backend.");
  });

  it("reports a window rename and a pane that closed", () => {
    const a = snap([sess("$0", "s", [win("@0", 0, "old", [pane("%1"), pane("%2")])])]);
    const b = snap([sess("$0", "s", [win("@0", 0, "new", [pane("%1")])])]);
    expect(text(diffEvents(a, b, 1))).toEqual(["Window old renamed to new in s.", "Pane %2 (bash) closed in s:new."]);
  });

  it("reports attach and detach", () => {
    const a = snap([sess("$0", "s", [win("@0", 0, "w", [pane("%1")])])]);
    const b = snap([sess("$0", "s", [win("@0", 0, "w", [pane("%1")])], { attached: 1 })]);
    expect(text(diffEvents(a, b, 1))).toEqual(["A client attached to s."]);
    expect(text(diffEvents(b, a, 1))).toEqual(["The last client detached from s."]);
  });

  it("does not compare ids across a server restart: that is one event, not a pile of invented changes", () => {
    const restarted = snap([sess("$0", "other", [win("@0", 0, "w", [pane("%0")])])], { server: { pid: 2, started: 9, socket_path: "/x" } });
    const e = diffEvents(base, restarted, 1);
    expect(e.map((x) => x.kind)).toEqual(["server_restarted"]);
  });

  it("stamps the server and time on every event", () => {
    expect(text(diffEvents(base, snap([]), 42))).toEqual(["Session backend ended."]);
    const e2 = diffEvents(base, snap([sess("$0", "backend", [win("@0", 0, "dev", [pane("%1")], { name: "dev" })], { attached: 2 })]), 42);
    expect(e2.every((x) => x.at === 42 && x.server === serverKey("h", DEFAULT_SOCKET))).toBe(true);
  });
});

describe("the summary over the page", () => {
  const entries: HostEntry[] = [
    { hostId: "a", name: "a", socket: DEFAULT_SOCKET },
    { hostId: "b", name: "b", socket: DEFAULT_SOCKET },
    { hostId: "c", name: "c", socket: DEFAULT_SOCKET },
  ];
  const ka = serverKey("a", DEFAULT_SOCKET);
  const kb = serverKey("b", DEFAULT_SOCKET);
  const snaps = {
    [ka]: snap([sess("$0", "x", [win("@0", 0, "w", [pane("%1"), pane("%2")]), win("@1", 1, "v", [pane("%3")])]), sess("$1", "y", [win("@2", 0, "w", [pane("%4")])])], { host_id: "a" }),
    [kb]: snap([], { host_id: "b", state: { kind: "failed", message: "x" } }),
  };

  it("counts what was read and says how many hosts could not be", () => {
    const s = summarize(entries, snaps, {});
    expect([s.hosts, s.sessions, s.windows, s.panes]).toEqual([3, 2, 3, 4]);
    expect(s.unreadable).toBe(2); // b failed, c was never read
  });

  it("has no CPU or memory until something was measured, never a made-up zero", () => {
    const s = summarize(entries, snaps, {});
    expect([s.cpuPct, s.rss]).toEqual([null, null]);
  });

  it("sums only measured sessions", () => {
    const res: Resources = {
      host_id: "a", taken_at_ms: 1, panes: {}, cpu_note: null, estimate: true,
      sessions: [
        { session_id: "$0", usage: { processes: 3, rss: 1000, cpu_pct: 10, top: [] }, panes_seen: 3, panes_total: 3 },
        { session_id: "$1", usage: null, panes_seen: 0, panes_total: 1 },
      ],
    };
    const s = summarize(entries, snaps, { [ka]: res });
    expect([s.cpuPct, s.rss]).toEqual([10, 1000]);
    const noCpu = summarize(entries, snaps, { [ka]: { ...res, sessions: [{ ...res.sessions[0], usage: { processes: 1, rss: 5, cpu_pct: null, top: [] } }] } });
    expect([noCpu.cpuPct, noCpu.rss]).toEqual([null, 5]);
  });
});

describe("series for the small charts", () => {
  it("keeps the newest readings", () => {
    expect(pushHistory(undefined, 1)).toEqual([1]);
    let h: number[] = [];
    for (let i = 0; i < 40; i++) h = pushHistory(h, i, 30);
    expect(h).toHaveLength(30);
    expect(h[0]).toBe(10);
    expect(h.at(-1)).toBe(39);
  });
  it("draws nothing for fewer than two readings and scales the rest into the box", () => {
    expect(sparkPoints([], 100, 24)).toBe("");
    expect(sparkPoints([5], 100, 24)).toBe("");
    const pts = sparkPoints([0, 10], 100, 24).split(" ").map((p) => p.split(",").map(Number));
    expect(pts[0]).toEqual([0, 23]);
    expect(pts[1]).toEqual([100, 1]);
    expect(sparkPoints([3, 3, 3], 100, 24)).not.toContain("NaN");
  });
});

describe("where tmux put each pane", () => {
  const body = "159x48,0,0{79x48,0,0,1,79x48,80,0[79x24,80,0,2,79x23,80,25,3]}";
  const layout = `${layoutChecksum(body)},${body}`;
  const w = win("@0", 0, "dev", [pane("%1"), pane("%2"), pane("%3")], { layout });

  it("turns the window's layout into shares of its area", () => {
    const r = paneRects(w)!;
    expect(r.get("%1")).toMatchObject({ left: 0, top: 0 });
    expect(r.get("%1")!.width).toBeCloseTo((79 / 159) * 100);
    expect(r.get("%1")!.height).toBe(100);
    expect(r.get("%2")!.left).toBeCloseTo((80 / 159) * 100);
    expect(r.get("%3")!.top).toBeCloseTo((25 / 48) * 100);
  });

  it("gives up rather than draw something that is not the window", () => {
    expect(paneRects({ ...w, layout: "" })).toBeNull();
    expect(paneRects({ ...w, layout: layout.replace(/^[0-9a-f]{4}/, "0000") }), "bad checksum").toBeNull();
    expect(paneRects({ ...w, panes: [pane("%1"), pane("%2")] }), "a pane the layout has but the window does not").toBeNull();
    expect(paneRects({ ...w, panes: [] })).toBeNull();
  });
});

describe("filtering output", () => {
  const out = "one\n\u001b[31mred error\u001b[0m\nthree\nError again\n\n\n";
  it("strips colour codes, drops trailing blanks and keeps the last lines", () => {
    expect(filterOutput(out, "", { regex: false, caseSensitive: false, max: 100 }).lines).toEqual(["one", "red error", "three", "Error again"]);
    expect(filterOutput(out, "", { regex: false, caseSensitive: false, max: 2 }).lines).toEqual(["three", "Error again"]);
  });
  it("matches as text by default, with or without case, and as a pattern on request", () => {
    expect(filterOutput(out, "error", { regex: false, caseSensitive: false, max: 100 }).lines).toEqual(["red error", "Error again"]);
    expect(filterOutput(out, "error", { regex: false, caseSensitive: true, max: 100 }).lines).toEqual(["red error"]);
    expect(filterOutput("a.c\nabc", "a.c", { regex: false, caseSensitive: true, max: 9 }).lines).toEqual(["a.c"]);
    expect(filterOutput("a.c\nabc", "a.c", { regex: true, caseSensitive: true, max: 9 }).lines).toEqual(["a.c", "abc"]);
  });
  it("reports an invalid or runaway pattern instead of hanging or throwing", () => {
    expect(filterOutput("x", "(", { regex: true, caseSensitive: false, max: 9 }).error).toMatch(/not a valid/);
    const t0 = Date.now();
    const r = filterOutput("a".repeat(5000), "(a+)+$", { regex: true, caseSensitive: false, max: 9 });
    expect(r.error).toMatch(/too complex/);
    expect(Date.now() - t0).toBeLessThan(500);
    expect(filterOutput("x", "y".repeat(501), { regex: false, caseSensitive: false, max: 9 }).error).toMatch(/too long/);
  });
});
