// The Tmux Monitor's store: who is watched and when, which readings are kept, what a failure looks like, how changes are
// asked for and reported, and how a session is attached or switched in a terminal. The backend is a scripted stand-in.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
let handler: ((e: { payload: unknown }) => void) | undefined;
let fail: Record<string, { code: string; message: string }> = {};
let switched = true;
let generation = 10;
const typed: [string, string][] = [];

const pane = (id: string, over = {}) => ({ id, index: 0, active: true, title: "", command: "bash", path: "/home", pid: 1, width: 80, height: 24, dead: false, in_mode: false, ...over });
const session = (id: string, name: string, over = {}) => ({ id, name, attached: 0, created: 1, activity: 0, group: null, mouse: false, windows: [{ id: "@0", index: 0, name: "w", active: true, layout: "", zoomed: false, activity: false, bell: false, panes: [pane("%0")] }], ...over });
const snapshot = (hostId: string, sessions: unknown[], over = {}) => ({
  host_id: hostId, socket: { kind: "default" }, generation: ++generation, taken_at_ms: Date.now(), version: { major: 3, minor: 4, dev: false, raw: "tmux 3.4" },
  state: sessions.length ? { kind: "ready" } : { kind: "no_server" }, server: { pid: 7, started: 1, socket_path: "/tmp/tmux-1/default" }, sessions, other_sockets: [], stale: null, ...over,
});
let served: Record<string, unknown> = {};

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    if (fail[cmd]) throw fail[cmd];
    switch (cmd) {
      case "tmux_snapshot":
      case "tmux_refresh":
        return served[String(args.hostId)] ?? snapshot(String(args.hostId), []);
      case "tmux_attach_command":
        // Exactly the shape src-tauri/src/tmux/script.rs writes: the marker is quoted, then the whole command is quoted for `sh -c`.
        return `sh -c 'exec env SSHVAULT_TAB='\\''${String(args.tab)}'\\'' tmux -u attach-session -t '\\''${String(args.sessionId)}'\\'''`;
      case "tmux_switch_client":
        return switched;
      case "tmux_operate": {
        const op = args.op as { op: string; session?: string; name?: string };
        const cur = served[String(args.hostId)] as { sessions: { id: string; name: string }[] };
        let sessions = cur.sessions;
        if (op.op === "kill_session") sessions = sessions.filter((s) => s.id !== op.session);
        if (op.op === "rename_session") sessions = sessions.map((s) => (s.id === op.session ? { ...s, name: String(op.name) } : s));
        if (op.op === "new_session") sessions = [...sessions, session("$9", String(op.name))];
        served[String(args.hostId)] = snapshot(String(args.hostId), sessions);
        return { created: op.op === "new_session" ? "$9" : null, snapshot: served[String(args.hostId)] };
      }
      case "tmux_capture":
        return { info: { id: args.paneId, width: 80, height: 24, command: "bash", dead: false, alternate: false, cursor_x: 0, cursor_y: 0, in_mode: false, history: 0, pid: 1, title: "" }, text: `text of ${String(args.paneId)}`, taken_at_ms: Date.now() };
      case "tmux_search":
        return { results: (args.targets as { session_id: string; window_id: string; pane_id: string }[]).map((t) => ({ ...t, hits: [{ line: 1, text: "needle", spans: [[0, 6]], before: [], after: [] }] })), failed: [], searched: (args.targets as unknown[]).length, limited: false, cancelled: false };
      default:
        return null;
    }
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(async (_name: string, h: (e: { payload: unknown }) => void) => {
    handler = h;
    return () => (handler = undefined);
  }),
}));
vi.mock("$lib/dialogs.svelte", () => ({ ask: vi.fn(async () => true), askRemember: vi.fn(async () => ({ ok: true, checked: false })), askText: vi.fn(async () => "typed") }));
vi.mock("$lib/ssh", () => ({ ssh: { write: vi.fn(async (id: string, data: string) => void typed.push([id, data])) } }));

