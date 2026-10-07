import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import { InputWatch, OFF, completionLine, policyFor, type Policy } from "../completion";
import { readLine } from "../completion/line";
import { hostMode } from "../completion/policy";
import { LineTracker } from "../guard";
import { parseOsc133 } from "../shellintegration";

const ON = { smartCompletion: true, acInline: true, acMenu: true, acSnippets: true, acOptions: true, acRemotePaths: true, acSeedHistory: false };
const EVERYTHING: Policy = { enabled: true, history: true, inline: true, menu: true, snippets: true, options: true, remotePaths: true, seedHistory: false };

describe("policy", () => {
  it("follows the settings when the host has no say", () => {
    expect(policyFor(ON)).toEqual(EVERYTHING);
    expect(policyFor(ON, {})).toEqual(EVERYTHING);
    expect(policyFor(ON, null, "staging")).toEqual(EVERYTHING);
    expect(policyFor(ON).seedHistory).toBe(false);
    expect(policyFor({ ...ON, acSeedHistory: true }).seedHistory).toBe(true);
    expect(policyFor({ ...ON, acSeedHistory: true, acRemotePaths: false }).seedHistory).toBe(false);
    expect(policyFor({ ...ON, acSeedHistory: true }, { completion: "history" }).seedHistory).toBe(false);
    expect(policyFor({ ...ON, acSeedHistory: true }, null, "production").seedHistory).toBe(false);
    expect(policyFor({ ...ON, acOptions: false, acRemotePaths: false })).toMatchObject({ options: false, remotePaths: false, snippets: true });
  });

  it("the master switch turns everything off, whatever a host says", () => {
    const off = { ...ON, smartCompletion: false };
    for (const completion of [undefined, "on", "history", "off", "garbage"]) {
      expect(policyFor(off, { completion }, "production")).toEqual(OFF);
    }
  });

  it("a host can switch it off, or keep it to history", () => {
    expect(policyFor(ON, { completion: "off" })).toEqual(OFF);
    expect(policyFor(ON, { completion: "history" })).toEqual({ enabled: true, history: true, inline: true, menu: true, snippets: false, options: false, remotePaths: false, seedHistory: false });
  });

  it("production hosts get history only unless the host says otherwise", () => {
    expect(policyFor(ON, {}, "production")).toMatchObject({ enabled: true, snippets: false, options: false, remotePaths: false });
    expect(policyFor(ON, { completion: "on" }, "production")).toEqual(EVERYTHING);
    expect(policyFor(ON, { completion: "off" }, "production")).toEqual(OFF);
  });

  it("with neither the inline suggestion nor the popup there is nothing to run", () => {
    expect(policyFor({ ...ON, acInline: false, acMenu: false })).toEqual(OFF);
    expect(policyFor({ ...ON, acInline: false })).toMatchObject({ enabled: true, inline: false, menu: true });
    expect(policyFor({ ...ON, acMenu: false })).toMatchObject({ enabled: true, inline: true, menu: false });
  });

  it("ignores a host value it doesn't know", () => {
    expect(hostMode("on")).toBe("on");
    expect(hostMode("history")).toBe("history");
    expect(hostMode("off")).toBe("off");
    for (const bad of ["", "yes", "ON", 1, null, undefined, {}]) expect(hostMode(bad)).toBeUndefined();
    expect(policyFor(ON, { completion: "yes" })).toEqual(EVERYTHING);
  });

  it("ships switched off, as the owner decided; the parts are on once it is", async () => {
    const { DEFAULT_PREFS } = await import("../stores/settings.svelte");
    expect(DEFAULT_PREFS.smartCompletion).toBe(false);
    expect(policyFor(DEFAULT_PREFS)).toEqual(OFF);
    expect(policyFor({ ...DEFAULT_PREFS, smartCompletion: true })).toEqual(EVERYTHING);
  });
});

/** A terminal that fails the test the moment anything looks at it. */
function untouchable(): { term: never; touched: () => number } {
  let n = 0;
  const trap = new Proxy(
    {},
    {
      get(_t, p) {
        n++;
        throw new Error(`the terminal was read (${String(p)}) with smart completion off`);
      },
    },
  );
  return { term: trap as never, touched: () => n };
}

describe("with smart completion off", () => {
  it("no completion code reaches the terminal", () => {
    const watch = new InputWatch();
    for (const policy of [OFF, policyFor({ ...ON, smartCompletion: false }, { completion: "on" }), policyFor(ON, { completion: "off" }), policyFor({ ...ON, acInline: false, acMenu: false })]) {
      const { term, touched } = untouchable();
      expect(completionLine(policy, term, watch, "ls")).toBeNull();
      expect(touched()).toBe(0);
    }
  });

  it("and the same call does read it when it is on", () => {
    const { term, touched } = untouchable();
    expect(() => completionLine(EVERYTHING, term, new InputWatch(), "ls")).toThrow(/was read/);
    expect(touched()).toBeGreaterThan(0);
  });
});

