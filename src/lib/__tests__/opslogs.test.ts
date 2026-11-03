import { describe, expect, it } from "vitest";
import { LogBuffer, Rate, SEVS, sevOfPriority, toRow, type WireEntry } from "../ops/logrows";
import { applyFilter, compile, isFiltered, noFilter, regexRisk, similarTo } from "../ops/logfilter";
import { csvCell, exportName, render, rowJson, toCsv, toJsonl, toText } from "../ops/logexport";
import { ROW_H, buildOffsets, rowHeight, visibleRows } from "../ops/loglayout";
import { DEFAULT_LOG_RULES, LogAlertDetector } from "../ops/logalerts";
import { relativeTime, retryText } from "../ops/logstatus";
import { historyOf } from "../stores/opslogs.svelte";
import { addAlert, AlertEngine, DEFAULT_RULES } from "../ops/alerts";

const wire = (host: string, seq: number, o: Partial<WireEntry> = {}): WireEntry => ({ host_id: host, seq, ts_us: 1_700_000_000_000_000 + seq * 1000, priority: 6, unit: "sshd.service", ident: "sshd", pid: 10, message: `m${seq}`, ...o });
const row = (host: string, seq: number, o: Partial<WireEntry> = {}, label = host) => toRow(wire(host, seq, o), label);

describe("rows", () => {
  it("maps journal priorities to four buckets and keeps the host the stream says", () => {
    expect([0, 1, 2, 3, 4, 5, 6, 7].map(sevOfPriority)).toEqual(["error", "error", "error", "error", "warn", "info", "info", "debug"]);
    const r = row("h1", 3, { priority: 3, journal_host: "liar" }, "claude");
    expect(r).toMatchObject({ id: "h1:3", hostId: "h1", host: "claude", sev: "error", guessed: false, journalHost: "liar", ts: 1_700_000_000_003 });
  });

  it("guesses a severity from the words for a log file, and says it guessed", () => {
    const r = toRow({ host_id: "h", seq: 1, ts_us: 1, message: "ERROR: disk failed" }, "x");
    expect(r).toMatchObject({ priority: null, sev: "error", guessed: true });
    expect(toRow({ host_id: "h", seq: 2, ts_us: 1, message: "all fine" }, "x").sev).toBe("info");
  });
});

describe("the buffer", () => {
  it("keeps rows in time order across hosts even when they arrive out of order", () => {
    const b = new LogBuffer(100);
    b.add([row("a", 1, { ts_us: 3_000_000 }), row("a", 2, { ts_us: 5_000_000 })]);
    b.add([row("b", 1, { ts_us: 4_000_000 }), row("b", 2, { ts_us: 1_000_000 })]);
    expect(b.rows.map((r) => r.us)).toEqual([1_000_000, 3_000_000, 4_000_000, 5_000_000]);
  });

  it("keeps arrival order when asked", () => {
    const b = new LogBuffer(100, "arrival");
    b.add([row("a", 1, { ts_us: 9 })]);
    b.add([row("b", 1, { ts_us: 1 })]);
    expect(b.rows.map((r) => r.hostId)).toEqual(["a", "b"]);
  });

  it("never holds more than its size and counts what it let go", () => {
    const b = new LogBuffer(100);
    for (let i = 1; i <= 15; i++) b.add(Array.from({ length: 10 }, (_, k) => row("a", i * 10 + k)));
    expect(b.rows).toHaveLength(100);
    expect(b.evicted).toBe(50);
    expect(b.rows[0].seq).toBe(60);
    b.resize(100);
    expect(b.rows).toHaveLength(100);
  });

  it("does not take the same event twice, and does not mutate an array a view may hold", () => {
    const b = new LogBuffer(100);
    b.add([row("a", 1), row("a", 2)]);
    const held = b.rows;
    expect(b.add([row("a", 2), row("a", 3)])).toBe(1);
    expect(held).toHaveLength(2);
    expect(b.rows).toHaveLength(3);
    // The same sequence number from another host is a different event.
    expect(b.add([row("b", 1)])).toBe(1);
    b.clear();
    expect(b.rows).toEqual([]);
    expect(b.add([row("a", 1)])).toBe(1);
  });

  it("stays fast with a large stream", () => {
    const b = new LogBuffer(20_000);
    const t = performance.now();
    for (let i = 0; i < 200; i++) b.add(Array.from({ length: 500 }, (_, k) => row("a", i * 500 + k + 1)));
    expect(b.rows).toHaveLength(20_000);
    expect(performance.now() - t).toBeLessThan(3000);
  });
});