const { tmuxMonitor: mon, markerOf, paneKey } = await import("$lib/stores/tmuxmonitor.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");
const { ui } = await import("$lib/stores/ui.svelte");
const { ask, askText } = await import("$lib/dialogs.svelte");
const { serverKey, DEFAULT_SOCKET } = await import("$lib/tmux/monitor");

const host = (id: string, label: string) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label, port: 22, group: "", tags: [], notes: "", favorite: false } });
const S = (h: string) => serverKey(h, DEFAULT_SOCKET);
const count = (cmd: string, pick?: (a: Record<string, unknown>) => boolean) => calls.filter(([c, a]) => c === cmd && (!pick || pick(a))).length;

beforeEach(() => {
  calls.length = 0;
  typed.length = 0;
  fail = {};
  switched = true;
  generation = 10;
  mon.reset();
  mon.prefs.hostIds = ["a", "b"];
  mon.prefs.seeded = true;
  mon.prefs.sockets = {};
  mon.prefs.tags = {};
  mon.query = "";
  mon.expanded = {};
  vaultStore.hosts = [host("a", "claude"), host("b", "Elastic"), host("c", "spare")] as never;
  served = { a: snapshot("a", [session("$0", "backend", { attached: 1 }), session("$1", "deploy")]), b: snapshot("b", [session("$0", "monitoring")]) };
  ui.tabs = [];
  ui.toasts = [];
  vi.mocked(ask).mockClear();
  vi.mocked(ask).mockResolvedValue(true);
  vi.mocked(askText).mockClear();
  vi.mocked(askText).mockResolvedValue("typed");
});

afterEach(() => {
  mon.stop();
  vi.useRealTimers();
});

describe("watching", () => {
  it("watches each host under one view name, reads them, and stops everything when the page is left", async () => {
    await mon.start();
    expect(calls.filter(([c]) => c === "tmux_watch").map(([, a]) => [a.hostId, a.subscriber, a.intervalMs])).toEqual([["a", "monitor", 4000], ["b", "monitor", 4000]]);
    await vi.waitFor(() => expect(Object.keys(mon.snapshots).sort()).toEqual([S("a"), S("b")]));
    expect(count("tmux_unwatch")).toBeGreaterThanOrEqual(1);
    calls.length = 0;
    mon.stop();
    expect(calls.map(([c]) => c)).toContain("tmux_unwatch");
    expect(calls.find(([c]) => c === "tmux_unwatch")?.[1]).toEqual({ subscriber: "monitor" });
  });

  it("fills an empty explorer once from the hosts that have a terminal open", async () => {
    mon.prefs.hostIds = [];
    mon.prefs.seeded = false;
    ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "b" } }, { id: "p2", target: { kind: "local" } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
    await mon.start();
    expect(mon.prefs.hostIds).toEqual(["b"]);
    mon.removeHost("b");
    await mon.start();
    expect(mon.prefs.hostIds, "once the person emptied it, it stays empty").toEqual([]);
  });

  it("lists only SSH hosts that are not in the explorer yet", () => {
    vaultStore.hosts = [host("a", "claude"), host("c", "spare"), { ...host("d", "desk"), data: { ...host("d", "desk").data, protocol: "rdp" } }] as never;
    expect(mon.addable.map((h) => h.id)).toEqual(["c"]);
  });

  it("applies pushed changes only when they are newer", async () => {
    await mon.start();
    await vi.waitFor(() => expect(mon.snapshots[S("a")]).toBeDefined());
    const have = mon.snapshots[S("a")].generation;
    handler?.({ payload: { ...snapshot("a", [session("$5", "late")]), generation: have - 3 } });
    expect(mon.snapshots[S("a")].sessions.map((s) => s.name)).toEqual(["backend", "deploy"]);
    handler?.({ payload: { ...snapshot("a", [session("$5", "fresh")]), generation: have + 3 } });
    expect(mon.snapshots[S("a")].sessions.map((s) => s.name)).toEqual(["fresh"]);
  });

  it("forgets everything read from hosts when the vault locks", async () => {
    await mon.start();
    await vi.waitFor(() => expect(mon.snapshots[S("a")]).toBeDefined());
    mon.previews[paneKey(S("a"), "%0")] = { at: 1, loading: false, capture: { text: "secret", info: {} as never, taken_at_ms: 1 } };
    mon.reset();
    expect(mon.snapshots).toEqual({});
    expect(mon.previews).toEqual({});
    expect(mon.selected).toBeNull();
    expect(mon.visible).toBe(false);
  });
});

