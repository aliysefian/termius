import { describe, expect, it } from "vitest";
import { shq } from "../ops/quote";
import { LineAssembler, LogBuffer, NO_FILTER, cleanLine, compileText, detectLevel, highlights, logScript, matches, sourceError, UNIT } from "../ops/logs";
import { DISRUPTIVE, LIST_SCRIPT, actionScript, parseUnits, statusScript, toneOf } from "../ops/services";
import { AlertEngine, CLEAR_MARGIN, DEFAULT_RULES, DOWN_AFTER, inQuietHours, type AlertRules } from "../ops/alerts";
import { BUCKET_MS, KEEP_MS, MemoryStore, fold, toSamples, within, type Bucket } from "../ops/archive";
import type { Sample } from "../hosthistory";

describe("quoting", () => {
  it("makes one word whatever is inside", () => {
    expect(shq("a b")).toBe("'a b'");
    expect(shq("it's")).toBe("'it'\\''s'");
    expect(shq("$(rm -rf ~)")).toBe("'$(rm -rf ~)'");
    expect(shq("")).toBe("''");
  });
});

describe("log scripts", () => {
  it("follows the journal, a unit, a priority, the kernel", () => {
    expect(logScript({ kind: "journal" }, 100)).toBe("exec journalctl --follow -n 100 --no-pager -o short-iso");
    expect(logScript({ kind: "journal", unit: "nginx.service", priority: "err" }, 50)).toBe("exec journalctl --follow -n 50 --no-pager -o short-iso -u 'nginx.service' -p err");
    expect(logScript({ kind: "journal", kernel: true }, 0)).toContain("-k");
  });

  it("follows a file by name, the path as one quoted word", () => {
    expect(logScript({ kind: "file", path: "/var/log/nginx/error.log" }, 200)).toBe("exec tail -n 200 -F -- '/var/log/nginx/error.log'");
    expect(logScript({ kind: "file", path: "/tmp/it's $(x).log" }, 1)).toBe("exec tail -n 1 -F -- '/tmp/it'\\''s $(x).log'");
  });

  it("caps the lines asked for and refuses what it can't make safe", () => {
    expect(logScript({ kind: "file", path: "/x" }, 1_000_000)).toContain("-n 1000 ");
    expect(logScript({ kind: "file", path: "/x" }, -5)).toContain("-n 0 ");
    expect(sourceError({ kind: "journal", unit: "nginx" })).not.toBeNull();
    expect(sourceError({ kind: "journal", unit: "x; id.service" })).not.toBeNull();
    expect(sourceError({ kind: "journal", unit: "ssh.service" })).toBeNull();
    expect(sourceError({ kind: "file", path: "relative.log" })).not.toBeNull();
    expect(sourceError({ kind: "file", path: "" })).not.toBeNull();
    expect(() => logScript({ kind: "journal", unit: "$(id).service" }, 10)).toThrow();
    expect(UNIT.test("systemd-resolved.service")).toBe(true);
    expect(UNIT.test("getty@tty1.service")).toBe(true);
  });
});

describe("turning output into lines", () => {
  it("holds back half a line until the rest arrives, and drops colour codes and carriage returns", () => {
    const a = new LineAssembler();
    expect(a.push("one\r\ntw")).toEqual(["one"]);
    expect(a.push("o\n\x1b[31mred\x1b[0m\n")).toEqual(["two", "red"]);
    expect(a.push("tail")).toEqual([]);
    expect(a.finish()).toEqual(["tail"]);
    expect(a.finish()).toEqual([]);
    expect(cleanLine("\x1b]0;title\x07text")).toBe("text");
  });

  it("does not let a line with no end grow without limit", () => {
    const a = new LineAssembler();
    const out = a.push("x".repeat(20_000));
    expect(out.length).toBe(1);
    expect(out[0].length).toBe(20_000);
  });

  it("guesses a level from the words", () => {
    expect(detectLevel("Oct 7 sshd[12]: Failed password for root")).toBe("error");
    expect(detectLevel("nginx: [warn] conflicting server name")).toBe("warn");
    expect(detectLevel("started service debug mode")).toBe("debug");
    expect(detectLevel("Accepted publickey for ops")).toBe("info");
  });

  it("keeps only the newest lines", () => {
    const b = new LogBuffer(3);
    b.add("h1", "web", ["a", "b"]);
    b.add("h2", "db", ["c", "d"]);
    expect(b.lines.map((l) => l.text)).toEqual(["b", "c", "d"]);
    expect(b.lines.map((l) => l.id)).toEqual([2, 3, 4]);
  });
});

