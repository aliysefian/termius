import { describe, expect, it } from "vitest";
import { HistoryIndex, rejectReason, sensitiveReason } from "../completion/history";
import { fitToColumns, ghostBox, nextWord } from "../completion/ghost";
import { completionBridge } from "../completion/bridge";

const q = (text: string, extra: Record<string, unknown> = {}) => ({ text, host: "a", ...extra });
const cmd = (command: string, at: number, extra: Record<string, unknown> = {}) => ({ command, at, exit: 0, ...extra });

describe("ranking", () => {
  it("prefers the more recent of two equal matches", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("git status", 1));
    h.add("a", cmd("git stash", 2));
    expect(h.suggest(q("git st"))).toBe("ash");
  });

  it("prefers this host over a newer command from another", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("ls -la /srv", 1));
    h.add("b", cmd("ls -lh /tmp", 2));
    expect(h.suggest(q("ls -l"))).toBe("a /srv");
  });

  it("prefers the same folder", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("make build", 1, { cwd: "/work/app" }));
    h.add("a", cmd("make bench", 2, { cwd: "/etc" }));
    expect(h.suggest(q("make b", { cwd: "/work/app" }))).toBe("uild");
    expect(h.suggest(q("make b", { cwd: "/etc" }))).toBe("ench");
  });

  it("prefers a command that succeeded", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("npm run build", 1));
    h.add("a", { command: "npm run bad", at: 2, exit: 1 });
    expect(h.suggest(q("npm run b"))).toBe("uild");
  });

  it("prefers what followed the previous command last time", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("git push", 1, { prev: "git commit" }));
    h.add("a", cmd("git pull", 2, { prev: "ls" }));
    expect(h.suggest(q("git p", { prev: "git commit" }))).toBe("ush");
    expect(h.suggest(q("git p", { prev: "ls" }))).toBe("ull");
  });

  it("offers nothing for an empty line, an exact match, or no match", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("ls", 1));
    expect(h.suggest(q(""))).toBeNull();
    expect(h.suggest(q("  "))).toBeNull();
    expect(h.suggest(q("ls"))).toBeNull();
    expect(h.suggest(q("cd"))).toBeNull();
  });
});

describe("what is never kept or offered", () => {
  const bad = [
    "mysql -u root -pHunter2",
    "curl -H 'Authorization: Bearer abcdefgh12345678' https://x",
    "export API_TOKEN=abc123",
    "psql postgres://me:hunter2@db/app",
    "sshpass -p x ssh host",
    "tool --password hunter2",
    "curl -u admin:secret https://x",
    "echo AKIAABCDEFGHIJKLMNOP",
    "deploy 9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0a",
  ];
  it.each(bad)("rejects %s", (c) => {
    expect(sensitiveReason(c)).not.toBeNull();
    const h = new HistoryIndex();
    expect(h.add("a", cmd(c, 1))).toBe(false);
    expect(h.size).toBe(0);
  });

  it("keeps ordinary commands", () => {
    for (const c of ["git commit -m 'fix the token refresh'", "ls /var/log", "ssh -p 2222 me@host", "grep -r password docs/", "cd ~/projects/my-app"]) {
      expect(sensitiveReason(c), c).toBeNull();
    }
  });

  it("drops leading-space, multi-line, control-character, too-long and could-not-run commands", () => {
    expect(rejectReason({ command: "ls", leadingSpace: true })).toBe("starts with a space");
    expect(rejectReason({ command: "echo a\necho b" })).not.toBeNull();
    expect(rejectReason({ command: "echo a\tb" })).not.toBeNull();
    expect(rejectReason({ command: "x".repeat(501) })).toBe("too long");
    expect(rejectReason({ command: "nosuchcmd", exit: 127 })).toBe("did not start");
    expect(rejectReason({ command: "ls", exit: 0 })).toBeNull();
  });

  it("never suggests a secret-looking command even if it got in some other way", () => {
    const h = new HistoryIndex();
    h.seed("a", [cmd("mysql -u root -pHunter2", 1), cmd("mysql -u root", 0)]);
    expect(h.suggest(q("mysql -u root"))).toBeNull();
    expect(h.suggest(q("mysql"))).toBe(" -u root");
  });
});

describe("size caps and clearing", () => {
  it("keeps the newest per host and overall", () => {
    const h = new HistoryIndex(3, 5);
    for (let i = 0; i < 10; i++) h.add("a", cmd(`echo ${i}`, i));
    expect(h.size).toBe(3);
    expect(h.suggest(q("echo "))).toBe("9");
    expect(h.suggest(q("echo 0"))).toBeNull();
    for (let i = 0; i < 3; i++) h.add("b", cmd(`ls ${i}`, 100 + i));
    expect(h.size).toBe(5);
  });

  it("clears one host or all", () => {
    const h = new HistoryIndex();
    h.add("a", cmd("ls -l", 1));
    h.add("b", cmd("ls -a", 2));
    h.clear("a");
    expect(h.size).toBe(1);
    h.clear();
    expect(h.size).toBe(0);
  });
});

describe("speed", () => {
  it("a key press to a suggestion over 10,000 entries stays under 16 ms", () => {
    const h = new HistoryIndex(10_000, 10_000);
    for (let i = 0; i < 10_000; i++) h.add("a", cmd(`${["git", "docker", "ls", "kubectl", "ssh"][i % 5]} command-${i} --flag ${i * 7}`, i, { cwd: `/w/${i % 40}` }));
    const typed = ["g", "gi", "git c", "docker command-9", "kubectl command-123", "s", "ssh command-4"];
    // Warm up, then take the worst of many.
    for (const t of typed) h.suggest(q(t));
    let worst = 0;
    for (let round = 0; round < 20; round++) {
      for (const t of typed) {
        const t0 = performance.now();
        h.suggest(q(t, { cwd: "/w/3", prev: "ls" }));
        worst = Math.max(worst, performance.now() - t0);
      }
    }
    console.info(`10,000 entries: worst key-to-suggestion ${worst.toFixed(2)} ms`);
    expect(worst).toBeLessThan(16);
  });
});