describe("failures", () => {
  it("says so when a host has never been read, instead of showing an empty list", async () => {
    fail = { tmux_refresh: { code: "tmux_exec", message: "no route to host" } };
    await mon.refresh("a", DEFAULT_SOCKET);
    expect(mon.snapshots[S("a")]).toBeUndefined();
    expect(mon.failures[S("a")]).toMatchObject({ tone: "bad", title: "Unable to refresh sessions because the SSH connection is unavailable." });
  });

  it("keeps the list it has when a refresh fails afterwards", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    fail = { tmux_refresh: { code: "tmux_exec", message: "reset" } };
    await mon.refresh("a", DEFAULT_SOCKET);
    expect(mon.snapshots[S("a")].sessions).toHaveLength(2);
    expect(mon.failures[S("a")]).toBeUndefined();
  });

  it("clears a failure once a reading arrives", async () => {
    fail = { tmux_refresh: { code: "tmux_timeout", message: "slow" } };
    await mon.refresh("b", DEFAULT_SOCKET);
    expect(mon.failures[S("b")]).toBeDefined();
    fail = {};
    await mon.refresh("b", DEFAULT_SOCKET);
    expect(mon.failures[S("b")]).toBeUndefined();
  });
});

describe("changes", () => {
  it("takes the reading that confirms the change and tells the person", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    const r = await mon.run("a", DEFAULT_SOCKET, { op: "kill_session", session: "$1" }, "Killed deploy.");
    expect(r).not.toBeNull();
    expect(mon.snapshots[S("a")].sessions.map((s) => s.name)).toEqual(["backend"]);
    expect(ui.toasts.at(-1)?.text).toBe("Killed deploy.");
  });

  it("reports a refusal and refreshes, without changing the tree itself", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    fail = { tmux_operate: { code: "tmux_rejected", message: "duplicate session: backend" } };
    expect(await mon.run("a", DEFAULT_SOCKET, { op: "new_session", name: "backend", cwd: null, command: null })).toBeNull();
    expect(ui.toasts.at(-1)).toMatchObject({ kind: "error", text: "duplicate session: backend" });
    expect(mon.snapshots[S("a")].sessions).toHaveLength(2);
    await vi.waitFor(() => expect(count("tmux_refresh")).toBeGreaterThan(1));
  });

  it("warns, not errors, when the server restarted under the ids", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    fail = { tmux_operate: { code: "tmux_server_changed", message: "the tmux server on this host has restarted; refresh and choose again" } };
    await mon.run("a", DEFAULT_SOCKET, { op: "kill_session", session: "$1" });
    expect(ui.toasts.at(-1)?.kind).toBe("warning");
  });

  it("does not refresh after a request the app itself refused", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    calls.length = 0;
    fail = { tmux_operate: { code: "validation", message: "A session needs a name." } };
    await mon.run("a", DEFAULT_SOCKET, { op: "rename_session", session: "$0", name: "" });
    expect(count("tmux_refresh")).toBe(0);
  });

  it("asks before killing a session and names what will be lost", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    vi.mocked(ask).mockResolvedValue(false);
    await mon.killSession(S("a"), mon.snapshots[S("a")].sessions[0]);
    expect(count("tmux_operate")).toBe(0);
    const msg = String(vi.mocked(ask).mock.calls[0][0]);
    expect(msg).toContain('Kill session "backend"');
    expect(msg).toContain("1 window");
    expect(msg).toContain("1 pane");
    expect(msg).toContain("may be terminated");
    expect(vi.mocked(ask).mock.calls[0][1]).toMatchObject({ danger: true });
    vi.mocked(ask).mockResolvedValue(true);
    await mon.killSession(S("a"), mon.snapshots[S("a")].sessions[0]);
    expect(calls.find(([c]) => c === "tmux_operate")?.[1].op).toEqual({ op: "kill_session", session: "$0" });
  });

  it("warns about what runs in a window before killing it", async () => {
    served.a = snapshot("a", [session("$0", "backend", { windows: [{ id: "@0", index: 0, name: "w", active: true, layout: "", zoomed: false, activity: false, bell: false, panes: [pane("%0", { command: "cargo" }), pane("%1")] }] })]);
    await mon.refresh("a", DEFAULT_SOCKET);
    await mon.killWindow(S("a"), "@0");
    expect(String(vi.mocked(ask).mock.calls[0][0])).toContain("cargo");
  });

  it("creates a session and selects it", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    vi.mocked(askText).mockResolvedValueOnce("fresh").mockResolvedValueOnce("");
    await mon.newSession("a", DEFAULT_SOCKET);
    expect(calls.find(([c]) => c === "tmux_operate")?.[1].op).toEqual({ op: "new_session", name: "fresh", cwd: null, command: null });
    expect(mon.selected).toBe(`s:${S("a")}:$9`);
  });

  it("gives a renamed session its tags back under the new name", async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    mon.setTags(S("a"), "backend", ["Production", " ", "Database"]);
    expect(mon.tagsFor(S("a"), "backend")).toEqual(["Production", "Database"]);
    vi.mocked(askText).mockResolvedValue("api");
    await mon.renameSession(S("a"), mon.snapshots[S("a")].sessions[0]);
    expect(mon.tagsFor(S("a"), "api")).toEqual(["Production", "Database"]);
    expect(mon.tagsFor(S("a"), "backend")).toEqual([]);
  });
});

