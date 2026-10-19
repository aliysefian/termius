// The client against a real tmux, on a private socket so no session of yours is touched. Skipped without tmux.
// The app's type setup has no Node types, hence the suppressed imports and the small stand-ins below.
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { spawn, spawnSync } from "node:child_process";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TmuxControlClient, isIdleShell } from "$lib/tmux/TmuxControlClient";
import { parseTmuxVersion } from "$lib/tmux/discovery";
import { layoutPanes } from "$lib/tmux/tmuxLayoutParser";

/* eslint-disable @typescript-eslint/no-explicit-any */
declare const process: { pid: number; env: Record<string, string | undefined> };
type Proc = { stdin: { write(t: string): void }; stdout: { on(e: "data", f: (d: Uint8Array) => void): void }; kill(): void };

const have = spawnSync("tmux", ["-V"]).status === 0;
const sock = `sshvault-test-${process.pid}`;
const dec = new TextDecoder();

const until = async (what: string, ok: () => boolean, ms = 3000) => {
  const end = Date.now() + ms;
  while (!ok()) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 20));
  }
};

describe.skipIf(!have)("against a real tmux", () => {
  let proc: Proc;
  let client: TmuxControlClient;
  const screen = new Map<number, string>();
  /** The pane the session started with; later splits make others active. */
  let first = 0;

  beforeAll(async () => {
    const version = parseTmuxVersion(dec.decode(spawnSync("tmux", ["-V"]).stdout));
    // `-C` talks control mode over plain pipes, so no terminal is needed.
    proc = spawn("tmux", ["-L", sock, "-f", "/dev/null", "-C", "new-session", "-s", "t", "-x", "100", "-y", "30", "sh"], {
      env: { ...process.env, TERM: "xterm", PS1: "$ " },
    });
    client = new TmuxControlClient({ write: (t) => proc.stdin.write(t), version, debounceMs: 10, plainControl: true });
    proc.stdout.on("data", (d: Uint8Array) => client.feed(new Uint8Array(d)));
    client.on((e) => {
      if (e.type === "output") screen.set(e.paneId, (screen.get(e.paneId) ?? "") + dec.decode(e.data));
    });
    await until("the session", () => client.ready && client.windows.size === 1);
    first = client.windowList[0].activePaneId!;
  });

  afterAll(() => {
    spawnSync("tmux", ["-L", sock, "kill-server"]);
    proc?.kill();
  });

  it("knows the session, its window and pane", () => {
    expect(client.sessionName).toBe("t");
    const w = client.windowList[0];
    expect(w.active).toBe(true);
    expect(w.layout && layoutPanes(w.layout.root)).toHaveLength(1);
  });

  it("types into a pane and reads what it prints", async () => {
    const pane = first;
    client.sendKeys(pane, "echo marker$((40+2))\r");
    await until("output", () => (screen.get(pane) ?? "").includes("marker42"));
  });

  it("types text with quotes and control characters the old way, too (no send-keys -H)", async () => {
    const old = new TmuxControlClient({ write: (t) => proc.stdin.write(t), version: parseTmuxVersion("tmux 2.9"), debounceMs: 10, plainControl: true });
    const pane = first;
    old.sendKeys(pane, "echo 'it'\\''s'-ok\r");
    await until("output", () => (screen.get(pane) ?? "").includes("it's-ok"));
  });

  it("sends characters outside ASCII as they are", async () => {
    const pane = first;
    client.sendKeys(pane, "echo ✓é\r");
    await until("output", () => (screen.get(pane) ?? "").includes("✓é"));
  });

  it("splits a window and follows the new layout", async () => {
    client.splitWindow(null, "side");
    await until("two panes", () => {
      const l = client.windowList[0].layout;
      return !!l && layoutPanes(l.root).length === 2;
    });
    const panes = layoutPanes(client.windowList[0].layout!.root);
    expect(panes[0].x).toBe(0);
    expect(panes[1].x).toBeGreaterThan(panes[0].w);
  });

  it("resizes a pane and sees the layout change", async () => {
    const [left] = layoutPanes(client.windowList[0].layout!.root);
    client.resizePaneAxis(left.paneId, "x", 30);
    await until("30 columns", () => layoutPanes(client.windowList[0].layout!.root)[0].w === 30);
  });

  it("opens a window, renames it, and closes it", async () => {
    client.newWindow();
    await until("two windows", () => client.windows.size === 2);
    const second = client.windowList[1];
    await client.send(`rename-window -t @${second.id} build`);
    await until("the new name", () => client.windows.get(second.id)?.name === "build");
    client.killWindow(second.id);
    await until("one window", () => client.windows.size === 1);
  });

  it("captures a pane's screen", async () => {
    const pane = first;
    const lines = await client.capturePane(pane);
    expect(lines.join("\n")).toContain("marker42");
  });

  it("snapshots a pane, with the barrier falling between the output before it and after", async () => {
    const pane = first;
    const seen: string[] = [];
    client.on((e) => e.type === "output" && e.paneId === pane && seen.push(`out:${dec.decode(e.data)}`));
    const snap = client.snapshotPane(pane, 100, () => seen.push("barrier"));
    client.sendKeys(pane, "echo after$((1+1))\r");
    const s = await snap;
    expect(s.lines.join("\n")).toContain("marker42");
    expect(s.alternateLines).toBeNull();
    expect(s.cursor.y).toBeGreaterThan(0);
    await until("later output", () => seen.join("").includes("after2"));
    expect(seen.indexOf("barrier")).toBeGreaterThanOrEqual(0);
    // What was typed after the snapshot was asked for comes after the barrier, not before.
    expect(seen.findIndex((x) => x.includes("after"))).toBeGreaterThan(seen.indexOf("barrier"));
  });

  it("reports the program in a pane's foreground", async () => {
    const pane = first;
    expect(isIdleShell(await client.foregroundCommand(pane))).toBe(true);
  });

  it("ends when detached", async () => {
    let exited = false;
    client.on((e) => e.type === "exit" && (exited = true));
    client.detach();
    await until("exit", () => exited);
    expect(client.closed).toBe(true);
  });
});