describe("storage", () => {
  it("the index never touches storage", () => {
    const trap = () => {
      throw new Error("storage was used");
    };
    const real = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", { configurable: true, get: trap });
    try {
      const h = new HistoryIndex();
      h.add("a", cmd("ls -l", 1));
      h.seed("a", [cmd("ls -a", 0)]);
      expect(h.suggest(q("ls "))).toBe("-l");
      h.clear();
    } finally {
      if (real) Object.defineProperty(globalThis, "localStorage", real);
      else delete (globalThis as { localStorage?: unknown }).localStorage;
    }
  });
});

describe("ghost layout", () => {
  const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height });
  const base = { screen: rect(18, 12, 400, 200), root: rect(10, 10, 500, 300), cols: 40, rows: 10, cursorX: 5, cursorY: 2, suggestion: "echo" };

  it("sits at the cursor cell in the overlay's own coordinates", () => {
    expect(ghostBox(base)).toMatchObject({ left: 8 + 5 * 10, top: 2 + 2 * 20, cellWidth: 10, cellHeight: 20, text: "echo", cellsWide: 4 });
  });

  it("follows the font size: the cell grid scales it", () => {
    const g = ghostBox({ ...base, screen: rect(18, 12, 800, 400) });
    expect(g).toMatchObject({ left: 8 + 5 * 20, cellWidth: 20, cellHeight: 40 });
  });

  it("never runs past the row, so it cannot cover the next line", () => {
    const g = ghostBox({ ...base, cursorX: 36, suggestion: "abcdefghij" });
    expect(g?.text).toBe("abcd");
    expect(ghostBox({ ...base, cursorX: 39 })?.text).toBe("e");
  });

  it("counts wide characters as two cells", () => {
    expect(fitToColumns("日本語", 5)).toBe("日本");
    expect(ghostBox({ ...base, suggestion: "日本" })?.cellsWide).toBe(4);
  });

  it("draws nothing when there is no room or the numbers are nonsense", () => {
    expect(ghostBox({ ...base, cursorX: 40 })).toBeNull();
    expect(ghostBox({ ...base, cols: 0 })).toBeNull();
    expect(ghostBox({ ...base, screen: rect(0, 0, 0, 0) })).toBeNull();
    expect(ghostBox({ ...base, cursorY: 10 })).toBeNull();
  });

  it("takes one word at a time", () => {
    expect(nextWord(" -la /tmp")).toBe(" -la");
    expect(nextWord("/tmp")).toBe("/tmp");
    expect(nextWord("")).toBe("");
  });
});

describe("bridge", () => {
  it("answers false with no pane, and only the current pane may clear itself", () => {
    expect(completionBridge.accept()).toBe(false);
    const a = { accept: () => true, acceptWord: () => true, dismiss: () => true, openMenu: () => true, menuKey: () => true };
    const b = { accept: () => false, acceptWord: () => false, dismiss: () => false, openMenu: () => false, menuKey: () => false };
    completionBridge.set(a);
    completionBridge.set(b);
    completionBridge.clear(a);
    expect(completionBridge.accept()).toBe(false);
    completionBridge.clear(b);
    expect(completionBridge.dismiss()).toBe(false);
    completionBridge.set(a);
    expect(completionBridge.acceptWord()).toBe(true);
    completionBridge.clear(a);
  });
});

describe("what Remember commands saves", () => {
  it("never a secret-looking or leading-space command, and the index is cleared with the history", async () => {
    const { settings } = await import("../stores/settings.svelte");
    const { completionHistory } = await import("../completion/history");
    settings.prefs.rememberCommands = true;
    settings.history = {};
    settings.recordCommand("h1", "ls -la", 0, { cwd: "/srv" });
    settings.recordCommand("h1", "export API_TOKEN=abc123", 0);
    settings.recordCommand("h1", "echo hidden", 0, { leadingSpace: true });
    expect(settings.history["h1"].map((e) => e.command)).toEqual(["ls -la"]);
    expect(settings.history["h1"][0].cwd).toBe("/srv");
    completionHistory.add("h1", cmd("ls -la", 1));
    settings.clearHistory("h1");
    expect(completionHistory.suggest(q("ls", { host: "h1" }))).toBeNull();
    settings.prefs.rememberCommands = false;
  });
});

describe("learning from a host's own history files", () => {
  it("turns lines into entries a second apart, newest last, skipping what should not be kept", async () => {
    const { entriesFromHistoryLines } = await import("../completion/history");
    const e = entriesFromHistoryLines(["ls", "", " secret-looking", "ls", "git status", "x".repeat(600), "make test\r"], 10_000);
    expect(e.map((x) => x.command)).toEqual(["ls", "git status", "make test"]);
    expect(e.map((x) => x.at)).toEqual([8000, 9000, 10_000]);
    expect(e.every((x) => x.exit === null)).toBe(true);
  });
  it("seeding goes through the same refusal as typed commands", async () => {
    const { HistoryIndex, entriesFromHistoryLines } = await import("../completion/history");
    const h = new HistoryIndex();
    h.seed("web", entriesFromHistoryLines(["mysql -u root -pHunter2", "export API_TOKEN=abcdef123456", "docker ps"], 5000));
    expect(h.search("", "web", 10).map((x) => x.command)).toEqual(["docker ps"]);
  });
});
