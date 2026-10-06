import { describe, expect, it } from "vitest";
import { arrange, escapeName, pathQuestion, RemoteLookup, type Reply, type Request } from "../completion/remote";

function setup(over: { enabled?: () => boolean; reply?: (r: Request) => Reply | Promise<Reply>; fail?: unknown } = {}) {
  const asked: Request[] = [];
  let clock = 0;
  const gates: (() => void)[] = [];
  const lookup = new RemoteLookup({
    enabled: over.enabled ?? (() => true),
    fetch: async (r) => {
      asked.push(r);
      if (over.fail) throw over.fail;
      return over.reply ? over.reply(r) : { entries: [{ name: "a", dir: false }], truncated: false };
    },
    now: () => clock,
    // Time only moves when the test says so; a sleep waits until it is let go.
    sleep: () => new Promise<void>((res) => gates.push(res)),
  });
  const settle = async () => {
    while (gates.length) gates.shift()!();
    await new Promise((r) => setTimeout(r, 0));
  };
  return { lookup, asked, settle, tick: (ms: number) => (clock += ms) };
}

describe("with the switch off", () => {
  it("no question is ever sent", async () => {
    const t = setup({ enabled: () => false });
    const p = [t.lookup.dir("/etc", ""), t.lookup.generator("git-branches", "/srv"), t.lookup.dir("~", "x")];
    await t.settle();
    expect(await Promise.all(p)).toEqual([null, null, null]);
    expect(t.asked).toEqual([]);
    expect(t.lookup.sent).toBe(0);
  });

  it("turning it off while waiting for typing to settle stops the question", async () => {
    let on = true;
    const t = setup({ enabled: () => on });
    const p = t.lookup.dir("/etc", "");
    on = false;
    await t.settle();
    expect(await p).toBeNull();
    expect(t.asked).toEqual([]);
  });
});

describe("a burst of keys", () => {
  it("makes one question, for the last key; the earlier ones get nothing", async () => {
    const t = setup();
    const first = t.lookup.dir("/etc", "a");
    const second = t.lookup.dir("/etc", "ab");
    const third = t.lookup.dir("/etc", "abc");
    await t.settle();
    expect(await first).toBeNull();
    expect(await second).toBeNull();
    expect((await third)?.entries.length).toBe(1);
    expect(t.asked).toEqual([{ kind: "dir", dir: "/etc", prefix: "abc" }]);
  });

  it("drops an answer that arrives after a newer question", async () => {
    let release!: (r: Reply) => void;
    const t = setup({ reply: () => new Promise<Reply>((r) => (release = r)) });
    const slow = t.lookup.dir("/a", "");
    await t.settle();
    const newer = t.lookup.dir("/b", "");
    release({ entries: [{ name: "late", dir: false }], truncated: false });
    expect(await slow).toBeNull();
    await t.settle();
    release({ entries: [{ name: "fresh", dir: false }], truncated: false });
    expect((await newer)?.entries[0].name).toBe("fresh");
  });
});

describe("keeping answers", () => {
  const listing: Reply = { entries: [{ name: "alpha", dir: false }, { name: "beta", dir: true }, { name: "alps", dir: false }], truncated: false };

  it("answers a longer prefix from a complete listing, without asking again", async () => {
    const t = setup({ reply: () => listing });
    const p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    const narrowed = await t.lookup.dir("/w", "alp");
    expect(narrowed?.entries.map((e) => e.name)).toEqual(["alpha", "alps"]);
    expect(t.asked.length).toBe(1);
  });

  it("asks again when the listing was cut, since the prefix may reach what was left out", async () => {
    const t = setup({ reply: () => ({ ...listing, truncated: true }) });
    const p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    const again = t.lookup.dir("/w", "al");
    await t.settle();
    await again;
    expect(t.asked.length).toBe(2);
  });

  it("forgets after the time is up", async () => {
    const t = setup({ reply: () => listing });
    let p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    t.tick(6000);
    p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    expect(t.asked.length).toBe(2);
  });
});