describe("InputWatch", () => {
  const pos = { line: 3, col: 2 };
  it("follows the shell's marks", () => {
    const w = new InputWatch();
    expect(w.integrated).toBe(false);
    expect(w.inputStart).toBeNull();
    w.feed({ kind: "prompt" }, { line: 3, col: 0 }, 80);
    expect(w.integrated).toBe(true);
    expect(w.state).toBe("prompt");
    expect(w.inputStart).toBeNull();
    w.feed({ kind: "command" }, pos, 80);
    expect(w.state).toBe("input");
    expect(w.inputStart).toEqual({ pos, cols: 80 });
    w.feed({ kind: "output" }, { line: 4, col: 0 }, 80);
    expect(w.state).toBe("running");
    expect(w.inputStart).toBeNull();
    w.feed({ kind: "end", exit: 0 }, { line: 6, col: 0 }, 80);
    expect(w.state).toBe("prompt");
    expect(w.inputStart).toBeNull();
  });

  it("forgets everything on reset", () => {
    const w = new InputWatch();
    w.feed({ kind: "command" }, pos, 80);
    w.reset();
    expect(w.integrated).toBe(false);
    expect(w.inputStart).toBeNull();
  });
});

// -- real shells ---------------------------------------------------------------------------------
//
// Sessions recorded from real bash, zsh and fish by completion/record.py: the bytes typed and
// the bytes each shell sent back, step by step, with and without shell integration. The
// expectations were written with the steps, from what each step should leave on the line.

interface Frame {
  label: string;
  typed: string;
  output: string;
  expect: { text: string; atEnd?: boolean } | null;
}
interface Session {
  shell: string;
  version: string;
  mode: "integrated" | "plain";
  cols: number;
  rows: number;
  frames: Frame[];
}

const files = import.meta.glob("./completion/*.json", { eager: true, import: "default" }) as Record<string, Session>;
const sessions: Session[] = Object.keys(files)
  .sort()
  .map((k) => files[k]);

const b64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
const utf8 = (s: string) => new TextDecoder().decode(b64(s));
const write = (term: Terminal, bytes: Uint8Array) => new Promise<void>((r) => term.write(bytes, r));

/** A terminal wired the way TerminalPane wires the real one. */
function pane(cols: number, rows: number) {
  const term = new Terminal({ cols, rows, scrollback: 1000, allowProposedApi: true });
  const watch = new InputWatch();
  const tracker = new LineTracker();
  term.parser.registerOscHandler(133, (data) => {
    const mark = parseOsc133(data);
    if (!mark) return false;
    watch.feed(mark, { line: term.buffer.active.baseY + term.buffer.active.cursorY, col: term.buffer.active.cursorX }, term.cols);
    if (mark.kind === "command") tracker.reset();
    return true;
  });
  return { term, watch, tracker, line: () => completionLine(EVERYTHING, term, watch, tracker.line) };
}

describe("recorded shell sessions", () => {
  it("has a session for bash, zsh and fish, each with and without shell integration", () => {
    expect(sessions.map((s) => `${s.shell}-${s.mode}`).sort()).toEqual(["bash-integrated", "bash-plain", "fish-integrated", "fish-plain", "zsh-integrated", "zsh-plain"]);
    for (const s of sessions) expect(s.frames.length).toBeGreaterThan(25);
  });

  for (const s of sessions) {
    it(`${s.shell} ${s.version}, ${s.mode}: the line is right after every step`, async () => {
      const p = pane(s.cols, s.rows);
      const trail: string[] = [];
      for (const f of s.frames) {
        // The window sends keys on, and sees them first.
        p.tracker.feed(utf8(f.typed));
        await write(p.term, b64(f.output));
        const got = p.line();
        trail.push(`${f.label}: ${got ? JSON.stringify(got.text) : "none"}`);
        if (f.expect === null) {
          expect(got, `${f.label} must give no suggestions\n${trail.join("\n")}`).toBeNull();
          continue;
        }
        expect(got, `${f.label} must read the line\n${trail.join("\n")}`).not.toBeNull();
        expect(got!.text, f.label).toBe(f.expect.text);
        if (f.expect.atEnd !== undefined) expect(got!.cursorAtEnd, `${f.label}: cursor at end`).toBe(f.expect.atEnd);
        // Marks are only trusted when the shell sent them.
        expect(got!.reliable, `${f.label}: reliable`).toBe(s.mode === "integrated");
      }
      p.term.dispose();
    });
  }

  it("covers the situations the design lists", () => {
    const labels = new Set(sessions.flatMap((s) => s.frames.map((f) => f.label)));
    for (const want of ["wrap-2", "multibyte", "left", "own-suggestion", "running", "secret-start", "secret-typed", "mouse-typed", "alt-typed"]) {
      expect(labels.has(want), want).toBe(true);
    }
  });

  it("recalled history (Up) is read from the screen with integration and unknown without it, in bash and zsh", () => {
    for (const s of sessions.filter((x) => x.shell !== "fish")) {
      const up = s.frames.find((f) => f.label === "up")!;
      expect(up.expect, `${s.shell} ${s.mode}`).toEqual(s.mode === "integrated" ? { text: "echo one", atEnd: true } : null);
    }
  });

  it("reads a line that wraps over three rows, and a line of wide characters", async () => {
    // These come from the recordings above at 40 columns; check the geometry is what the test means.
    const s = sessions.find((x) => x.shell === "bash" && x.mode === "integrated")!;
    const wrapped = s.frames.find((f) => f.label === "wrap-2")!;
    expect(wrapped.expect!.text.length + 2).toBeGreaterThan(s.cols * 2);
    const wide = s.frames.find((f) => f.label === "multibyte")!;
    expect(wide.expect!.text).toContain("東京");
  });
});

