// The Tmux Monitor's pure logic: the tree, search, notices, accepting readings, previews, and the ANSI renderer.
import { describe, expect, it } from "vitest";
import { actionsFor } from "$lib/tmux/actions";
import { parseAnsi, plainText } from "$lib/tmux/ansi";
import {
  DEFAULT_SOCKET, PREVIEW_DEFAULTS, dragWidth, layoutFor, accept, activePane, ago, buildRows, bytes, clampPrefs, closedKey, installHint, locate, matches, noticeFor, nodeKey, parentOf, previewsDue, sameContent, serverKey, socketKey, step,
  type HostEntry, type HostSnapshot, type Pane, type Session, type Window,
} from "$lib/tmux/monitor";

const pane = (id: string, over: Partial<Pane> = {}): Pane => ({ id, index: 0, active: true, title: "", command: "bash", path: "/home/u", pid: 100, width: 80, height: 24, dead: false, in_mode: false, ...over });
const win = (id: string, index: number, name: string, panes: Pane[], over: Partial<Window> = {}): Window => ({ id, index, name, active: index === 0, layout: "layout", zoomed: false, activity: false, bell: false, panes, ...over });
const sess = (id: string, name: string, windows: Window[], over: Partial<Session> = {}): Session => ({ id, name, attached: 0, created: 1700000000, activity: 0, group: null, mouse: false, windows, ...over });
const snap = (host: string, sessions: Session[], over: Partial<HostSnapshot> = {}): HostSnapshot => ({
  host_id: host, socket: DEFAULT_SOCKET, generation: 1, taken_at_ms: 1, version: { major: 3, minor: 4, dev: false, raw: "tmux 3.4" }, state: sessions.length ? { kind: "ready" } : { kind: "no_server" },
  server: { pid: 1, started: 1, socket_path: "/tmp/tmux-1/default" }, sessions, other_sockets: [], stale: null, ...over,
});

const claude = snap("h-claude", [
  sess("$0", "backend", [win("@0", 0, "development", [pane("%1", { command: "bash" }), pane("%2", { command: "cargo run", active: false })]), win("@1", 1, "logs", [pane("%3", { command: "journalctl" })])], { attached: 1 }),
  sess("$1", "deployment", [win("@2", 0, "deploy", [pane("%4", { command: "ansible" })])]),
]);
const elastic = snap("h-elastic", [sess("$0", "monitoring", [win("@0", 0, "metrics", [pane("%0", { command: "htop" }), pane("%1", { command: "journalctl", active: false })])])]);
const empty = snap("h-empty", []);
const entries: HostEntry[] = [
  { hostId: "h-claude", name: "claude", socket: DEFAULT_SOCKET },
  { hostId: "h-elastic", name: "Elastic", socket: DEFAULT_SOCKET },
  { hostId: "h-empty", name: "ranovarte.net", socket: DEFAULT_SOCKET },
];
const all = new Map([claude, elastic, empty].map((s) => [serverKey(s.host_id, s.socket), s]));
const noTags = () => [] as string[];
const rows = (open: string[] = [], query = "", tags: (s: string, n: string) => string[] = noTags) => buildRows(entries, all, new Set(open), query, tags);
const labels = (r: ReturnType<typeof rows>) => r.map((x) => `${"  ".repeat(x.depth)}${x.label}`);