describe("a host that says no", () => {
  it("is not asked again, for anything", async () => {
    const t = setup({ fail: { code: "completion_refused", message: "no" } });
    const p = t.lookup.dir("/w", "");
    await t.settle();
    expect(await p).toBeNull();
    expect(t.lookup.refused).toBe(true);
    for (let i = 0; i < 5; i++) {
      const q = [t.lookup.dir("/x", String(i)), t.lookup.generator("docker-containers")];
      await t.settle();
      await Promise.all(q);
    }
    expect(t.asked.length).toBe(1);
  });

  it("an ordinary failure is not repeated at once, but later is", async () => {
    const t = setup({ fail: { code: "completion_timeout", message: "slow" } });
    let p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    expect(t.asked.length).toBe(1);
    expect(t.lookup.refused).toBe(false);
    t.tick(11_000);
    p = t.lookup.dir("/w", "");
    await t.settle();
    await p;
    expect(t.asked.length).toBe(2);
  });
});

describe("turning a word into a question", () => {
  it("absolute, home and relative paths", () => {
    expect(pathQuestion("/etc/ho", null)).toEqual({ dir: "/etc", prefix: "ho", dirText: "/etc/" });
    expect(pathQuestion("/", null)).toEqual({ dir: "/", prefix: "", dirText: "/" });
    expect(pathQuestion("~/src/ap", null)).toEqual({ dir: "~/src", prefix: "ap", dirText: "~/src/" });
    expect(pathQuestion("~/", null)).toEqual({ dir: "~", prefix: "", dirText: "~/" });
    expect(pathQuestion("~", null)).toEqual({ dir: "~", prefix: "", dirText: "~/" });
    expect(pathQuestion("src/ma", "/work/app")).toEqual({ dir: "/work/app/src", prefix: "ma", dirText: "src/" });
    expect(pathQuestion("ma", "/work/app")).toEqual({ dir: "/work/app", prefix: "ma", dirText: "" });
    expect(pathQuestion("../lib/x", "/work/app")).toEqual({ dir: "/work/lib", prefix: "x", dirText: "../lib/" });
    expect(pathQuestion("", "/work/app")).toEqual({ dir: "/work/app", prefix: "", dirText: "" });
  });

  it("can't answer a relative path without the shell's folder, or another user's home", () => {
    expect(pathQuestion("src/ma", null)).toBeNull();
    expect(pathQuestion("src/ma", "relative/nonsense")).toBeNull();
    expect(pathQuestion("~root/x", "/w")).toBeNull();
  });

  it("does not climb out of the root", () => {
    expect(pathQuestion("../../../../x", "/a")).toEqual({ dir: "/", prefix: "x", dirText: "../../../../" });
  });
});

describe("names as shell words", () => {
  it("escapes what a shell would take for syntax, and nothing else", () => {
    expect(escapeName("my docs")).toBe("my\\ docs");
    expect(escapeName("it's")).toBe("it\\'s");
    expect(escapeName('say "hi"')).toBe('say\\ \\"hi\\"');
    expect(escapeName("$(x)")).toBe("\\$\\(x\\)");
    expect(escapeName("a;b&c|d")).toBe("a\\;b\\&c\\|d");
    expect(escapeName("plain-name_1.txt")).toBe("plain-name_1.txt");
    expect(escapeName("~tilde", true)).toBe("\\~tilde");
    expect(escapeName("é日本")).toBe("é日本");
  });
});

describe("arranging a listing", () => {
  const e = (name: string, dir = false) => ({ name, dir });
  it("folders first, then by name; hidden only when asked", () => {
    const list = [e("b"), e(".git", true), e("a"), e("src", true), e(".env")];
    expect(arrange(list, "", false).map((x) => x.name)).toEqual(["src", "a", "b"]);
    expect(arrange(list, ".", false).map((x) => x.name)).toEqual([".git", "src", ".env", "a", "b"]);
    expect(arrange(list, "", true).map((x) => x.name)).toEqual(["src"]);
  });
});
