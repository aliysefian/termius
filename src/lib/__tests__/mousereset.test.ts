import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import { MOUSE_OFF, RESET_INPUT_MODES, RESET_SESSION_MODES, isMouseReport, mouseReportAllowed } from "../termprotocol";

const write = (t: Terminal, s: string) => new Promise<void>((r) => t.write(s, r));
const fresh = () => new Terminal({ cols: 80, rows: 24, allowProposedApi: true });

type Core = { coreMouseService: { triggerMouseEvent(e: object): boolean } };
/** A mouse event as the renderer would deliver it. Returns whether xterm.js sent a report. */
function mouse(t: Terminal, button: number, action: number) {
  const core = (t as unknown as { _core: Core })._core;
  return core.coreMouseService.triggerMouseEvent({ col: 3, row: 2, x: 30, y: 20, button, action, ctrl: false, alt: false, shift: false });
}
// xterm.js numbering: button 0 left, 1 middle, 2 right, 3 none, 4 wheel; action 0 up, 1 down, 32 move
const MOVE = 32;

describe("what is a mouse report", () => {
  it("recognises SGR, urxvt and legacy reports, single or several", () => {
    expect(isMouseReport("\x1b[<35;2;16M")).toBe(true);
    expect(isMouseReport("\x1b[<0;45;9m")).toBe(true);
    expect(isMouseReport("\x1b[<35;2;16M\x1b[<35;6;15M")).toBe(true);
    expect(isMouseReport("\x1b[67;2;16M")).toBe(true);
    expect(isMouseReport("\x1b[M C!")).toBe(true);
  });

  it("leaves typing alone, including a report whose escape was lost (it is then ordinary text)", () => {
    for (const s of ["a", "ls\r", "\x1b[A", "\x1b[1;5C", "\x1b[200~paste\x1b[201~", "35;2;16M", "[<35;2;16M", "\x1bOP"]) expect(isMouseReport(s), s).toBe(false);
  });
});

describe("when a report may be sent", () => {
  it("never with no mouse mode on", () => {
    for (const r of ["\x1b[<0;2;3M", "\x1b[<0;2;3m", "\x1b[<35;2;3M", "\x1b[<64;2;3M", "\x1b[67;2;3M"]) expect(mouseReportAllowed(r, "none"), r).toBe(false);
  });

  it("hover (button 35) only in any-motion mode, 1003", () => {
    expect(mouseReportAllowed("\x1b[<35;2;3M", "any")).toBe(true);
    for (const m of ["x10", "vt200", "drag"] as const) expect(mouseReportAllowed("\x1b[<35;2;3M", m), m).toBe(false);
    expect(mouseReportAllowed("\x1b[67;2;3M", "drag")).toBe(false);
  });

  it("motion with a button held needs drag or any-motion; clicks and the wheel need any mode", () => {
    expect(mouseReportAllowed("\x1b[<32;2;3M", "vt200")).toBe(false);
    expect(mouseReportAllowed("\x1b[<32;2;3M", "drag")).toBe(true);
    expect(mouseReportAllowed("\x1b[<0;2;3M", "vt200")).toBe(true);
    expect(mouseReportAllowed("\x1b[<64;2;3M", "x10")).toBe(true);
  });

  it("everything that isn't a mouse report is sent in every mode", () => {
    for (const m of ["none", "x10", "vt200", "drag", "any"] as const) expect(mouseReportAllowed("ls -la\r", m)).toBe(true);
  });
});

