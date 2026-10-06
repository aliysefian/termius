import { describe, expect, it } from "vitest";
import { fuzzyScore } from "../fuzzy";
import { HistoryIndex } from "../completion/history";
import { groups, historyItems, specItems, matchPositions, menuHeight, placeMenu, snippetItems, flatten, MENU_WIDTH } from "../completion/menu";

const sn = (id: string, label: string, command: string, description = "") => ({ id, label, command, description });

describe("matchPositions", () => {
  it("marks one run when the text occurs as it was typed", () => {
    expect(matchPositions("sta", "git status")).toEqual([4, 5, 6]);
    expect(matchPositions("STA", "git status")).toEqual([4, 5, 6]);
  });

  it("marks each character in order otherwise", () => {
    expect(matchPositions("gts", "git status")).toEqual([0, 2, 4]);
  });

  it("is empty for no query or no match, and agrees with the score about what matches", () => {
    expect(matchPositions("", "ls")).toEqual([]);
    expect(matchPositions("zz", "ls")).toEqual([]);
    for (const [q, t] of [["gts", "git status"], ["dkr", "docker run"], ["xyz", "git status"], ["ls -l", "ls -la /srv"]]) {
      expect(matchPositions(q, t).length > 0).toBe(fuzzyScore(q, t) !== null);
    }
  });
});

describe("history in the list", () => {
  const index = new HistoryIndex();
  index.add("a", { command: "git status", at: 1, exit: 0, cwd: "/work/app" });
  index.add("a", { command: "git stash", at: 2, exit: 1 });
  index.add("b", { command: "grep -rn status src", at: 3, exit: 0 });

  it("finds fuzzy matches, this host first and what succeeded before what failed, with the match marked", () => {
    const found = historyItems("gst", index.search("gst", "a", 9));
    expect(found.map((i) => i.label)).toEqual(["git status", "git stash", "grep -rn status src"]);
    expect(found[0].labelHit.length).toBe(3);
    expect(found[0].detail).toBe("app");
    expect(found[1].detail).toBe("exit 1");
  });

  it("lists recent commands for an empty line, and leaves out exactly what is typed", () => {
    expect(historyItems("", index.search("", "a", 9)).length).toBe(3);
    expect(historyItems("git stash", index.search("git stash", "a", 9)).map((i) => i.label)).toEqual([]);
  });

  it("is capped", () => {
    const big = new HistoryIndex();
    for (let i = 0; i < 100; i++) big.add("a", { command: `echo ${i}`, at: i, exit: 0 });
    expect(historyItems("echo", big.search("echo", "a", 100)).length).toBe(8);
  });
});

describe("snippets in the list", () => {
  const all = [
    sn("1", "Disk usage", "df -h", "Free space"),
    sn("2", "Restart nginx", "sudo systemctl restart nginx"),
    sn("3", "Tail log", "tail -f /var/log/{{file}}.log"),
    sn("4", "Two lines", "cd /srv\nls"),
    sn("5", "Empty", "  "),
  ];

  it("matches by label and by command", () => {
    expect(snippetItems("disk", all).map((i) => i.id)).toEqual(["s:1"]);
    expect(snippetItems("systemctl", all).map((i) => i.id)).toEqual(["s:2"]);
    expect(snippetItems("df", all).map((i) => i.id)).toEqual(["s:1"]);
  });

  it("highlights where it matched: the label, or the command line shown under it", () => {
    const byLabel = snippetItems("disk", all)[0];
    expect(byLabel.labelHit.length).toBe(4);
    const byBody = snippetItems("systemctl", all)[0];
    expect(byBody.labelHit).toEqual([]);
    expect(byBody.detailHit.length).toBe(9);
  });

  it("flags a snippet that asks for values and says for which", () => {
    const [tail] = snippetItems("tail", all);
    expect(tail.variables).toBe(true);
    expect(tail.detail).toContain("asks for file");
    expect(tail.insert).toBe("tail -f /var/log/{{file}}.log");
  });

  it("leaves out snippets of several lines (typing one would run it) and empty ones", () => {
    expect(snippetItems("", all).map((i) => i.id).sort()).toEqual(["s:1", "s:2", "s:3"]);
  });

  it("drops a snippet whose body is already listed from history", () => {
    const h = new HistoryIndex();
    h.add("a", { command: "df -h", at: 1, exit: 0 });
    const gs = groups([...historyItems("df", h.search("df", "a", 9)), ...snippetItems("df", all)]);
    expect(gs.map((g) => g.title)).toEqual(["History"]);
    expect(flatten(gs).length).toBe(1);
  });
});

describe("placing the list", () => {
  const bounds = { left: 0, top: 0, width: 800, height: 400 };
  const cell = (left: number, top: number) => ({ left, top, width: 8, height: 18 });
  const size = { width: MENU_WIDTH, height: 200 };

  it("goes under the line when it fits", () => {
    expect(placeMenu(cell(100, 50), size, bounds)).toMatchObject({ top: 68, left: 100, above: false, height: 200 });
  });

  it("flips above the line near the bottom", () => {
    const p = placeMenu(cell(100, 350), size, bounds);
    expect(p.above).toBe(true);
    expect(p.top + p.height).toBe(350);
  });

  it("stays inside the bounds sideways", () => {
    expect(placeMenu(cell(700, 50), size, bounds).left).toBe(800 - MENU_WIDTH);
    expect(placeMenu(cell(-30, 50), size, bounds).left).toBe(0);
    expect(placeMenu(cell(10, 50), size, { ...bounds, width: 300 }).width).toBe(300);
  });

  it("gets shorter rather than leaving the bounds", () => {
    const p = placeMenu(cell(100, 150), { width: 300, height: 600 }, bounds);
    expect(p.top).toBeGreaterThanOrEqual(0);
    expect(p.top + p.height).toBeLessThanOrEqual(400);
    expect(p.height).toBeLessThan(600);
  });

  it("estimates its own height from rows and headers", () => {
    const h = historyItems("e", [{ command: "echo", host: "a", at: 1, exit: 0 }]);
    expect(menuHeight(groups(h))).toBe(8 + 22 + 24);
  });
});

describe("command suggestions in the list", () => {
  const found = [
    { kind: "subcommand" as const, name: "checkout", description: "Switch branches", insert: "checkout ", replaces: 2 },
    { kind: "option" as const, name: "--amend", description: "Amend the last commit", insert: "--amend ", replaces: 4 },
    { kind: "value" as const, name: "plain", description: "", insert: "plain ", replaces: 0 },
  ];

  it("lists each kind under its own heading and replaces only the typed word", () => {
    const items = specItems("ch", found);
    expect(items.map((i) => i.group)).toEqual(["Commands", "Options", "Values"]);
    expect(items[0]).toMatchObject({ label: "checkout", detail: "Switch branches", insert: "checkout ", erase: 2, labelHit: [0, 1] });
    expect(items[1].labelHit).toEqual([]);
  });

  it("puts them first when given first, and keeps the other headings after", () => {
    const h = new HistoryIndex();
    h.add("a", { command: "git checkout main", at: 1, exit: 0 });
    const gs = groups([...specItems("ch", found.slice(0, 1)), ...historyItems("git ch", h.search("git ch", "a", 9))]);
    expect(gs.map((g) => g.title)).toEqual(["Commands", "History"]);
  });
});
