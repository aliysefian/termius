import { describe, expect, it } from "vitest";
import { MAX_SNIPPETS, PACK_FORMAT, STARTER_PACK, newOnes, parsePack, toPack } from "../snippetpacks";
import { BUILTINS, promptedVariables, render } from "../snippetvars";

describe("the starter pack", () => {
  it("only looks: nothing in it deletes, stops, restarts or changes anything", () => {
    const risky = /\b(rm|rmdir|mv|dd|mkfs|shred|kill|pkill|killall|reboot|shutdown|halt|poweroff|chmod|chown|useradd|userdel|passwd|systemctl\s+(stop|restart|disable|enable|start|reload|mask)|docker\s+(rm|rmi|stop|kill|prune|restart)|git\s+(reset|clean|push|checkout|commit)|truncate)\b|>\s*\/(?!dev\/null)|\|\s*sh\b|\bsudo\b/;
    for (const x of STARTER_PACK) expect(risky.test(x.command), `${x.label}: ${x.command}`).toBe(false);
  });

  it("is well formed: unique names, one line, short, a folder and a description", () => {
    expect(STARTER_PACK.length).toBeGreaterThanOrEqual(20);
    expect(new Set(STARTER_PACK.map((x) => x.label)).size).toBe(STARTER_PACK.length);
    for (const x of STARTER_PACK) {
      expect(x.command.includes("\n"), x.label).toBe(false);
      expect(x.command.length, x.label).toBeLessThan(300);
      expect(x.folder.startsWith("Starter/"), x.label).toBe(true);
      expect(x.description.length, x.label).toBeGreaterThan(5);
    }
  });

  it("asks only for named values, and a snippet with none is sent exactly as written", () => {
    const named = STARTER_PACK.flatMap((x) => promptedVariables(x.command));
    for (const n of named) expect(/^[a-z]+$/.test(n), n).toBe(true);
    expect(named).toEqual(expect.arrayContaining(["folder", "service", "file", "name", "url", "container"]));
    expect((BUILTINS as readonly string[]).includes("folder")).toBe(false);
    const ctx = { host: "h", hostname: "h.example", port: 22, user: "u" };
    for (const x of STARTER_PACK.filter((y) => !promptedVariables(y.command).length)) expect(render(x.command, ctx, {}), x.label).toBe(x.command);
    // Filling the named values in leaves nothing unfilled.
    for (const x of STARTER_PACK) {
      const values = Object.fromEntries(promptedVariables(x.command).map((n) => [n, "VALUE"]));
      expect(render(x.command, ctx, values).includes("{{"), x.label).toBe(false);
    }
  });

  it("makes a pack that reads back whole", () => {
    const back = parsePack(toPack(STARTER_PACK));
    expect(back.error).toBeNull();
    expect(back.skipped).toEqual([]);
    expect(back.snippets.length).toBe(STARTER_PACK.length);
    expect(back.snippets[0]).toMatchObject({ label: STARTER_PACK[0].label, command: STARTER_PACK[0].command, folder: STARTER_PACK[0].folder });
  });
});

const pack = (snippets: unknown, over: Record<string, unknown> = {}) => JSON.stringify({ format: PACK_FORMAT, version: 1, snippets, ...over });

describe("reading a pack from someone else", () => {
  it("refuses what is not a pack, or is from the future, or is too big", () => {
    for (const bad of ["", "nonsense", "[]", "null", "42", JSON.stringify({ format: "other", version: 1, snippets: [] })]) expect(parsePack(bad).error, bad).not.toBeNull();
    expect(parsePack(pack([], { version: 2 })).error).toContain("newer");
    expect(parsePack(pack([], { version: "1" })).error).not.toBeNull();
    expect(parsePack(JSON.stringify({ format: PACK_FORMAT, version: 1 })).error).not.toBeNull();
    expect(parsePack("x".repeat(1024 * 1024 + 1)).error).toContain("too large");
    expect(parsePack(pack(Array.from({ length: MAX_SNIPPETS + 1 }, (_, i) => ({ label: `a${i}`, command: "ls" })))).error).toContain("at most");
  });

  it("keeps good entries and says why it left others out", () => {
    const r = parsePack(pack([
      { label: "Good", command: "ls -l", description: "d", folder: "A/B", tags: ["x", " y "] },
      { label: "", command: "ls" },
      { label: "No command", command: "   " },
      { label: "Bad folder", command: "ls", folder: "../etc" },
      { label: "Bad folder 2", command: "ls", folder: "/abs" },
      { label: "Bad tags", command: "ls", tags: "nope" },
      { label: "Escape", command: "ls\x1b[31m" },
      { label: "Number", command: 42 },
      "just a string",
      null,
    ]));
    expect(r.error).toBeNull();
    expect(r.snippets).toEqual([{ label: "Good", command: "ls -l", description: "d", folder: "A/B", tags: ["x", "y"] }]);
    expect(r.skipped.map((x) => x.why)).toEqual([
      "it has no usable name", "it has no usable command", "its folder is not usable", "its folder is not usable",
      "its tags are not usable", "it has no usable command", "it has no usable command", "it has no usable name", "it has no usable name",
    ]);
  });

  it("allows tabs and newlines in a command, and ignores keys it does not know", () => {
    const r = parsePack(pack([{ label: "Two lines", command: "cd /srv\n\tls", extra: { evil: true }, __proto__: { x: 1 } }]));
    expect(r.snippets).toHaveLength(1);
    expect(r.snippets[0].command).toBe("cd /srv\n\tls");
    expect(Object.keys(r.snippets[0]).sort()).toEqual(["command", "description", "folder", "label", "tags"]);
  });

  it("limits each field", () => {
    const r = parsePack(pack([
      { label: "x".repeat(121), command: "ls" },
      { label: "A", command: "x".repeat(8001) },
      { label: "B", command: "ls", description: "d".repeat(501) },
      { label: "C", command: "ls", tags: Array.from({ length: 21 }, (_, i) => `t${i}`) },
      { label: "D", command: "ls", tags: ["t".repeat(41)] },
    ]));
    expect(r.snippets).toEqual([]);
    expect(r.skipped).toHaveLength(5);
  });

  it("does not run, or evaluate, anything it reads", () => {
    const r = parsePack(pack([{ label: "$(touch /tmp/x)", command: "`id`; rm -rf ~" }]));
    expect(r.snippets).toEqual([{ label: "$(touch /tmp/x)", command: "`id`; rm -rf ~", description: "", folder: "", tags: [] }]);
  });
});

describe("adding to what is there", () => {
  it("leaves out what is already present, and repeats within the pack", () => {
    const have = [{ label: "A", command: "ls" }];
    const r = newOnes(have, [{ label: "A", command: "ls" }, { label: "A", command: "ls -l" }, { label: "B", command: "pwd" }, { label: "B", command: "pwd" }]);
    expect(r.fresh.map((x) => x.command)).toEqual(["ls -l", "pwd"]);
    expect(r.already).toBe(2);
  });
});