describe("attaching", () => {
  beforeEach(async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    await mon.refresh("b", DEFAULT_SOCKET);
  });

  it("opens a new tab on the right host with the command the backend built, marked so it can be found again", async () => {
    const open = vi.spyOn(ui, "openTerminal").mockImplementation(() => {});
    expect(await mon.attach(S("a"), "$1", "tab")).toBe(true);
    const [hostId, title, command] = open.mock.calls[0];
    expect(hostId).toBe("a");
    expect(title).toBe("claude · deploy");
    const marker = markerOf(command);
    expect(marker).toMatch(/^[0-9a-f-]{36}$|^m\d+/);
    expect(mon.attached[marker!]).toEqual({ server: S("a"), sessionId: "$1" });
    const call = calls.find(([c]) => c === "tmux_attach_command")![1];
    expect(call).toMatchObject({ hostId: "a", sessionId: "$1", detachOthers: false });
    open.mockRestore();
  });

  it("splits beside the active terminal, and uses a new tab when the tab is full", async () => {
    const open = vi.spyOn(ui, "openTerminal").mockImplementation(() => {});
    const split = vi.spyOn(ui, "splitWith").mockReturnValueOnce(true).mockReturnValueOnce(false);
    ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "a" } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
    ui.activeTabId = "t1";
    await mon.attach(S("a"), "$0", "right");
    expect(split.mock.calls[0][0]).toBe("vertical");
    expect(split.mock.calls[0][1]).toMatchObject({ kind: "host", hostId: "a" });
    expect(open).not.toHaveBeenCalled();
    await mon.attach(S("a"), "$0", "down");
    expect(split.mock.calls[1][0]).toBe("horizontal");
    expect(open).toHaveBeenCalledTimes(1);
    expect(ui.toasts.at(-1)?.text).toMatch(/tab is full/);
    open.mockRestore();
    split.mockRestore();
  });

  it("refuses a session that has left the list, without asking the host", async () => {
    calls.length = 0;
    expect(await mon.attach(S("a"), "$77", "tab")).toBe(false);
    expect(ui.toasts.at(-1)?.text).toBe("This session no longer exists on the remote host.");
    expect(count("tmux_attach_command")).toBe(0);
  });

  it("reports an attach that failed in the words the person expects", async () => {
    fail = { tmux_attach_command: { code: "tmux_rejected", message: "That session is not in the list." } };
    expect(await mon.attach(S("a"), "$0", "tab")).toBe(false);
    expect(ui.toasts.at(-1)?.text).toMatch(/^Unable to attach to the selected tmux session\./);
  });

  it("replaces by moving the terminal's own tmux client, typing nothing", async () => {
    const marker = "11111111-2222-3333-4444-555555555555";
    mon.attached[marker] = { server: S("a"), sessionId: "$0" };
    const command = `sh -c 'exec env SSHVAULT_TAB='\\''${marker}'\\'' tmux -u attach-session -t '\\''$0'\\'''`;
    ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "a", command } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
    ui.activeTabId = "t1";
    expect(await mon.attach(S("a"), "$1", "replace")).toBe(true);
    expect(calls.find(([c]) => c === "tmux_switch_client")![1]).toMatchObject({ hostId: "a", tab: marker, sessionId: "$1" });
    expect(typed).toEqual([]);
    expect(mon.attached[marker]).toEqual({ server: S("a"), sessionId: "$1" });
    expect(mon.attachmentOf("p1")?.sessionId).toBe("$1");
  });

  it("asks before typing into a terminal that is not attached, and types nothing if declined", async () => {
    ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "a" } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
    ui.activeTabId = "t1";
    vi.mocked(ask).mockResolvedValue(false);
    expect(await mon.attach(S("a"), "$1", "replace")).toBe(false);
    expect(typed).toEqual([]);
    vi.mocked(ask).mockResolvedValue(true);
    expect(await mon.attach(S("a"), "$1", "replace")).toBe(true);
    expect(typed).toHaveLength(1);
    expect(typed[0][0]).toBe("p1");
    expect(typed[0][1].endsWith("\r")).toBe(true);
  });

  it("will not replace a terminal that is on another host", async () => {
    ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "b" } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
    ui.activeTabId = "t1";
    calls.length = 0;
    expect(await mon.attach(S("a"), "$0", "replace")).toBe(false);
    expect(count("tmux_switch_client")).toBe(0);
    expect(typed).toEqual([]);
    expect(ui.toasts.at(-1)?.text).toMatch(/not on this host/);
  });
});

