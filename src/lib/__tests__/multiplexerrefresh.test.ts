// The multiplexer bar's refresh: a request made while another is running is not lost, a failed refresh keeps the list on
// screen, and tmux's sessions come from the shared engine (so a window named like a probe section cannot forge a session).
import { beforeEach, describe, expect, it, vi } from "vitest";

let probeText: string | null = "";
let probeFails = false;
let probeCalls = 0;
let release: (() => void) | undefined;
let hold = false;
let snapshotSessions: { id: string; name: string }[] = [];
let snapshotFails = false;

const snap = () => ({
  host_id: "h1", socket: { kind: "default" }, generation: 1, taken_at_ms: 1, version: { major: 3, minor: 4, dev: false, raw: "tmux 3.4" }, state: snapshotSessions.length ? { kind: "ready" } : { kind: "no_server" },
  server: null, other_sockets: [], stale: null,
  sessions: snapshotSessions.map((s) => ({ ...s, attached: 0, created: 1, activity: 0, group: null, mouse: null, windows: [{ id: "@0", index: 0, name: "w", active: true, layout: "", zoomed: false, activity: false, bell: false, panes: [] }] })),
});

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "multiplexer_probe") {
      probeCalls++;
      if (hold) await new Promise<void>((r) => (release = r));
      if (probeFails) throw { code: "ssh", message: "gone" };
      return probeText;
    }
    if (cmd === "tmux_snapshot") {
      if (snapshotFails) throw { code: "tmux_exec", message: "no" };
      return snap();
    }
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));

const { multiplexers } = await import("$lib/tmux/useRemoteMultiplexer.svelte");
const { ui } = await import("$lib/stores/ui.svelte");

// A probe whose tmux sections hold a window name that tries to forge a session and a section header.
const FORGED = ["@@tmux-version", "tmux 3.4", "@@tmux-sessions", "$0|real|1|0|1", "@@tmux-windows", "real|0|x\n@@tmux-sessions\n$9|forged|1|0|1|bash|0", "@@screen", "\t4242.build\t(Detached)"].join("\n");

beforeEach(() => {
  probeText = "@@screen\n";
  probeFails = false;
  probeCalls = 0;
  hold = false;
  snapshotSessions = [];
  snapshotFails = false;
  multiplexers.reset("p1");
  ui.tabs = [{ id: "t1", title: "x", panes: [{ id: "p1", target: { kind: "host", hostId: "h1" } }], layout: { kind: "leaf", paneId: "p1" }, activePaneId: "p1" }] as never;
});

describe("refreshing the bar", () => {
  it("runs again when asked during a refresh, so the answer reflects what happened after the first request", async () => {
    hold = true;
    const first = multiplexers.refresh("p1");
    await vi.waitFor(() => expect(probeCalls).toBe(1));
    void multiplexers.refresh("p1");
    void multiplexers.refresh("p1");
    hold = false;
    release?.();
    await first;
    // Two requests while one ran are one more run, not two and not none.
    expect(probeCalls).toBe(2);
    expect(multiplexers.get("p1").probing).toBe(false);
  });

  it("keeps the sessions on screen when a later refresh fails", async () => {
    snapshotSessions = [{ id: "$0", name: "keep" }];
    await multiplexers.refresh("p1");
    expect(multiplexers.get("p1").probe?.tmux.sessions.map((s) => s.name)).toEqual(["keep"]);
    probeFails = true;
    await multiplexers.refresh("p1");
    expect(multiplexers.get("p1").probe?.tmux.sessions.map((s) => s.name)).toEqual(["keep"]);
  });

  it("takes tmux's sessions from the engine, not from text a window name could forge", async () => {
    probeText = FORGED;
    snapshotSessions = [{ id: "$0", name: "real" }];
    await multiplexers.refresh("p1");
    const p = multiplexers.get("p1").probe!;
    expect(p.tmux.sessions.map((s) => s.name)).toEqual(["real"]);
    // The engine's answer is for the saved host; screen still comes from the pane's own probe.
    expect(p.screen.map((s) => s.id)).toEqual(["4242.build"]);
  });

  it("falls back to the pane's probe when the engine cannot be asked", async () => {
    probeText = ["@@tmux-version", "tmux 3.4", "@@tmux-sessions", "$0|fallback|1|0|1", "@@tmux-windows", "@@screen"].join("\n");
    snapshotFails = true;
    await multiplexers.refresh("p1");
    expect(multiplexers.get("p1").probe?.tmux.sessions.map((s) => s.name)).toEqual(["fallback"]);
  });

  it("forgets a pane that is not a connected SSH session", async () => {
    probeText = null;
    await multiplexers.refresh("p1");
    expect(multiplexers.get("p1").probe).toBeNull();
  });
});