describe("filtering", () => {
  const b = new LogBuffer();
  b.add("h1", "web", ["GET /index 200", "ERROR db timeout", "warn: slow"]);
  b.add("h2", "db", ["checkpoint complete", "ERROR disk full"]);
  const show = (f: Partial<typeof NO_FILTER>) => {
    const filter = { ...NO_FILTER, ...f };
    const re = compileText(filter);
    expect(re instanceof Error).toBe(false);
    return b.lines.filter((l) => matches(filter, l, re as RegExp | null)).map((l) => l.text);
  };

  it("by text, regex, level and host", () => {
    expect(show({})).toHaveLength(5);
    expect(show({ text: "error" })).toEqual(["ERROR db timeout", "ERROR disk full"]);
    expect(show({ text: "e.r+or", regex: true })).toHaveLength(2);
    expect(show({ text: "a.b" })).toEqual([]);
    expect(show({ levels: ["warn"] })).toEqual(["warn: slow"]);
    expect(show({ hosts: ["h2"] })).toEqual(["checkpoint complete", "ERROR disk full"]);
    expect(show({ hosts: ["h2"], text: "disk" })).toEqual(["ERROR disk full"]);
  });

  it("plain text is not a pattern", () => {
    expect(show({ text: "(" })).toEqual([]);
    expect(show({ text: ".*" })).toEqual([]);
  });

  it("a pattern that does not compile is an error, not a crash", () => {
    expect(compileText({ text: "(", regex: true }) instanceof Error).toBe(true);
    expect(compileText({ text: "x".repeat(400), regex: false }) instanceof Error).toBe(true);
    expect(compileText({ text: "", regex: true })).toBeNull();
  });

  it("says where it matched", () => {
    const re = compileText({ text: "err", regex: false }) as RegExp;
    expect(highlights("Error and error", re)).toEqual([[0, 3], [10, 13]]);
    expect(highlights("anything", null)).toEqual([]);
    expect(highlights("abc", compileText({ text: "x*", regex: true }) as RegExp)).toEqual([]);
  });
});

describe("services", () => {
  const OUT = `  accounts-daemon.service   loaded    active   running Accounts Service
● backup.service            loaded    failed   failed  Nightly backup
  nginx.service             loaded    active   running A high performance web server
  cron.service              loaded    inactive dead    Regular background program processing daemon
@@FILES
accounts-daemon.service enabled enabled
backup.service enabled enabled
nginx.service enabled enabled
cron.service disabled enabled
`;

  it("reads units and which are enabled, sorted, with the failed one's bullet", () => {
    const r = parseUnits(OUT);
    expect(r.systemd).toBe(true);
    expect(r.units.map((u) => u.name)).toEqual(["accounts-daemon.service", "backup.service", "cron.service", "nginx.service"]);
    const backup = r.units.find((u) => u.name === "backup.service")!;
    expect(backup).toMatchObject({ active: "failed", sub: "failed", description: "Nightly backup", enabled: "enabled" });
    expect(r.units.find((u) => u.name === "cron.service")!.enabled).toBe("disabled");
    expect(r.units.find((u) => u.name === "nginx.service")!.description).toBe("A high performance web server");
  });

  it("says when the host has no systemd", () => {
    expect(parseUnits("@@NO-SYSTEMD\n")).toEqual({ systemd: false, units: [] });
    expect(LIST_SCRIPT).toContain("command -v systemctl");
    expect(LIST_SCRIPT.includes("\n")).toBe(false);
  });

  it("builds an action with the unit as one quoted word, and refuses anything that is not a unit", () => {
    expect(actionScript("restart", "nginx.service", false)).toBe("systemctl restart -- 'nginx.service' 2>&1");
    expect(actionScript("stop", "nginx.service", true)).toBe("sudo -n systemctl stop -- 'nginx.service' 2>&1");
    for (const bad of ["nginx", "x; reboot.service", "$(id).service", "a b.service", "-rf.service ", ""]) expect(() => actionScript("start", bad, false), bad).toThrow();
    expect(() => actionScript("poweroff" as never, "a.service", false)).toThrow();
    expect(statusScript("ssh.service")).toContain("systemctl status");
    expect(() => statusScript("; id")).toThrow();
  });

  it("knows which actions disturb users and colours states", () => {
    expect(DISRUPTIVE).toEqual(["stop", "restart", "disable"]);
    expect(toneOf({ active: "failed", sub: "failed" })).toBe("bad");
    expect(toneOf({ active: "active", sub: "running" })).toBe("good");
    expect(toneOf({ active: "inactive", sub: "dead" })).toBe("muted");
  });
});