describe("previews", () => {
  beforeEach(async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
  });

  it("captures what is asked for and keeps the text by pane, in memory only", async () => {
    const key = paneKey(S("a"), "%0");
    await mon.capture(key);
    expect(mon.previews[key].capture?.text).toBe("text of %0");
    expect(calls.find(([c]) => c === "tmux_capture")![1]).toMatchObject({ hostId: "a", paneId: "%0", ansi: true });
  });

  it("explains a pane that cannot be captured, and a server that restarted", async () => {
    const key = paneKey(S("a"), "%0");
    fail = { tmux_capture: { code: "tmux_rejected", message: "can't find pane: %0" } };
    await mon.capture(key);
    expect(mon.previews[key].error).toBe("Pane output could not be captured.");
    fail = { tmux_capture: { code: "tmux_server_changed", message: "x" } };
    await mon.capture(key);
    expect(mon.previews[key].error).toMatch(/restarted/);
  });

  it("captures the selected pane on a timer while the monitor is on screen, and stops when it is not", async () => {
    vi.useFakeTimers();
    await mon.start();
    mon.select(`p:${S("a")}:%0`);
    mon.setVisiblePanes([paneKey(S("a"), "%0")]);
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(1100);
    expect(count("tmux_capture")).toBe(1);
    mon.stop();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(count("tmux_capture")).toBe(0);
  });

  it("drops previews of panes that have left the server", async () => {
    const gone = paneKey(S("a"), "%7");
    mon.previews[gone] = { at: 1, loading: false, capture: { text: "old", info: {} as never, taken_at_ms: 1 } };
    mon.ingest(snapshot("a", [session("$0", "backend")]) as never);
    expect(mon.previews[gone]).toBeUndefined();
  });
});