describe("the event rate", () => {
  it("is read from what really arrived, and withheld until there is time behind it", () => {
    const r = new Rate();
    const t0 = 1_000_000_000;
    expect(r.perMinute(t0)).toBeNull();
    r.add(10, t0);
    expect(r.perMinute(t0 + 4000)).toBeNull();
    r.add(50, t0 + 20_000);
    // 60 events over 30 seconds of observation: 120 a minute.
    expect(r.perMinute(t0 + 30_000)).toBe(120);
    // Old seconds fall out of the window.
    expect(r.window(t0 + 200_000).count).toBe(0);
  });
});

describe("filtering", () => {
  const rows = [
    row("a", 1, { priority: 6, message: "Accepted publickey for ops", unit: "sshd.service", ident: "sshd" }, "claude"),
    row("a", 2, { priority: 3, message: "nginx failed", unit: "nginx.service", ident: "nginx" }, "claude"),
    row("b", 1, { priority: 4, message: "Disk usage high", unit: "app.service", ident: "app" }, "Elastic"),
    row("b", 2, { priority: 7, message: "trace x=1", unit: "app.service", ident: "app", fields: [["_CMDLINE", "/usr/bin/app --secretflag"]] }, "Elastic"),
  ];
  const run = (f: Partial<ReturnType<typeof noFilter>>) => applyFilter(rows, compile({ ...noFilter(), ...f })).rows.map((r) => r.id);

  it("searches message, host, unit, program and the journal's other fields, case-insensitively", () => {
    expect(run({ text: "PUBLICKEY" })).toEqual(["a:1"]);
    expect(run({ text: "elastic" })).toEqual(["b:1", "b:2"]);
    expect(run({ text: "nginx.service" })).toEqual(["a:2"]);
    expect(run({ text: "secretflag" })).toEqual(["b:2"]);
    // The process id is searched too.
    expect(run({ text: "10" })).toHaveLength(4);
    expect(applyFilter([row("a", 9, { pid: 4242 })], compile({ ...noFilter(), text: "4242" })).rows).toHaveLength(1);
  });

  it("distinguishes exactly one severity from that one and worse", () => {
    expect(run({ sev: "warn", atLeast: true })).toEqual(["a:2", "b:1"]);
    expect(run({ sev: "warn", atLeast: false })).toEqual(["b:1"]);
    expect(run({ sev: "error", atLeast: true })).toEqual(["a:2"]);
    expect(run({ sev: "debug", atLeast: true })).toHaveLength(4);
    expect(run({ sev: "debug", atLeast: false })).toEqual(["b:2"]);
    expect(SEVS).toEqual(["error", "warn", "info", "debug"]);
  });

  it("filters by host and unit (a unit matches by the program's name too)", () => {
    expect(run({ hosts: ["b"] })).toEqual(["b:1", "b:2"]);
    expect(run({ units: ["sshd"] })).toEqual(["a:1"]);
    expect(run({ units: ["app.service", "nginx.service"] })).toEqual(["a:2", "b:1", "b:2"]);
    expect(run({ hosts: ["a"], units: ["app.service"] })).toEqual([]);
  });

  it("filters by time on what has been received", () => {
    const now = 1_700_000_000_500 + 10 * 60_000;
    const f = { ...noFilter(), range: "5m" as const };
    expect(applyFilter(rows, compile(f, now)).rows).toEqual([]);
    expect(applyFilter(rows, compile({ ...f, range: "15m" }, now)).rows).toHaveLength(4);
    expect(applyFilter(rows, compile({ ...noFilter(), range: "custom", from: 1_700_000_000_002, to: 1_700_000_000_003 })).rows.map((r) => r.id)).toEqual(["a:2", "b:2"]);
    expect(isFiltered(noFilter())).toBe(false);
    expect(isFiltered({ ...noFilter(), range: "5m" })).toBe(true);
  });

  it("takes a regular expression, and refuses a bad or dangerous one without failing", () => {
    expect(run({ text: "^Accepted|Disk", regex: true })).toEqual(["a:1", "b:1"]);
    const bad = compile({ ...noFilter(), text: "(oops", regex: true });
    expect(bad.error).toBeTruthy();
    expect(bad.test(rows[0])).toBe(true);
    for (const risky of ["(a+)+$", "(.*)*x", "(a*){2,}", "(a|ab)*c", "(x+x+)+y", "(a)\\1"]) expect(regexRisk(risky), risky).toBeTruthy();
    for (const fine of ["a+b", "(foo|bar)+", "^\\d{3}-\\d+$", "fail(ed|ure)", "[a-z]+@[a-z]+"]) expect(regexRisk(fine), fine).toBeNull();
    expect(regexRisk("x".repeat(300))).toMatch(/longer/);
    // The risky shape is never run, so it can't hang.
    const t = performance.now();
    applyFilter(rows, compile({ ...noFilter(), text: "(a+)+$", regex: true }));
    expect(performance.now() - t).toBeLessThan(200);
  });

  it("says where the text matched, for highlighting", () => {
    const c = compile({ ...noFilter(), text: "dis" });
    expect(c.spans("Disk and disk")).toEqual([[0, 3], [9, 12]]);
    expect(compile(noFilter()).spans("anything")).toEqual([]);
    expect(compile({ ...noFilter(), text: "a.c", regex: false }).spans("a.c abc")).toEqual([[0, 3]]);
  });

  it("stops at its time limit instead of freezing", () => {
    const many = Array.from({ length: 100_000 }, (_, i) => row("a", i + 1));
    const slow = { error: null, active: true, spans: () => [], test: () => { const t = performance.now(); while (performance.now() - t < 1) { /* spin */ } return true; } };
    const r = applyFilter(many, slow, 30);
    expect(r.timedOut).toBe(true);
    expect(r.rows.length).toBeLessThan(many.length);
  });

  it("finds rows like one, from the words that aren't numbers", () => {
    expect(similarTo("Failed password for invalid user admin from 10.0.0.1 port 22")).toBe("Failed password for invalid");
    expect(similarTo("12345 6789")).toBe("12345 6789");
  });
});