describe("alerts", () => {
  const rules = (over: Partial<AlertRules> = {}): AlertRules => ({ ...DEFAULT_RULES, ...over });
  const engine = (r: AlertRules, muted: string[] = []) => new AlertEngine(() => r, (id) => muted.includes(id));
  const NOON = new Date(2026, 9, 7, 12, 0).getTime();

  it("calls a host down only after repeated failures, and says when it is back", () => {
    const e = engine(rules());
    expect(DOWN_AFTER).toBe(2);
    expect(e.health("h1", "web", false, NOON)).toEqual([]);
    const down = e.health("h1", "web", false, NOON + 1);
    expect(down).toHaveLength(1);
    expect(down[0]).toMatchObject({ kind: "down", host: "web", quiet: false });
    // Still down: said once.
    expect(e.health("h1", "web", false, NOON + 2)).toEqual([]);
    expect(e.health("h1", "web", true, NOON + 3)[0].kind).toBe("up");
    expect(e.health("h1", "web", true, NOON + 4)).toEqual([]);
  });

  it("one dropped check is not an outage", () => {
    const e = engine(rules());
    e.health("h1", "web", false, NOON);
    expect(e.health("h1", "web", true, NOON + 1)).toEqual([]);
    expect(e.health("h1", "web", false, NOON + 2)).toEqual([]);
  });

  it("raises a threshold only after enough readings in a row, then once, then clears with margin", () => {
    const e = engine(rules({ samples: 3, cpu: 90 }));
    const cpu = (v: number) => e.metrics("h1", "web", { cpuPct: v, memPct: 10, diskPct: 10 }, NOON);
    expect(cpu(95)).toEqual([]);
    expect(cpu(96)).toEqual([]);
    const hit = cpu(97);
    expect(hit).toHaveLength(1);
    expect(hit[0]).toMatchObject({ kind: "cpu" });
    expect(hit[0].message).toContain("97%");
    expect(cpu(99)).toEqual([]);
    // Just under the line is not "back to normal".
    expect(CLEAR_MARGIN).toBe(5);
    expect(cpu(88)).toEqual([]);
    expect(cpu(70)[0].kind).toBe("cleared");
    expect(cpu(70)).toEqual([]);
  });

  it("a dip resets the count, so only sustained load counts", () => {
    const e = engine(rules({ samples: 3 }));
    const cpu = (v: number) => e.metrics("h1", "web", { cpuPct: v, memPct: null, diskPct: null }, NOON);
    cpu(95);
    cpu(95);
    cpu(10);
    expect(cpu(95)).toEqual([]);
    expect(cpu(95)).toEqual([]);
    expect(cpu(95)).toHaveLength(1);
  });

  it("a reading that is not available changes nothing, and a null threshold is off", () => {
    const e = engine(rules({ samples: 1, cpu: null, mem: 50 }));
    expect(e.metrics("h1", "web", { cpuPct: 100, memPct: null, diskPct: null }, NOON)).toEqual([]);
    expect(e.metrics("h1", "web", { cpuPct: 100, memPct: 60, diskPct: null }, NOON).map((a) => a.kind)).toEqual(["mem"]);
  });

  it("is quiet about muted hosts and about everything with the rule off", () => {
    const e = engine(rules({ samples: 1 }), ["h1"]);
    e.health("h1", "web", false, NOON);
    expect(e.health("h1", "web", false, NOON)).toEqual([]);
    expect(e.metrics("h1", "web", { cpuPct: 100, memPct: 100, diskPct: 100 }, NOON)).toEqual([]);
    const off = engine(rules({ down: false }));
    off.health("h2", "db", false, NOON);
    expect(off.health("h2", "db", false, NOON)).toEqual([]);
  });

  it("marks what happens in quiet hours, including overnight windows", () => {
    const q = { on: true, from: "22:00", to: "07:00" };
    const at = (h: number, m = 0) => new Date(2026, 9, 7, h, m).getTime();
    expect(inQuietHours(at(23), q)).toBe(true);
    expect(inQuietHours(at(3), q)).toBe(true);
    expect(inQuietHours(at(7), q)).toBe(false);
    expect(inQuietHours(at(12), q)).toBe(false);
    expect(inQuietHours(at(22), q)).toBe(true);
    expect(inQuietHours(at(23), { ...q, on: false })).toBe(false);
    expect(inQuietHours(at(10), { on: true, from: "09:00", to: "17:00" })).toBe(true);
    expect(inQuietHours(at(8), { on: true, from: "09:00", to: "17:00" })).toBe(false);
    expect(inQuietHours(at(10), { on: true, from: "09:00", to: "09:00" })).toBe(false);
    const e = engine(rules({ quiet: q, samples: 1 }));
    expect(e.metrics("h1", "web", { cpuPct: 99, memPct: null, diskPct: null }, at(23))[0].quiet).toBe(true);
  });
});

