import { describe, expect, it } from "vitest";
import { TmuxControlModeParser, unescapeOutput, type TmuxEvent } from "$lib/tmux/TmuxControlModeParser";
import { layoutChecksum, layoutDividers, layoutPanes, layoutSize, parseLayout } from "$lib/tmux/tmuxLayoutParser";

const enc = new TextEncoder();
const dec = new TextDecoder();
const DCS = "\x1bP1000p";

/** A layout string with its checksum computed, from just the body. */
const withSum = (body: string) => `${layoutChecksum(body)},${body}`;

describe("layout parser", () => {
  it("checks the checksum the way tmux does", () => {
    // Real strings printed by tmux 3.4.
    expect(layoutChecksum("80x24,0,0,0")).toBe("b25d");
    expect(layoutChecksum("80x24,0,0{40x24,0,0,0,39x24,41,0,1}")).toBe("8205");
  });

  it("reads a single pane", () => {
    const l = parseLayout("b25d,80x24,0,0,0")!;
    expect(l.root).toEqual({ type: "pane", x: 0, y: 0, w: 80, h: 24, paneId: 0 });
    expect(layoutSize(l)).toEqual({ cols: 80, rows: 24 });
  });

  it("reads side-by-side and stacked splits, nested", () => {
    const l = parseLayout(withSum("159x48,0,0{79x48,0,0,0,79x48,80,0[79x24,80,0,2,79x23,80,25,3]}"))!;
    expect(l.root.type === "split" && l.root.dir).toBe("row");
    expect(layoutPanes(l.root)).toEqual([
      { x: 0, y: 0, w: 79, h: 48, paneId: 0 },
      { x: 80, y: 0, w: 79, h: 24, paneId: 2 },
      { x: 80, y: 25, w: 79, h: 23, paneId: 3 },
    ]);
    const right = l.root.type === "split" ? l.root.children[1] : null;
    expect(right?.type === "split" && right.dir).toBe("column");
  });

  it("finds the dividers, and a pane to resize for each", () => {
    const l = parseLayout(withSum("159x48,0,0{79x48,0,0,0,79x48,80,0[79x24,80,0,2,79x23,80,25,3]}"))!;
    const d = layoutDividers(l.root);
    expect(d).toHaveLength(2);
    // The line between the halves, then the one inside the right half.
    expect(d[0]).toMatchObject({ dir: "row", at: { x: 79, y: 0, w: 1, h: 48 }, edgePaneId: 0 });
    expect(d[1]).toMatchObject({ dir: "column", at: { x: 80, y: 24, w: 79, h: 1 }, edgePaneId: 2 });
  });

  it("aims a divider at the pane touching it, through nested splits", () => {
    // Left half is itself stacked; the divider to its right touches the lower pane's edge as well as the upper.
    const l = parseLayout(withSum("159x48,0,0{79x48,0,0[79x24,0,0,0,79x23,0,25,1],79x48,80,0,2}"))!;
    const d = layoutDividers(l.root).find((x) => x.dir === "row")!;
    expect([0, 1]).toContain(d.edgePaneId);
  });

  it("refuses a bad checksum, junk and trailing text", () => {
    expect(parseLayout("0000,80x24,0,0,0")).toBeNull();
    expect(parseLayout("")).toBeNull();
    expect(parseLayout("b25d,80x24,0,0")).toBeNull();
    expect(parseLayout(withSum("80x24,0,0,0") + "x")).toBeNull();
    expect(parseLayout(withSum("80x24,0,0{40x24,0,0,0"))).toBeNull();
  });
});

describe("output unescaping", () => {
  it("decodes octal bytes and leaves the rest", () => {
    expect(dec.decode(unescapeOutput(enc.encode("a\\033[1mb\\015\\012")))).toBe("a\x1b[1mb\r\n");
    expect(dec.decode(unescapeOutput(enc.encode("back\\134slash")))).toBe("back\\slash");
  });
  it("keeps UTF-8 bytes intact", () => {
    expect(dec.decode(unescapeOutput(enc.encode("héllo ✓")))).toBe("héllo ✓");
  });
  it("leaves a backslash that is not followed by three octal digits", () => {
    expect(dec.decode(unescapeOutput(enc.encode("a\\x \\08 \\")))).toBe("a\\x \\08 \\");
  });
});

function run(parser: TmuxControlModeParser, ...chunks: string[]) {
  const events: TmuxEvent[] = [];
  let raw = "";
  for (const c of chunks) {
    const f = parser.feed(enc.encode(c));
    events.push(...f.events);
    raw += dec.decode(f.raw);
  }
  return { events, raw };
}