describe("export", () => {
  const rows = [row("a", 1, { message: "plain", priority: 3 }, "claude"), row("b", 1, { message: '=HYPERLINK("x"), "quoted"\nnext', priority: 6, unit: "", ident: "app", pid: undefined }, "Elastic")];

  it("writes text, JSON Lines and CSV with the time, host, severity, unit and message", () => {
    expect(toText(rows).split("\n")[0]).toBe("2023-11-14T22:13:20.001Z claude error sshd.service[10]: plain");
    const lines = toJsonl(rows).trimEnd().split("\n").map((l) => JSON.parse(l));
    expect(lines[0]).toMatchObject({ host: "claude", host_id: "a", severity: "error", unit: "sshd.service", message: "plain", priority: 3 });
    expect(lines[1].message).toContain("\n");
    const csv = toCsv(rows);
    expect(csv.split("\r\n")[0]).toBe("time,host,severity,unit,program,pid,message");
    expect(render(rows, "csv")).toBe(csv);
    expect(rowJson(rows[0])).toBe(JSON.stringify(JSON.parse(rowJson(rows[0]))));
  });

  it("defuses spreadsheet formulas and quotes what needs it", () => {
    expect(csvCell("=1+1")).toBe("'=1+1");
    expect(csvCell("+cmd")).toBe("'+cmd");
    expect(csvCell("-2")).toBe("'-2");
    expect(csvCell("@x")).toBe("'@x");
    expect(csvCell('a"b')).toBe('"a""b"');
    expect(csvCell("a,b")).toBe('"a,b"');
    expect(csvCell(null)).toBe("");
    expect(toCsv(rows)).toContain(`"'=HYPERLINK(""x""), ""quoted""\nnext"`);
  });

  it("names a file from the time and handles nothing to write", () => {
    expect(exportName("jsonl", new Date("2026-11-03T23:04:12Z"))).toBe("sshvault-logs-2026-11-03T23-04-12.jsonl");
    expect(toText([])).toBe("");
    expect(toJsonl([])).toBe("");
  });
});