// -- situations a recording can't easily reach (written by hand, no shell involved) ----------------

describe("by hand", () => {
  const MARK = (c: string) => `\x1b]133;${c}\x07`;
  const prompt = `${MARK("A")}$ ${MARK("B")}`;

  async function run(text: string, opts: { cols?: number; typed?: string | null } = {}) {
    const p = pane(opts.cols ?? 40, 10);
    await write(p.term, new TextEncoder().encode(text));
    return { ...p, got: completionLine(EVERYTHING, p.term, p.watch, opts.typed ?? null) };
  }

  it("reads what's typed after the prompt", async () => {
    const r = await run(`${prompt}git sta`);
    expect(r.got).toEqual({ text: "git sta", cursorAtEnd: true, reliable: true });
  });

  it("says nothing while a command runs, until the next prompt", async () => {
    const running = await run(`${prompt}sleep 5\r\n${MARK("C")}`);
    expect(running.got).toBeNull();
    const back = await run(`${prompt}ls\r\n${MARK("C")}out\r\n${MARK("D;0")}${prompt}`);
    expect(back.got).toEqual({ text: "", cursorAtEnd: true, reliable: true });
  });

  it("gives up on a multi-line command with continuation prompts", async () => {
    const r = await run(`${prompt}echo "one\r\n> two`);
    expect(r.got).toBeNull();
  });

  it("gives up when the cursor is moved back before the start of the input", async () => {
    const r = await run(`${prompt}ab\x1b[5D`);
    expect(r.got).toBeNull();
  });

  it("knows the cursor is not at the end when text follows it, such as a right-hand prompt or the shell's own suggestion", async () => {
    const rprompt = await run(`${prompt}ab\x1b7\x1b[30G12:00\x1b8`);
    expect(rprompt.got).toEqual({ text: "ab", cursorAtEnd: false, reliable: true });
    const grey = await run(`${prompt}ec\x1b7\x1b[2mho one\x1b[0m\x1b8`);
    expect(grey.got).toMatchObject({ text: "ec", cursorAtEnd: false });
  });

  it("gives up after the window is resized, because the saved position may have moved", async () => {
    const r = await run(`${prompt}ls -la`);
    expect(r.got?.text).toBe("ls -la");
    r.term.resize(30, 10);
    expect(completionLine(EVERYTHING, r.term, r.watch, null)).toBeNull();
  });

  it("nothing in the alternate screen, with the mouse tracked, or before any prompt", async () => {
    expect((await run(`${prompt}ab\x1b[?1049h`)).got).toBeNull();
    expect((await run(`${prompt}ab\x1b[?1000h`)).got).toBeNull();
    expect((await run(`${prompt}ab\x1b[?1002h`)).got).toBeNull();
    expect((await run(`${prompt}ab\x1b[?1003h`)).got).toBeNull();
    expect((await run(`${MARK("A")}$ `)).got).toBeNull();
  });

  describe("without shell integration, the keys followed are all there is", () => {
    it("reads the typed line when the screen agrees with it", async () => {
      const r = await run("$ ls -la", { typed: "ls -la" });
      expect(r.got).toEqual({ text: "ls -la", cursorAtEnd: true, reliable: false });
    });

    it("gives up when the keys lost track (history, arrows, completion)", async () => {
      expect((await run("$ ls", { typed: null })).got).toBeNull();
    });

    it("gives up when the screen and the keys disagree", async () => {
      expect((await run("$ lx", { typed: "ls" })).got).toBeNull();
    });

    it("reads a wrapped line from its first row", async () => {
      const long = "x".repeat(55);
      const r = await run(`$ ${long}`, { cols: 20, typed: long });
      expect(r.got).toMatchObject({ text: long, reliable: false });
    });

    it("stays out of password-like prompts, and of what is typed into them", async () => {
      for (const prompt of ["Password: ", "[sudo] password for ali: ", "Enter passphrase for /home/ali/.ssh/id_ed25519: ", "Verification code: ", "Enter PIN: ", "API token: "]) {
        expect((await run(prompt, { typed: "" })).got, prompt).toBeNull();
      }
      // The keys typed into a silent prompt never appear on screen, so the two disagree.
      expect((await run("Password: ", { typed: "hunter2" })).got).toBeNull();
    });

    it("does not mistake an ordinary prompt for a password prompt", async () => {
      expect((await run("ali@web-01:~$ ", { typed: "" })).got).toMatchObject({ text: "" });
      // 'pass' alone is not a password prompt.
      expect((await run("root@pass-through:/# ls", { typed: "ls" })).got).toMatchObject({ text: "ls" });
    });
  });
});
