import { describe, expect, it } from "vitest";
import { CommandTracker, parseOsc133, parseOsc7 } from "../shellintegration";

describe("OSC parsing", () => {
  it("reads the directory from OSC 7", () => {
    expect(parseOsc7("file://web-01/home/ops/app")).toBe("/home/ops/app");
    expect(parseOsc7("file:///srv/my%20dir")).toBe("/srv/my dir");
    expect(parseOsc7("http://x/y")).toBeNull();
  });
  it("reads OSC 133 marks", () => {
    expect(parseOsc133("A")).toEqual({ kind: "prompt" });
    expect(parseOsc133("D;0")).toEqual({ kind: "end", exit: 0 });
    expect(parseOsc133("D;127")).toEqual({ kind: "end", exit: 127 });
    expect(parseOsc133("D")).toEqual({ kind: "end", exit: null });
    expect(parseOsc133("Z")).toBeNull();
  });
});

describe("CommandTracker", () => {
  it("records what was typed, its output range and exit code", () => {
    const lines: Record<number, string> = { 10: "ops@web-01:~$ ls -la /tmp", 11: "total 0", 12: "drwx x", 13: "ops@web-01:~$ " };
    let t = 1000;
    const tr = new CommandTracker((l) => lines[l] ?? "", () => (t += 500));
    expect(tr.active).toBe(false);
    tr.feed({ kind: "prompt" }, { line: 10, col: 0 });
    tr.feed({ kind: "command" }, { line: 10, col: 14 });
    tr.feed({ kind: "output" }, { line: 11, col: 0 });
    const rec = tr.feed({ kind: "end", exit: 0 }, { line: 13, col: 0 });
    expect(rec).toMatchObject({ command: "ls -la /tmp", exit: 0, outputStart: 11, outputEnd: 12 });
    expect(rec!.endedAt - rec!.startedAt).toBe(500);
    expect(tr.last).toBe(rec);
    expect(tr.active).toBe(true);
  });

  it("ignores empty commands (just pressing Enter) and finds prompts", () => {
    const tr = new CommandTracker(() => "$ ");
    tr.feed({ kind: "prompt" }, { line: 1, col: 0 });
    tr.feed({ kind: "command" }, { line: 1, col: 2 });
    tr.feed({ kind: "output" }, { line: 2, col: 0 });
    expect(tr.feed({ kind: "end", exit: 0 }, { line: 2, col: 0 })).toBeNull();
    tr.feed({ kind: "prompt" }, { line: 2, col: 0 });
    tr.feed({ kind: "prompt" }, { line: 9, col: 0 });
    expect(tr.nearestPrompt(5, -1)).toBe(2);
    expect(tr.nearestPrompt(5, 1)).toBe(9);
    expect(tr.nearestPrompt(9, 1)).toBeNull();
  });
});