describe("the emulator's own state", () => {
  it("follows 1000, 1002, 1003 and 1006 on and off", async () => {
    const t = fresh();
    expect(t.modes.mouseTrackingMode).toBe("none");
    await write(t, "\x1b[?1000h");
    expect(t.modes.mouseTrackingMode).toBe("vt200");
    await write(t, "\x1b[?1002h");
    expect(t.modes.mouseTrackingMode).toBe("drag");
    await write(t, "\x1b[?1003h\x1b[?1006h");
    expect(t.modes.mouseTrackingMode).toBe("any");
    await write(t, "\x1b[?1003l");
    expect(t.modes.mouseTrackingMode).toBe("none");
  });

  it("a full reset (ESC c) turns the mouse, bracketed paste, application keys and the alternate screen off", async () => {
    const t = fresh();
    await write(t, "\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1h\x1b[?1049h");
    expect([t.modes.mouseTrackingMode, t.modes.bracketedPasteMode, t.modes.applicationCursorKeysMode, t.buffer.active.type]).toEqual(["any", true, true, "alternate"]);
    await write(t, "\x1bc");
    expect([t.modes.mouseTrackingMode, t.modes.bracketedPasteMode, t.modes.applicationCursorKeysMode, t.buffer.active.type]).toEqual(["none", false, false, "normal"]);
  });

  it("xterm.js leaves the mouse on after a soft reset (DECSTR); the extra reset turns it off", async () => {
    const t = fresh();
    await write(t, "\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1h");
    await write(t, "\x1b[!p");
    // The embedder's handler (see TerminalPane) writes RESET_INPUT_MODES when it sees CSI ! p.
    expect(t.modes.mouseTrackingMode).toBe("any");
    await write(t, RESET_INPUT_MODES);
    expect([t.modes.mouseTrackingMode, t.modes.bracketedPasteMode, t.modes.applicationCursorKeysMode]).toEqual(["none", false, false]);
  });
});

describe("a session that ends with the mouse on", () => {
  /** What the pane sends: xterm.js's output through the same filter as TerminalPane's onData. */
  function pane() {
    const t = fresh();
    const sent: string[] = [];
    t.onData((d) => {
      if (mouseReportAllowed(d, t.modes.mouseTrackingMode)) sent.push(d);
    });
    return { t, sent };
  }

  it("before the end, 1003 + 1006 send moves and clicks as SGR reports", async () => {
    const { t, sent } = pane();
    await write(t, "\x1b[?1003h\x1b[?1006h");
    expect(mouse(t, 3, MOVE)).toBe(true);
    expect(sent.length).toBe(1);
    expect(sent[0]).toMatch(/^\x1b\[<35;\d+;\d+M$/);
  });

  it("after the end no mouse byte is emitted, for moves, clicks, drags or the wheel", async () => {
    const { t, sent } = pane();
    await write(t, "\x1b[?1003h\x1b[?1006h");
    mouse(t, 3, MOVE);
    const before = sent.length;
    // The connection drops; the pane writes this to the emulator (TerminalPane does it on every end).
    await write(t, RESET_SESSION_MODES);
    expect(t.modes.mouseTrackingMode).toBe("none");
    for (const button of [0, 1, 2, 3, 4]) for (const action of [0, 1, 32]) mouse(t, button, action);
    expect(sent.length).toBe(before);
  });

  it("the session-end reset also clears bracketed paste, application keys and the alternate screen", async () => {
    const t = fresh();
    await write(t, "\x1b[?1003h\x1b[?1006h\x1b[?1015h\x1b[?2004h\x1b[?1h\x1b[?1049h");
    await write(t, RESET_SESSION_MODES);
    expect([t.modes.mouseTrackingMode, t.modes.bracketedPasteMode, t.modes.applicationCursorKeysMode, t.buffer.active.type]).toEqual(["none", false, false, "normal"]);
  });

  it("even if the emulator were still reporting, the filter drops the report whole", async () => {
    const { sent } = pane();
    // No mode on: a report that gets through anyway is dropped (the pane's mode is "none").
    expect(mouseReportAllowed("\x1b[<35;2;16M", "none")).toBe(false);
    expect(sent).toEqual([]);
  });

  it("a shell prompt turns the mouse off (the program that wanted it was killed)", async () => {
    const { t, sent } = pane();
    await write(t, "\x1b[?1003h\x1b[?1006h");
    await write(t, MOUSE_OFF);
    mouse(t, 3, MOVE);
    expect(sent).toEqual([]);
    expect(t.modes.mouseTrackingMode).toBe("none");
  });
});