describe("the kept history", () => {
  const s = (t: number, cpu: number | null, mem: number | null = null): Sample => ({ t, cpu, mem, rx: null, tx: null });
  const T0 = Date.UTC(2026, 9, 7, 12, 0, 0);

  it("folds readings in the same minute into their average, per series", () => {
    let b: Bucket[] = [];
    b = fold(b, s(T0 + 5_000, 10, 50), KEEP_MS.day);
    b = fold(b, s(T0 + 35_000, 30, null), KEEP_MS.day);
    b = fold(b, s(T0 + 50_000, 50, 70), KEEP_MS.day);
    expect(b).toHaveLength(1);
    expect(b[0].t).toBe(T0);
    expect(b[0].cpu).toBe(30);
    expect(b[0].mem).toBe(60);
    expect(b[0].n).toEqual({ cpu: 3, mem: 2, rx: 0, tx: 0 });
    expect(b[0].rx).toBeNull();
  });

  it("starts a bucket per minute and drops what is older than it keeps", () => {
    let b: Bucket[] = [];
    for (let i = 0; i < 3; i++) b = fold(b, s(T0 + i * BUCKET_MS, i), KEEP_MS.day);
    expect(b.map((x) => x.cpu)).toEqual([0, 1, 2]);
    // Exactly a day back is kept; the minute before it is not.
    b = fold(b, s(T0 + KEEP_MS.day + BUCKET_MS, 9), KEEP_MS.day);
    expect(b.map((x) => x.cpu)).toEqual([1, 2, 9]);
    b = fold(b, s(T0 + KEEP_MS.day + 3 * BUCKET_MS, 8), KEEP_MS.day);
    expect(b.map((x) => x.cpu)).toEqual([9, 8]);
  });

  it("ignores a reading from the past (the clock stepped back) instead of rewriting history", () => {
    let b = fold([], s(T0 + BUCKET_MS * 5, 40), KEEP_MS.week);
    const same = fold(b, s(T0, 99), KEEP_MS.week);
    expect(same).toBe(b);
  });

  it("gives samples for the charts and a window of recent buckets", () => {
    let b: Bucket[] = [];
    for (let i = 0; i < 10; i++) b = fold(b, s(T0 + i * BUCKET_MS, i), KEEP_MS.day);
    expect(toSamples(b)[0].t).toBe(T0 + BUCKET_MS / 2);
    expect(within(b, T0 + 9 * BUCKET_MS, 3 * BUCKET_MS).map((x) => x.cpu)).toEqual([6, 7, 8, 9]);
  });

  it("saves, loads and clears through a store", async () => {
    const st = new MemoryStore();
    const b = fold([], s(T0, 1), KEEP_MS.day);
    await st.save("h1", b);
    await st.save("h2", b);
    expect(await st.load("h1")).toEqual(b);
    expect(await st.load("nope")).toEqual([]);
    await st.clear("h1");
    expect(await st.load("h1")).toEqual([]);
    await st.clear();
    expect(await st.load("h2")).toEqual([]);
  });
});