describe("layout for a long list", () => {
  const rows = Array.from({ length: 50_000 }, (_, i) => row("a", i + 1, { message: i % 10 === 0 ? "x".repeat(300) : "short" }));
  const opts = { wrap: false, charsPerLine: 80, open: null, detailH: 300 };

  it("gives each row one line, a wrapped one a few, and the open one room for its details", () => {
    expect(rowHeight(rows[1], opts)).toBe(ROW_H);
    expect(rowHeight(rows[0], { ...opts, wrap: true })).toBeGreaterThan(ROW_H);
    expect(rowHeight(rows[1], { ...opts, wrap: true })).toBe(ROW_H);
    expect(rowHeight(rows[1], { ...opts, open: rows[1].id })).toBe(ROW_H + 300);
    // A very long message is cut at a few lines.
    expect(rowHeight(rows[0], { ...opts, wrap: true, charsPerLine: 10 })).toBeLessThanOrEqual(6 + 6 * 18);
  });

  it("draws only the rows in view, however many there are", () => {
    const offsets = buildOffsets(rows, opts);
    expect(offsets.length).toBe(rows.length + 1);
    const { start, end } = visibleRows(offsets, 24 * 25_000, 600);
    expect(end - start).toBeLessThan(60);
    expect(start).toBeLessThanOrEqual(25_000);
    expect(end).toBeGreaterThan(25_000);
    expect(visibleRows(offsets, 0, 600).start).toBe(0);
    const last = visibleRows(offsets, offsets[rows.length] - 600, 600);
    expect(last.end).toBe(rows.length);
    expect(visibleRows(buildOffsets([], opts), 0, 100)).toEqual({ start: 0, end: 0 });
  });

  it("builds the offsets of a large list quickly", () => {
    const t = performance.now();
    buildOffsets(rows, opts);
    expect(performance.now() - t).toBeLessThan(500);
  });
});

