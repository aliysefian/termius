import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TmuxControlClient, isIdleShell, quoteArg, sendKeysCommands } from "$lib/tmux/TmuxControlClient";
import { layoutChecksum } from "$lib/tmux/tmuxLayoutParser";
import { parseTmuxVersion } from "$lib/tmux/discovery";

const enc = new TextEncoder();
const sum = (b: string) => `${layoutChecksum(b)},${b}`;

function setup(version = "tmux 3.4", inControlMode = true) {
  const sent: string[] = [];
  const client = new TmuxControlClient({ write: (t) => sent.push(t), version: parseTmuxVersion(version), debounceMs: 50 });
  const feed = (s: string) => client.feed(enc.encode(s));
  if (inControlMode) feed("\x1bP1000p");
  /** Answer the oldest unanswered command. */
  let n = 100;
  const reply = (lines: string[] = [], ok = true) => feed(`%begin 1 ${++n} 1\n${lines.join("\n")}${lines.length ? "\n" : ""}%${ok ? "end" : "error"} 1 ${n} 1\n`);
  return { client, sent, feed, reply };
}

describe("send-keys", () => {
  it("sends bytes as hex", () => {
    expect(sendKeysCommands(3, enc.encode("ls\r"), true)).toEqual(["send-keys -t %3 -H 6c 73 0d"]);
    expect(sendKeysCommands(3, enc.encode("é"), true)).toEqual(["send-keys -t %3 -H c3 a9"]);
  });
  it("splits a long paste into short lines", () => {
    const cmds = sendKeysCommands(1, new Uint8Array(600).fill(0x61), true);
    expect(cmds).toHaveLength(3);
    expect(cmds.every((c) => c.length < 900)).toBe(true);
  });
  it("without -H: text as literals, control keys by name", () => {
    expect(sendKeysCommands(2, enc.encode("ls -l\r"), false)).toEqual(["send-keys -t %2 -l 'ls -l'", "send-keys -t %2 Enter"]);
    expect(sendKeysCommands(2, enc.encode("\x1b[A\x03it's"), false)).toEqual(["send-keys -t %2 Up", "send-keys -t %2 C-c", `send-keys -t %2 -l 'it'\\''s'`]);
    expect(sendKeysCommands(2, enc.encode("\x7f\t\x1b"), false)).toEqual(["send-keys -t %2 BSpace", "send-keys -t %2 Tab", "send-keys -t %2 Escape"]);
  });
  it("quotes arguments", () => {
    expect(quoteArg("a b")).toBe("'a b'");
    expect(quoteArg("it's")).toBe("'it'\\''s'");
  });
  it("picks hex only for tmux 3.0 and later", () => {
    const old = setup("tmux 2.9a");
    old.client.sendKeys(0, "a");
    expect(old.sent).toEqual(["send-keys -t %0 -l 'a'\n"]);
    const now = setup("tmux 3.0");
    now.client.sendKeys(0, "a");
    expect(now.sent).toEqual(["send-keys -t %0 -H 61\n"]);
  });
});