describe("control mode parser", () => {
  it("passes the shell through until the introducer, then takes over", () => {
    const p = new TmuxControlModeParser();
    const { events, raw } = run(p, `$ tmux -CC attach\r\n${DCS}%begin 1 2 0\r\n%end 1 2 0\r\n%window-add @0\r\n`);
    expect(raw).toBe("$ tmux -CC attach\r\n");
    expect(p.active).toBe(true);
    expect(events).toEqual([
      { type: "response", num: 2, ok: true, lines: [], flags: "0" },
      { type: "window-add", windowId: 0 },
    ]);
  });

  it("finds an introducer split across chunks, and a lookalike that isn't one", () => {
    const p = new TmuxControlModeParser();
    const a = run(p, "x\x1b", "P10", "00p%window-add @4\n");
    expect(a.raw).toBe("x");
    expect(a.events).toEqual([{ type: "window-add", windowId: 4 }]);
    const q = new TmuxControlModeParser();
    const b = run(q, "a\x1b", "[0mb");
    expect(b.raw).toBe("a\x1b[0mb");
    expect(q.active).toBe(false);
  });

  it("joins a line split across chunks", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "%output %3 he", "llo\\015\\012\n");
    expect(events).toHaveLength(1);
    const e = events[0];
    expect(e.type === "output" && e.paneId).toBe(3);
    expect(e.type === "output" && dec.decode(e.data)).toBe("hello\r\n");
  });

  it("routes %output to its pane without leaking it as raw text", () => {
    const p = new TmuxControlModeParser();
    const { events, raw } = run(p, `${DCS}%output %0 \\033[32mok\\033[0m\r\n%output %12 two\r\n`);
    expect(raw).toBe("");
    expect(events.map((e) => e.type === "output" && e.paneId)).toEqual([0, 12]);
  });

  it("reads the extended form", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "%extended-output %5 120 : hi\\040there\n");
    expect(events[0].type === "output" && dec.decode(events[0].data)).toBe("hi there");
  });

  it("reports layout changes, with the visible layout and flags", () => {
    const p = new TmuxControlModeParser(true);
    const l = withSum("80x24,0,0,0");
    const { events } = run(p, `%layout-change @1 ${l} ${l} *\n`);
    expect(events).toEqual([{ type: "layout-change", windowId: 1, layout: l, visibleLayout: l, flags: "*" }]);
  });

  it("reports window and session notifications", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(
      p,
      "%window-add @2\n%window-close @2\n%window-renamed @1 my build\n%window-pane-changed @1 %4\n%session-changed $3 deploy\n%session-renamed ops\n%sessions-changed\n%session-window-changed $3 @1\n",
    );
    expect(events).toEqual([
      { type: "window-add", windowId: 2 },
      { type: "window-close", windowId: 2 },
      { type: "window-renamed", windowId: 1, name: "my build" },
      { type: "window-pane-changed", windowId: 1, paneId: 4 },
      { type: "session-changed", sessionId: 3, name: "deploy" },
      { type: "session-renamed", name: "ops" },
      { type: "sessions-changed" },
      { type: "session-window-changed", sessionId: 3, windowId: 1 },
    ]);
  });

  it("treats a window tmux calls unlinked as added or closed", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "%unlinked-window-add @3\n%unlinked-window-close @3\n");
    expect(events).toEqual([
      { type: "window-add", windowId: 3 },
      { type: "window-close", windowId: 3 },
    ]);
  });

  it("collects a reply between %begin and %end, even if its lines start with %", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "%begin 10 7 1\r\n@0 a|b\r\n%output %1 looks like a notification\r\n%end 10 7 1\r\n");
    expect(events).toEqual([{ type: "response", num: 7, ok: true, lines: ["@0 a|b", "%output %1 looks like a notification"], flags: "1" }]);
  });

  it("marks a failed command", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "%begin 10 8 1\nno such window\n%error 10 8 1\n");
    expect(events).toEqual([{ type: "response", num: 8, ok: false, lines: ["no such window"], flags: "1" }]);
  });

  it("ends on %exit, swallows the closing ESC \\, and hands the shell back", () => {
    const p = new TmuxControlModeParser();
    const { events, raw } = run(p, `${DCS}%exit server exited\r\n\x1b\\`, "\r\n$ ");
    expect(events).toEqual([{ type: "exit", reason: "server exited" }]);
    expect(p.active).toBe(false);
    expect(raw).toBe("\r\n$ ");
  });

  it("swallows a closing ESC \\ that arrives in two pieces", () => {
    const p = new TmuxControlModeParser(true);
    const { raw } = run(p, "%exit\n\x1b", "\\$ ");
    expect(raw).toBe("$ ");
  });

  it("can start another control session after the first", () => {
    const p = new TmuxControlModeParser();
    const { events } = run(p, `${DCS}%exit\n\x1b\\$ tmux -CC\r\n${DCS}%window-add @9\n`);
    expect(events.map((e) => e.type)).toEqual(["exit", "window-add"]);
  });

  it("ignores lines it doesn't know and blank lines", () => {
    const p = new TmuxControlModeParser(true);
    const { events } = run(p, "\n%pause %1\n%subscription-changed x $1 @1 1 %1 : v\ngarbage\n%window-add @1\n");
    expect(events).toEqual([{ type: "window-add", windowId: 1 }]);
  });
});