describe("noticing trouble in the lines", () => {
  const rules = { ...DEFAULT_LOG_RULES, critical: true, auth: true, service: true };
  const r = (host: string, o: Partial<WireEntry>) => row(host, Math.floor(Math.random() * 1e9), o, host);

  it("raises nothing when every rule is off", () => {
    const d = new LogAlertDetector(() => DEFAULT_LOG_RULES);
    expect(d.feed([r("a", { priority: 0, message: "boom" })], 1000)).toEqual([]);
  });

  it("raises a critical entry once per cooldown, per host", () => {
    const d = new LogAlertDetector(() => rules);
    const crit = r("a", { priority: 2, message: "hardware error", unit: "kernel", ident: "kernel" });
    expect(d.feed([crit], 1000)).toHaveLength(1);
    expect(d.feed([crit], 2000)).toEqual([]);
    expect(d.feed([crit], 1000 + 11 * 60_000)).toHaveLength(1);
    expect(d.feed([r("b", { priority: 2, message: "hardware error", unit: "kernel", ident: "kernel" })], 3000)).toHaveLength(1);
    expect(d.feed([r("a", { priority: 3, message: "just an error" })], 4000)).toEqual([]);
  });

  it("raises repeated SSH login failures only at the count, from sshd only", () => {
    const d = new LogAlertDetector(() => ({ ...rules, authCount: 3 }));
    const fail = (ip: string) => r("a", { message: `Failed password for root from ${ip} port 22 ssh2`, ident: "sshd", unit: "ssh.service" });
    expect(d.feed([fail("1.1.1.1"), fail("1.1.1.2")], 1000)).toEqual([]);
    const out = d.feed([fail("203.0.113.9")], 2000);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ kind: "auth", hostId: "a" });
    expect(out[0].message).toMatch(/3 failed SSH logins.*203\.0\.113\.9/);
    // Other programs saying the same words don't count.
    const d2 = new LogAlertDetector(() => ({ ...rules, authCount: 2 }));
    const other = r("a", { message: "Failed password for x", ident: "login", unit: "login.service" });
    expect(d2.feed([other, other, other], 1000)).toEqual([]);
    // Failures spread over longer than the window don't add up.
    const d3 = new LogAlertDetector(() => ({ ...rules, authCount: 2, authWindowSecs: 60 }));
    expect(d3.feed([fail("1.1.1.1")], 0)).toEqual([]);
    expect(d3.feed([fail("1.1.1.1")], 120_000)).toEqual([]);
  });

  it("raises a failed service once, and not a storm", () => {
    const d = new LogAlertDetector(() => rules);
    const svc = (u: string) => r("a", { message: `${u}: Failed with result 'exit-code'.`, unit: u, ident: "systemd" });
    expect(d.feed([svc("nginx.service"), svc("nginx.service")], 1000)).toHaveLength(1);
    const many = ["a", "b", "c", "d", "e", "f", "g", "h"].map((n) => svc(`${n}.service`));
    expect(d.feed(many, 2000).length).toBeLessThanOrEqual(5);
  });

  it("is one alert with a count when the problem comes back while it is still open", () => {
    const engine = new AlertEngine(() => DEFAULT_RULES);
    const first = engine.fromLog("a", "claude", "service", "nginx.service", "claude: nginx.service failed", 1000)[0];
    const again = engine.fromLog("a", "claude", "service", "nginx.service", "claude: nginx.service failed (2)", 5000)[0];
    let r = addAlert([], first, 100);
    expect(r.merged).toBe(false);
    r = addAlert(r.list, again, 100);
    expect(r.merged).toBe(true);
    expect(r.list).toHaveLength(1);
    expect(r.list[0]).toMatchObject({ at: 1000, last: 5000, count: 2 });
    // An acknowledged one is over: the next is new.
    const acked = r.list.map((a) => ({ ...a, ack: 6000 }));
    expect(addAlert(acked, engine.fromLog("a", "claude", "service", "nginx.service", "again", 9000)[0], 100).merged).toBe(false);
    // A different key or host is a different alert.
    expect(addAlert(r.list, engine.fromLog("a", "claude", "service", "cron.service", "x", 9000)[0], 100).merged).toBe(false);
  });
});

describe("the status words", () => {
  it("says how long ago", () => {
    expect(relativeTime(1000, 1500)).toBe("just now");
    expect(relativeTime(0, 12_000)).toBe("12 s ago");
    expect(relativeTime(0, 180_000)).toBe("3 min ago");
    expect(relativeTime(0, 7_200_000)).toBe("2 h ago");
  });

  it("says what a host's stream is doing", () => {
    const h = { attempt: 2, retryAt: 10_000, message: "gone" };
    expect(retryText({ state: "reconnecting", ...h }, 4000)).toBe("reconnecting in 6 s, attempt 2");
    expect(retryText({ state: "reconnecting", ...h, retryAt: null }, 4000)).toBe("reconnecting, attempt 2");
    expect(retryText({ state: "failed", ...h }, 0)).toBe("failed: gone");
    expect(retryText({ state: "streaming", ...h }, 0)).toBe("streaming");
  });

  it("reads the history choice, never asking for more than the host is allowed to give", () => {
    expect(historyOf("last:500")).toEqual({ last: 500 });
    expect(historyOf("last:99999")).toEqual({ last: 5000 });
    expect(historyOf("since:900", 10_000)).toEqual({ since: 9100 });
    expect(historyOf("garbage")).toEqual({ last: 100 });
    expect(historyOf("since:0")).toEqual({ last: 100 });
  });
});