describe("client", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("pairs replies with commands in order and skips tmux's own", async () => {
    const { client, feed, reply } = setup();
    const a = client.send("display-message -p x");
    const b = client.send("display-message -p y");
    feed("%begin 1 5 0\n%end 1 5 0\n"); // the attach itself
    reply(["one"]);
    reply(["two"]);
    expect(await a).toEqual(["one"]);
    expect(await b).toEqual(["two"]);
  });

  it("rejects a command tmux refuses, and any that are waiting when it exits", async () => {
    const { client, feed, reply } = setup();
    const bad = client.send("kill-pane -t %99");
    const waiting = client.send("list-windows");
    reply(["can't find pane: %99"], false);
    await expect(bad).rejects.toThrow("can't find pane");
    const events: string[] = [];
    client.on((e) => events.push(e.type));
    feed("%exit\n\x1b\\");
    await expect(waiting).rejects.toThrow("ended");
    expect(events).toEqual(["exit"]);
    expect(client.closed).toBe(true);
    await expect(client.send("x")).rejects.toThrow();
  });

  it("refuses a command with a newline in it", async () => {
    await expect(setup().client.send("new-window\nkill-server")).rejects.toThrow("one line");
  });

  it("emits pane output and gives back what isn't protocol", () => {
    const { client } = setup("tmux 3.4", false);
    const out: string[] = [];
    client.on((e) => e.type === "output" && out.push(`${e.paneId}:${new TextDecoder().decode(e.data)}`));
    const raw = client.feed(enc.encode("$ tmux -CC\r\n\x1bP1000p%output %2 hi\\015\n"));
    expect(new TextDecoder().decode(raw)).toBe("$ tmux -CC\r\n");
    expect(out).toEqual(["2:hi\r"]);
  });

  it("loads the windows when the session is announced, and follows layout changes", async () => {
    const { client, feed, reply, sent } = setup();
    const l1 = sum("80x24,0,0,0");
    feed("%begin 1 1 0\n%end 1 1 0\n%session-changed $0 main\n");
    expect(client.sessionName).toBe("main");
    expect(sent[0]).toMatch(/^list-windows -F /);
    reply([`@0|0|1|${l1}|${l1}|%0|zsh`, `@1|1|0|${sum("80x24,0,0,1")}||%1|deploy|v2`]);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.ready).toBe(true);
    expect(client.windowList.map((w) => [w.id, w.name, w.active, w.activePaneId])).toEqual([
      [0, "zsh", true, 0],
      [1, "deploy|v2", false, 1],
    ]);
    expect(client.activeWindowId).toBe(0);

    const l2 = sum("80x24,0,0{40x24,0,0,0,39x24,41,0,2}");
    feed(`%layout-change @0 ${l2} ${l2} *\n`);
    expect(client.windows.get(0)!.layout!.root.type).toBe("split");
    // A layout that fails its checksum is ignored, not half-applied.
    feed(`%layout-change @0 0000,80x24,0,0,5 0000,80x24,0,0,5 *\n`);
    expect(client.windows.get(0)!.layout!.root.type).toBe("split");

    feed("%window-renamed @1 build\n%window-pane-changed @1 %7\n%session-window-changed $0 @1\n");
    expect(client.windows.get(1)).toMatchObject({ name: "build", activePaneId: 7, active: true });
    expect(client.windows.get(0)!.active).toBe(false);
  });

  it("drops a closed window and asks again on %window-add, once for several", async () => {
    const { client, feed, reply, sent } = setup();
    feed("%session-changed $0 main\n");
    reply([`@0|0|1|${sum("80x24,0,0,0")}||%0|a`]);
    await vi.advanceTimersByTimeAsync(0);
    sent.length = 0;
    feed("%window-add @1\n%window-add @2\n");
    expect(sent).toHaveLength(1);
    reply([`@0|0|1|${sum("80x24,0,0,0")}||%0|a`, `@1|1|0|${sum("80x24,0,0,1")}||%1|b`, `@2|2|0|${sum("80x24,0,0,2")}||%2|c`]);
    await vi.advanceTimersByTimeAsync(0);
    // The second add arrived while the first ask was out: one more round, not two.
    expect(sent).toHaveLength(2);
    reply([`@0|0|1|${sum("80x24,0,0,0")}||%0|a`, `@2|1|0|${sum("80x24,0,0,2")}||%2|c`]);
    await vi.advanceTimersByTimeAsync(0);
    expect(client.windowList.map((w) => w.id)).toEqual([0, 2]);
    feed("%window-close @2\n");
    expect(client.windows.has(2)).toBe(false);
  });

  it("maps the interface's controls to tmux commands", () => {
    const { client, sent } = setup();
    client.newWindow();
    client.splitWindow(4, "side");
    client.splitWindow(null, "below");
    client.killPane(4);
    client.selectPane(2);
    client.detach();
    expect(sent).toEqual(["new-window\n", "split-window -h -t %4\n", "split-window -v\n", "kill-pane -t %4\n", "select-pane -t %2\n", "detach-client\n"]);
  });

  it("sends only the last of a run of resizes", async () => {
    const { client, sent } = setup();
    for (const c of [100, 110, 120]) client.resizeClient(c, 40);
    client.resizePane(3, 60, 20);
    client.resizePane(3, 61, 20);
    client.resizePaneAxis(5, "x", 30);
    expect(sent).toEqual([]);
    await vi.advanceTimersByTimeAsync(60);
    expect(sent.sort()).toEqual(["refresh-client -C 120x40\n", "resize-pane -t %3 -x 61 -y 20\n", "resize-pane -t %5 -x 30\n"].sort());
  });

  it("asks for history with capture-pane", async () => {
    const { client, sent, reply } = setup();
    const p = client.capturePane(1, 1000);
    expect(sent[0]).toBe("capture-pane -p -e -J -t %1 -S -1000\n");
    reply(["a", "b"]);
    expect(await p).toEqual(["a", "b"]);
  });

  it("knows an idle shell from a running program", () => {
    expect(["zsh", "bash", "-bash", "fish"].every(isIdleShell)).toBe(true);
    expect(["vim", "nginx", "htop", "ssh"].some(isIdleShell)).toBe(false);
  });
});