describe("the explorer tree", () => {
  it("groups sessions by host and starts with hosts open and sessions closed", () => {
    expect(labels(rows())).toEqual(["claude", "  backend", "  deployment", "Elastic", "  monitoring", "ranovarte.net", "  No tmux sessions found."]);
    expect(rows().filter((r) => r.kind === "host").map((r) => r.meta)).toEqual(["2 sessions", "1 session", "No tmux sessions found"]);
  });

  it("opens sessions and windows down to the panes, keyed by tmux ids and not by names", () => {
    const server = serverKey("h-claude", DEFAULT_SOCKET);
    const r = rows([nodeKey.session(server, "$0"), nodeKey.window(server, "@0")]);
    expect(labels(r).slice(0, 8)).toEqual(["claude", "  backend", "    0: development", "      %1 bash", "      %2 cargo run", "    1: logs", "  deployment", "Elastic"]);
    const keys = r.map((x) => x.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(r.every((x) => !/backend|development/.test(x.key))).toBe(true);
    // The same ids on another host do not collide: "$0" and "%1" exist on both.
    expect(rows([nodeKey.session(serverKey("h-elastic", DEFAULT_SOCKET), "$0")]).filter((x) => x.sessionId === "$0").map((x) => x.key)).toContain(nodeKey.session(serverKey("h-elastic", DEFAULT_SOCKET), "$0"));
  });

  it("collapses a host without touching the others", () => {
    const r = rows([closedKey(nodeKey.host(serverKey("h-claude", DEFAULT_SOCKET)))]);
    expect(labels(r)).toEqual(["claude", "Elastic", "  monitoring", "ranovarte.net", "  No tmux sessions found."]);
    expect(r[0].expanded).toBe(false);
  });

  it("shows attached and active state, window and pane counts", () => {
    const r = rows();
    const backend = r.find((x) => x.label === "backend")!;
    expect(backend.attached).toBe(true);
    expect(backend.meta).toBe("2 windows · 1 attached");
    expect(r.find((x) => x.label === "deployment")!.meta).toBe("1 window · detached");
    const server = serverKey("h-claude", DEFAULT_SOCKET);
    const w = rows([nodeKey.session(server, "$0")]).find((x) => x.kind === "window")!;
    expect(w.meta).toBe("2 panes");
    expect(w.active).toBe(true);
  });

  it("marks a host whose last reading is stale", () => {
    const stale = snap("h-claude", claude.sessions, { stale: { since_ms: 1, message: "connection reset" } });
    const r = buildRows(entries.slice(0, 1), new Map([[serverKey("h-claude", DEFAULT_SOCKET), stale]]), new Set(), "", noTags);
    expect(r[0].stale).toBe(true);
    expect(r.some((x) => x.label === "backend")).toBe(true);
  });

  it("says a host has not been read yet instead of showing it as empty", () => {
    const r = buildRows(entries.slice(0, 1), new Map(), new Set(), "", noTags);
    expect(r[0].meta).toBe("reading…");
    expect(r).toHaveLength(1);
  });

  it("keeps each socket of a host as its own server", () => {
    const named = { kind: "name", value: "work" } as const;
    const e: HostEntry[] = [{ hostId: "h-claude", name: "claude", socket: DEFAULT_SOCKET }, { hostId: "h-claude", name: "claude · L:work", socket: named }];
    const m = new Map([[serverKey("h-claude", DEFAULT_SOCKET), claude], [serverKey("h-claude", named), snap("h-claude", [sess("$0", "other", [win("@0", 0, "w", [pane("%0")])])], { socket: named })]]);
    const r = buildRows(e, m, new Set(), "", noTags);
    expect(r.filter((x) => x.kind === "host").map((x) => x.label)).toEqual(["claude", "claude · L:work"]);
    expect(socketKey(named)).toBe("L:work");
    expect(serverKey("h-claude", DEFAULT_SOCKET)).not.toBe(serverKey("h-claude", named));
  });
});

describe("searching metadata", () => {
  it("matches host, session, window, pane title, command and tags, each word anywhere", () => {
    expect(matches("back dev", "backend", "development")).toBe(true);
    expect(matches("zzz", "backend")).toBe(false);
    expect(matches("", "x")).toBe(true);
    expect(matches("BACK", "backend")).toBe(true);
  });

  it("keeps the path to every match open and drops the rest", () => {
    expect(labels(rows([], "cargo")).filter((l) => !l.startsWith("ranovarte"))).toEqual(["claude", "  backend", "    0: development", "      %2 cargo run"]);
    expect(labels(rows([], "monitoring"))).toEqual(["Elastic", "  monitoring", "    0: metrics", "      %0 htop", "      %1 journalctl"]);
  });

  it("finds by host name, window name and tag", () => {
    expect(labels(rows([], "elastic"))[0]).toBe("Elastic");
    expect(labels(rows([], "logs")).some((l) => l.includes("1: logs"))).toBe(true);
    const tagged = rows([], "production", (_s, name) => (name === "deployment" ? ["Production"] : []));
    expect(labels(tagged)).toContain("  deployment");
    expect(labels(tagged)).not.toContain("  backend");
  });

  it("does not search pane output, only what tmux lists", () => {
    expect(rows([], "ERROR: disk full")).toHaveLength(0);
  });

  it("shows nothing for a query that matches nothing", () => {
    expect(rows([], "no-such-thing")).toEqual([]);
  });
});

describe("moving through the tree", () => {
  it("steps through the visible rows and stops at the ends", () => {
    const r = rows();
    expect(step(r, null, 1)).toBe(r[0].key);
    expect(step(r, r[0].key, 1)).toBe(r[1].key);
    expect(step(r, r[0].key, -1)).toBe(r[0].key);
    const nav = r.filter((x) => x.kind !== "notice");
    expect(step(r, nav.at(-1)!.key, 1)).toBe(nav.at(-1)!.key);
  });

  it("finds the parent row for the Left arrow", () => {
    const server = serverKey("h-claude", DEFAULT_SOCKET);
    const r = rows([nodeKey.session(server, "$0")]);
    const window = r.find((x) => x.kind === "window")!;
    expect(parentOf(r, window)?.label).toBe("backend");
    expect(parentOf(r, r[0])).toBeUndefined();
  });
});

describe("accepting readings", () => {
  it("keeps the newer reading and drops a slow older one", () => {
    const a = snap("h", [], { generation: 5 });
    const older = snap("h", [sess("$9", "old", [])], { generation: 3 });
    const newer = snap("h", [sess("$1", "new", [])], { generation: 6 });
    expect(accept(a, older)).toBe(a);
    expect(accept(a, a)).toBe(a);
    expect(accept(a, newer)).toBe(newer);
    expect(accept(undefined, older)).toBe(older);
  });

  it("tells a redraw-worthy change from a newer reading of the same thing", () => {
    expect(sameContent(claude, { ...claude, generation: 9, taken_at_ms: 99 })).toBe(true);
    expect(sameContent(claude, snap("h-claude", [], {}))).toBe(false);
    expect(sameContent(claude, { ...claude, stale: { since_ms: 1, message: "x" } })).toBe(false);
  });
});

describe("what to tell the person", () => {
  it("explains a missing tmux with the install command for the host's system", () => {
    const n = noticeFor(snap("h", [], { state: { kind: "not_installed", os: "Linux|ubuntu|debian|Ubuntu 24.04" } }))!;
    expect(n.title).toBe("Tmux is not installed on this host.");
    expect(n.hint).toBe("sudo apt install tmux");
    expect(n.canCreate).toBe(false);
    expect(installHint("Linux|rocky|rhel fedora|Rocky")).toContain("dnf install tmux");
    expect(installHint("Linux|arch|")).toBe("sudo pacman -S tmux");
    expect(installHint("Linux|alpine|")).toBe("sudo apk add tmux");
    expect(installHint("Darwin")).toBe("brew install tmux");
    expect(installHint("FreeBSD")).toBe("sudo pkg install tmux");
    expect(installHint("Plan9")).toMatch(/package manager/);
  });

  it("offers Create Session when there are no sessions, and only then", () => {
    expect(noticeFor(empty)).toMatchObject({ title: "No tmux sessions found.", canCreate: true });
    expect(noticeFor(snap("h", [], { state: { kind: "empty" } }))?.canCreate).toBe(true);
    expect(noticeFor(claude)).toBeNull();
  });

  it("keeps a permission problem, an old tmux and a failed reading apart from an empty list", () => {
    expect(noticeFor(snap("h", [], { state: { kind: "permission_denied", message: "Permission denied" } }))).toMatchObject({ title: "The current SSH user cannot access this tmux server.", tone: "bad", canCreate: false });
    expect(noticeFor(snap("h", [], { state: { kind: "unsupported", version: "tmux 1.8" } }))?.detail).toContain("tmux 1.8");
    expect(noticeFor(snap("h", [], { state: { kind: "failed", message: "weird" } }))?.tone).toBe("bad");
    expect(noticeFor(undefined)?.title).toBe("Not read yet");
  });
});

describe("finding things in a snapshot", () => {
  it("locates a row's session, window and pane, and nothing once they are gone", () => {
    const l = locate(claude, { sessionId: "$0", windowId: "@0", paneId: "%2" });
    expect([l.session?.name, l.window?.name, l.pane?.command]).toEqual(["backend", "development", "cargo run"]);
    expect(locate(claude, { sessionId: "$9" }).session).toBeUndefined();
    expect(locate(claude, { sessionId: "$0", windowId: "@0", paneId: "%99" }).pane).toBeUndefined();
    expect(locate(undefined, { sessionId: "$0" }).session).toBeUndefined();
  });

  it("picks a session's active pane from its active window", () => {
    expect(activePane(claude.sessions[0])?.pane.id).toBe("%1");
  });
});

describe("what each node's menu offers", () => {
  const row = (kind: "host" | "session" | "window" | "pane", ids: { sessionId?: string; windowId?: string; paneId?: string } = {}) => ({ kind, ...ids });
  const ids = (a: { id: string }[]) => a.map((x) => x.id);

  it("offers new session and refresh on a host", () => {
    expect(ids(actionsFor(row("host"), claude))).toEqual(["new-session", "refresh", "add-socket", "remove-host"]);
  });

  it("offers the attach choices, rename, tags, mouse and kill on a session", () => {
    const a = actionsFor(row("session", { sessionId: "$0" }), claude);
    expect(ids(a)).toEqual(["attach-tab", "attach-right", "attach-down", "attach-replace", "new-window", "rename-session", "tags", "mouse-on", "kill-session"]);
    expect(a.find((x) => x.id === "kill-session")?.danger).toBe(true);
    expect(actionsFor(row("session", { sessionId: "$0" }), snap("h", [sess("$0", "m", [win("@0", 0, "w", [pane("%0")])], { mouse: true })])).some((x) => x.id === "mouse-off")).toBe(true);
  });

  it("offers the live terminal first, only where the page can show it, and says why when it cannot", () => {
    const t = { canSplit: true, canReplace: true };
    expect(ids(actionsFor(row("session", { sessionId: "$0" }), claude, t))).not.toContain("attach-live");
    const on = actionsFor(row("session", { sessionId: "$0" }), claude, { ...t, live: { allowed: true } });
    expect(ids(on)[0]).toBe("attach-live");
    expect(on[0].disabled).toBeFalsy();
    const off = actionsFor(row("session", { sessionId: "$0" }), claude, { ...t, live: { allowed: false, why: "No hooks." } });
    expect(off[0]).toMatchObject({ id: "attach-live", disabled: true, why: "No hooks." });
    for (const kind of ["window", "pane"] as const) {
      const r = kind === "window" ? row("window", { sessionId: "$0", windowId: "@0" }) : row("pane", { sessionId: "$0", windowId: "@0", paneId: "%1" });
      expect(ids(actionsFor(r, claude, { ...t, live: { allowed: true } }))).toContain("attach-live");
    }
  });

  it("disables splitting and replacing when there is no terminal to do it in, and says why", () => {
    const a = actionsFor(row("session", { sessionId: "$0" }), claude, { canSplit: false, canReplace: false });
    const right = a.find((x) => x.id === "attach-right")!;
    expect(right.disabled).toBe(true);
    expect(right.why).toMatch(/no terminal tab/);
    expect(a.find((x) => x.id === "attach-replace")?.why).toMatch(/not on this host/);
    expect(a.find((x) => x.id === "attach-tab")?.disabled).toBeFalsy();
  });

  it("offers split right and split down on a pane, and zoom only with company", () => {
    const a = actionsFor(row("pane", { sessionId: "$0", windowId: "@0", paneId: "%1" }), claude);
    expect(ids(a)).toContain("split-right");
    expect(ids(a)).toContain("split-down");
    expect(a.find((x) => x.id === "zoom-pane")?.disabled).toBeFalsy();
    expect(actionsFor(row("pane", { sessionId: "$0", windowId: "@1", paneId: "%3" }), claude).find((x) => x.id === "zoom-pane")?.disabled).toBe(true);
  });

  it("says a node is gone when it has left the snapshot", () => {
    const a = actionsFor(row("session", { sessionId: "$99" }), claude);
    expect(ids(a)).toEqual(["refresh", "attach-tab"]);
    expect(a[1]).toMatchObject({ disabled: true, why: "This session no longer exists on the remote host." });
  });

  it("locks changes while the reading is stale", () => {
    const stale = snap("h-claude", claude.sessions, { stale: { since_ms: 1, message: "x" } });
    const a = actionsFor(row("session", { sessionId: "$0" }), stale);
    expect(a.find((x) => x.id === "kill-session")?.disabled).toBe(true);
    expect(a.find((x) => x.id === "kill-session")?.why).toMatch(/Refresh first/);
  });
});

describe("previews", () => {
  const last = (m: Record<string, number>) => new Map(Object.entries(m));
  const P = PREVIEW_DEFAULTS;

  it("captures the selected pane often, thumbnails rarely and nothing when the monitor is hidden", () => {
    expect(previewsDue(10_000, "a", ["a", "b"], last({ a: 5_000, b: 5_000 }), P, true)).toEqual(["a"]);
    expect(previewsDue(20_000, "a", ["a", "b"], last({ a: 5_000, b: 5_000 }), P, true)).toEqual(["a", "b"]);
    expect(previewsDue(20_000, "a", ["a", "b"], last({ a: 5_000, b: 5_000 }), P, false)).toEqual([]);
  });

  it("never captures a pane that is not on screen", () => {
    expect(previewsDue(99_000, "a", ["a"], last({}), P, true)).toEqual(["a"]);
    expect(previewsDue(99_000, null, [], last({}), P, true)).toEqual([]);
  });

  it("limits how many thumbnails are kept fresh", () => {
    const visible = Array.from({ length: 30 }, (_, i) => `p${i}`);
    const due = previewsDue(99_000, null, visible, last({}), { ...P, maxThumbnails: 5 }, true);
    expect(due).toHaveLength(5);
  });

  it("holds the intervals to sensible bounds", () => {
    expect(clampPrefs({ selectedMs: 1, thumbnailMs: 1e9, maxThumbnails: 999 })).toEqual({ selectedMs: 1000, thumbnailMs: 60000, maxThumbnails: 24 });
    expect(clampPrefs({ selectedMs: NaN })).toEqual(PREVIEW_DEFAULTS);
    expect(clampPrefs({})).toEqual(PREVIEW_DEFAULTS);
  });
});

describe("formatting", () => {
  it("writes sizes and ages", () => {
    expect(bytes(500)).toBe("500 B");
    expect(bytes(284 * 1024 * 1024)).toBe("284 MB");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(ago(2)).toBe("just now");
    expect(ago(30)).toBe("30 s ago");
    expect(ago(300)).toBe("5 min ago");
    expect(ago(7200)).toBe("2 h ago");
  });
});

describe("drawing a captured pane", () => {
  it("keeps colours and attributes and nothing else", () => {
    const lines = parseAnsi("plain \x1b[1;31mred bold\x1b[0m done\nnext");
    expect(plainText(lines)).toBe("plain red bold done\nnext");
    expect(lines[0].map((s) => [s.text, s.fg, s.bold])).toEqual([["plain ", undefined, undefined], ["red bold", "#cd3131", true], [" done", undefined, undefined]]);
  });

  it("reads 256-colour and true-colour sequences", () => {
    const [[a, b]] = parseAnsi("\x1b[38;5;196mA\x1b[48;2;1;2;3mB");
    expect(a.fg).toBe("rgb(255,0,0)");
    expect(b.bg).toBe("rgb(1,2,3)");
    expect(parseAnsi("\x1b[38;5;240mg")[0][0].fg).toBe("rgb(88,88,88)");
  });

  it("swaps colours for reverse video and resets with 0 or an empty parameter", () => {
    const [[a, b]] = parseAnsi("\x1b[7;31mX\x1b[mY");
    expect(a.bg).toBe("#cd3131");
    expect(b.bg).toBeUndefined();
  });

  it("drops every other escape sequence and control character, so a pane cannot move the cursor or set a title", () => {
    const hostile = "a\x1b[2Jb\x1b[10;5Hc\x1b]0;evil title\x07d\x1b]52;c;ZXZpbA==\x1b\\e\x1bPdcs\x1b\\f\x1b(Bg\x07h\x00i\x1b[?1049hj";
    expect(plainText(parseAnsi(hostile))).toBe("abcdefghij");
  });

  it("never produces markup: angle brackets stay text", () => {
    const lines = parseAnsi("<img src=x onerror=alert(1)>&amp;");
    expect(plainText(lines)).toBe("<img src=x onerror=alert(1)>&amp;");
  });

  it("treats a carriage return as the line being rewritten, and tabs as spaces", () => {
    expect(plainText(parseAnsi("old text\rnew"))).toBe("new");
    expect(plainText(parseAnsi("a\tb"))).toBe("a    b");
    expect(plainText(parseAnsi("a\r\nb"))).toBe("a\nb");
  });

  it("keeps only the last lines of a very long capture", () => {
    const text = Array.from({ length: 10_000 }, (_, i) => `l${i}`).join("\n");
    const lines = parseAnsi(text, 100);
    expect(lines).toHaveLength(100);
    expect(plainText(lines).endsWith("l9999")).toBe(true);
  });

  it("survives an unfinished escape at the end", () => {
    expect(plainText(parseAnsi("ok\x1b[3"))).toBe("ok");
    expect(plainText(parseAnsi("ok\x1b"))).toBe("ok");
  });
});

describe("resizing the panels", () => {
  it("grows the left panel with the pointer and the right panel against it, within the limits", () => {
    expect(dragWidth(300, 40, "left", 200, 560)).toBe(340);
    expect(dragWidth(300, 40, "right", 200, 520)).toBe(260);
    expect(dragWidth(300, -500, "left", 200, 560)).toBe(200);
    expect(dragWidth(300, 900, "left", 200, 560)).toBe(560);
    expect(dragWidth(300, 3.4, "left", 200, 560)).toBe(303);
  });

  it("gives a narrow window to the preview: the inspector goes first, then the explorer", () => {
    const open = { explorerOpen: true, inspectorOpen: true };
    expect(layoutFor(1400, open)).toEqual({ explorer: true, inspector: true });
    expect(layoutFor(800, open)).toEqual({ explorer: true, inspector: false });
    expect(layoutFor(500, open)).toEqual({ explorer: false, inspector: false });
    expect(layoutFor(1400, { explorerOpen: false, inspectorOpen: true })).toEqual({ explorer: false, inspector: true });
  });
});