describe("searching pane output", () => {
  beforeEach(async () => {
    await mon.refresh("a", DEFAULT_SOCKET);
    await mon.refresh("b", DEFAULT_SOCKET);
    mon.searchText = "needle";
  });

  it("searches every watched server and keeps each hit tied to its server, since pane ids repeat between servers", async () => {
    await mon.search();
    const searches = calls.filter(([c]) => c === "tmux_search");
    expect(searches.map(([, a]) => a.hostId).sort()).toEqual(["a", "b"]);
    expect(mon.searchReport?.results.map((r) => [r.server, r.pane_id]).sort()).toEqual([[S("a"), "%0"], [S("a"), "%0"], [S("b"), "%0"]].sort());
    expect(mon.searchReport?.searched).toBe(3);
  });

  it("limits the search to the selected server, session, window or pane", async () => {
    mon.select(`s:${S("b")}:$0`);
    mon.searchScope = "host";
    await mon.search();
    expect(calls.filter(([c]) => c === "tmux_search").map(([, a]) => a.hostId)).toEqual(["b"]);
    calls.length = 0;
    mon.select(`s:${S("a")}:$1`);
    mon.searchScope = "session";
    await mon.search();
    const t = (calls.find(([c]) => c === "tmux_search")![1].targets as { session_id: string }[]);
    expect(t.map((x) => x.session_id)).toEqual(["$1"]);
    mon.searchScope = "all";
  });

  it("sends the options as typed and never searches for nothing", async () => {
    mon.searchCase = true;
    mon.searchRegex = true;
    await mon.search();
    expect(calls.find(([c]) => c === "tmux_search")![1].query).toEqual({ text: "needle", case_sensitive: true, regex: true });
    calls.length = 0;
    mon.searchText = "";
    await mon.search();
    expect(count("tmux_search")).toBe(0);
    mon.searchCase = false;
    mon.searchRegex = false;
  });

  it("reports a search that failed", async () => {
    fail = { tmux_search: { code: "validation", message: "That is not a valid pattern: (" } };
    await mon.search();
    expect(mon.searchError).toBe("That is not a valid pattern: (");
    expect(mon.searching).toBe(false);
  });

  it("opens the path to a hit and selects its pane", () => {
    mon.reveal(S("a"), "$0", "@0", "%0");
    expect(mon.selected).toBe(`p:${S("a")}:%0`);
    expect(mon.rows.some((r) => r.kind === "pane" && r.server === S("a"))).toBe(true);
  });
});

describe("the panel beside a terminal", () => {
  it("can be turned off for one host and stays on for the rest", () => {
    expect(mon.panelEnabledFor("a")).toBe(true);
    mon.setPanelHost("a", false);
    expect(mon.panelEnabledFor("a")).toBe(false);
    expect(mon.panelEnabledFor("b")).toBe(true);
    mon.setPanelHost("a", true);
    expect(mon.panelEnabledFor("a")).toBe(true);
    mon.prefs.panel.enabled = false;
    expect(mon.panelEnabledFor("b")).toBe(false);
    mon.prefs.panel.enabled = true;
  });
});

describe("markers", () => {
  it("reads the tab marker out of the command exactly as the backend writes it (quoted twice), and only that", () => {
    // The same text src-tauri/src/tmux/tests.rs asserts the backend produces.
    expect(markerOf("sh -c 'exec env SSHVAULT_TAB='\\''abc-123'\\'' tmux attach'")).toBe("abc-123");
    // A command typed by hand with single quotes still works.
    expect(markerOf("env SSHVAULT_TAB='abc-123' tmux attach-session -t '$0'")).toBe("abc-123");
    expect(markerOf("env SSHVAULT_TAB='" + "a".repeat(80) + "' tmux")).toBeNull();
    expect(markerOf("ls")).toBeNull();
    expect(markerOf(undefined)).toBeNull();
    expect(markerOf("env SSHVAULT_TAB='bad;marker' tmux")).toBeNull();
  });
});
