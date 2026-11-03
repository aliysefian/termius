import { describe, expect, it } from "vitest";
import type { RunRecord, RunbookCheck, StepResult } from "../runbook";
import {
  addIds, applyRunEvent, applyRunEvents, canRun, canStart, chosenSummary, clock, compilePatterns, defaultStrategyForm, destructiveSteps, elapsed, explainStrategy, filterLog, firstFailedStep, formOf, fromRecord, groupsOf, inGroup,
  logText, MAX_LOG_LINES, missingValues, noFilter, phaseAfter, removeId, retryHosts, riskySteps, searchHosts, splitLines, startRun, strategyLabel, strategyOf, strategyProblem, tagsOf, tally, timeline, validate, valueProblem, type PickHost,
} from "../runbookrun";

const host = (id: string, label: string, over: Partial<PickHost> = {}): PickHost => ({ id, label, hostname: `${label}.example`, group: "", tags: [], unattended: true, production: false, ...over });
const hosts = [
  host("1", "web-01", { group: "Production/Web", tags: ["nginx"], production: true }),
  host("2", "web-02", { group: "Production/Web", tags: ["nginx", "canary"], production: true }),
  host("3", "db-01", { group: "Production/Db", tags: ["postgres"], production: true }),
  host("4", "lab", { group: "Lab", unattended: false }),
];

describe("choosing hosts", () => {
  it("searches by every word in label, address, group or tag", () => {
    expect(searchHosts(hosts, "").length).toBe(4);
    expect(searchHosts(hosts, "WEB").map((h) => h.id)).toEqual(["1", "2"]);
    expect(searchHosts(hosts, "web canary").map((h) => h.id)).toEqual(["2"]);
    expect(searchHosts(hosts, "postgres").map((h) => h.id)).toEqual(["3"]);
    expect(searchHosts(hosts, "lab.example").map((h) => h.id)).toEqual(["4"]);
    expect(searchHosts(hosts, "nothing")).toEqual([]);
  });

  it("counts nested groups and selects a whole group by prefix", () => {
    expect(groupsOf(hosts)).toEqual([
      { name: "Lab", count: 1 },
      { name: "Production", count: 3 },
      { name: "Production/Db", count: 1 },
      { name: "Production/Web", count: 2 },
    ]);
    expect(hosts.filter((h) => inGroup(h, "Production")).map((h) => h.id)).toEqual(["1", "2", "3"]);
    expect(inGroup(host("x", "x", { group: "ProductionX" }), "Production")).toBe(false);
    expect(tagsOf(hosts)).toEqual([{ name: "canary", count: 1 }, { name: "nginx", count: 2 }, { name: "postgres", count: 1 }]);
  });

  it("adds and removes without repeats, keeping the order chosen", () => {
    expect(addIds(["3"], ["1", "3", "2", "1"])).toEqual(["3", "1", "2"]);
    expect(removeId(["1", "2"], "1")).toEqual(["2"]);
    expect(chosenSummary([])).toBe("No hosts selected");
    expect(chosenSummary(hosts.slice(0, 1))).toBe("1 host selected, 1 production");
    expect(chosenSummary(hosts.slice(2))).toBe("2 hosts selected, 1 production");
  });
});

describe("strategies", () => {
  it("round-trips the form and the wire shape, including the old ones", () => {
    for (const s of ["parallel", { sequential: { stop_on_failure: true } }, { concurrent: { limit: 3, stop_on_failure: false } }, { rolling: { batch_size: 4, concurrency: null, stop_on_failure: true } }] as const) {
      const again = strategyOf(formOf(s));
      expect(JSON.stringify(again)).toBe(JSON.stringify(s));
    }
    expect(strategyOf({ ...defaultStrategyForm(), kind: "rolling", batch: 4, concurrency: 2 })).toEqual({ rolling: { batch_size: 4, concurrency: 2, stop_on_failure: false } });
  });

  it("checks the numbers", () => {
    const f = defaultStrategyForm();
    expect(strategyProblem({ ...f, kind: "concurrent", limit: 0 }, 3)).toMatch(/limit/);
    expect(strategyProblem({ ...f, kind: "concurrent", limit: 33 }, 3)).toMatch(/limit/);
    expect(strategyProblem({ ...f, kind: "concurrent", limit: 4 }, 3)).toBeNull();
    expect(strategyProblem({ ...f, kind: "rolling", batch: 2, concurrency: 3 }, 6)).toMatch(/concurrency/);
    expect(strategyProblem({ ...f, kind: "rolling", batch: 0, concurrency: 1 }, 6)).toMatch(/batch size/);
    expect(strategyProblem({ ...f, kind: "rolling", batch: 3, concurrency: 2 }, 6)).toBeNull();
  });

  it("explains each one, and always says steps run in order on a host", () => {
    const f = defaultStrategyForm();
    expect(explainStrategy({ ...f, kind: "rolling", batch: 2, concurrency: 2, stopOnFailure: true }, 5)).toContain("3 batches");
    expect(explainStrategy({ ...f, kind: "rolling", batch: 2, concurrency: 2, stopOnFailure: true }, 5)).toContain("later batches are skipped");
    expect(explainStrategy({ ...f, kind: "sequential" }, 3)).toContain("doesn't stop the next");
    expect(explainStrategy({ ...f, kind: "concurrent", limit: 4, stopOnFailure: true }, 10)).toContain("running ones stop after their current step");
    expect(explainStrategy({ ...f, kind: "parallel" }, 20)).toContain("8 hosts at a time, 20 in all");
    for (const kind of ["parallel", "concurrent", "sequential", "rolling"] as const) expect(explainStrategy({ ...f, kind }, 3)).toContain("steps in order");
    expect(strategyLabel({ rolling: { batch_size: 2, stop_on_failure: true } })).toBe("Rolling, batches of 2, stop after a failed batch");
    expect(strategyLabel(undefined)).toBe("All at once");
  });
});

describe("validate", () => {
  const check = (over: Partial<RunbookCheck> = {}): RunbookCheck => ({ name: "r", description: "", params: [], steps: 4, rollback_steps: 0, problems: [], ...over });
  const base = { check: check(), hosts: hosts.slice(0, 3), missingHosts: 0, values: {}, files: {}, strategy: defaultStrategyForm() };

  it("lists what passed and what was not tested", () => {
    const items = validate(base);
    expect(items.map((i) => i.text)).toContain("Runbook syntax and schema are valid");
    expect(items.map((i) => i.text)).toContain("4 configured steps");
    expect(items.map((i) => i.text)).toContain("3 target hosts selected");
    expect(items.some((i) => i.level === "info" && /Connectivity not tested/.test(i.text))).toBe(true);
    expect(canRun(items)).toBe(true);
  });

  it("is not able to run with problems, no hosts, missing variables or a bad strategy", () => {
    expect(canRun(validate({ ...base, check: check({ problems: [{ step: 1, message: "needs a command" }] }) }))).toBe(false);
    expect(validate({ ...base, check: check({ problems: [{ step: 1, message: "needs a command" }] }) }).some((i) => i.text === "Step 2: needs a command" && i.step === 1)).toBe(true);
    expect(validate({ ...base, check: check({ problems: [{ step: null, line: 14, message: "unclosed" }] }) }).some((i) => i.text === "Line 14: unclosed")).toBe(true);
    expect(canRun(validate({ ...base, hosts: [] }))).toBe(false);
    const params = [{ name: "dest", label: "Folder", default: null, optional: false, choices: [], kind: "dir" as const }, { name: "n", label: "Count", default: null, optional: false, choices: [], kind: "number" as const }];
    const miss = validate({ ...base, check: check({ params }), values: {}, files: {} });
    expect(miss.some((i) => /missing: Folder, Count/.test(i.text))).toBe(true);
    expect(canRun(validate({ ...base, check: check({ params }), values: { n: "3" }, files: { dest: "/tmp" } }))).toBe(true);
    expect(canRun(validate({ ...base, strategy: { ...defaultStrategyForm(), kind: "concurrent", limit: 99 } }))).toBe(false);
    expect(validate({ ...base, missingHosts: 2 }).some((i) => i.level === "error" && /2 selected hosts no longer exist/.test(i.text))).toBe(true);
  });

  it("warns about hosts that can't connect alone, and only says connected after really connecting", () => {
    const items = validate({ ...base, hosts: [hosts[3], hosts[0]] });
    expect(items.some((i) => i.level === "warn" && i.text.includes("lab"))).toBe(true);
    expect(canRun(items)).toBe(true);
    const tested = validate({ ...base, connected: { "1": { ok: true, error: null }, "2": { ok: false, error: "refused" } } });
    expect(tested.some((i) => i.level === "error" && i.text.includes("web-02 (refused)"))).toBe(true);
    const good = validate({ ...base, connected: { "1": { ok: true, error: null }, "2": { ok: true, error: null }, "3": { ok: true, error: null } } });
    expect(good.some((i) => i.level === "ok" && i.text === "Connected to 3 hosts just now")).toBe(true);
    expect(good.some((i) => /Connectivity not tested/.test(i.text))).toBe(false);
    expect(validate({ ...base, usesRemotePaths: true }).some((i) => /not verified until the run/.test(i.text))).toBe(true);
  });

  it("checks typed values against their type", () => {
    const num = { name: "n", label: "", default: null, optional: false, choices: [], kind: "number" as const };
    expect(valueProblem(num, "3.5")).toBeNull();
    expect(valueProblem(num, "abc")).toBe("Not a number.");
    expect(valueProblem({ ...num, kind: "boolean" }, "yes")).toBe("Choose true or false.");
    expect(valueProblem({ ...num, kind: "text", choices: ["a", "b"] }, "c")).toMatch(/One of/);
    expect(valueProblem(num, "  ")).toBeNull();
    expect(missingValues([{ ...num, optional: true }], {}, {})).toEqual([]);
    expect(missingValues([{ ...num, default: "1" }], {}, {})).toEqual([]);
  });
});

const result = (index: number, status: StepResult["status"], over: Partial<StepResult> = {}): StepResult => ({ index, name: `S${index + 1}`, status, output: { stdout: "", stderr: "", exit_code: 0, truncated: false, duration_ms: 12 }, note: "", ...over });

describe("a run that is going", () => {
  const names = ["Check", "Upload", "Deploy"];
  const t0 = 1_700_000_000_000;

  it("follows a host through connecting, running, each step and done", () => {
    let live = startRun([{ id: "a", label: "alpha" }, { id: "b", label: "beta" }], names);
    expect(live.hosts.map((h) => h.state)).toEqual(["pending", "pending"]);
    live = applyRunEvent(live, { event: "host_started", host_id: "a" }, t0);
    expect(live.hosts[0].state).toBe("connecting");
    live = applyRunEvent(live, { event: "connected", host_id: "a" }, t0 + 100);
    live = applyRunEvent(live, { event: "step_started", host_id: "a", index: 0 }, t0 + 120);
    expect(live.hosts[0]).toMatchObject({ state: "running", current: 0 });
    live = applyRunEvent(live, { event: "output", host_id: "a", index: 0, stream: "stdout", text: "one\ntwo\n", seq: 0 }, t0 + 200);
    live = applyRunEvent(live, { event: "step", host_id: "a", result: result(0, "ok") }, t0 + 300);
    expect(live.hosts[0].current).toBeNull();
    expect(live.hosts[0].steps).toHaveLength(1);
    live = applyRunEvent(live, { event: "host_done", host_id: "a", ok: true, error: null, status: "success", duration_ms: 3000 }, t0 + 400);
    expect(live.hosts[0]).toMatchObject({ state: "success", durationMs: 3000 });
    expect(live.hosts[1].state).toBe("pending");
    const lines = live.log.map((l) => `${l.host}|${l.step}|${l.stream}|${l.text}`);
    expect(lines).toContain("alpha|0|stdout|one");
    expect(lines).toContain("alpha|0|stdout|two");
    expect(lines).toContain("alpha|0|info|Step 1/3: Check");
    // A rollback step is numbered on its own and doesn't count in "of N".
    const withRollback = applyRunEvent(startRun([{ id: "a", label: "alpha" }], ["Check", "Ship", "Undo"], 2), { event: "step_started", host_id: "a", index: 2 }, t0);
    expect(withRollback.log[0].text).toBe("Rollback step 1: Undo");
    expect(applyRunEvent(startRun([{ id: "a", label: "alpha" }], ["Check", "Ship", "Undo"], 2), { event: "step_started", host_id: "a", index: 0 }, t0).log[0].text).toBe("Step 1/2: Check");
    expect(lines.at(-1)).toBe("alpha|null|info|Finished");
    expect(live.log.map((l) => l.n)).toEqual(live.log.map((_, i) => i));
  });

  it("marks failures, skips and cancellations with their reason", () => {
    let live = startRun([{ id: "a", label: "alpha" }, { id: "b", label: "beta" }, { id: "c", label: "gamma" }], names);
    live = applyRunEvent(live, { event: "step", host_id: "a", result: result(1, "failed", { note: "exited with 2" }) }, t0);
    live = applyRunEvent(live, { event: "host_done", host_id: "a", ok: false, error: null, status: "failed" }, t0);
    live = applyRunEvent(live, { event: "host_done", host_id: "b", ok: false, error: "skipped: an earlier host failed", status: "skipped" }, t0);
    live = applyRunEvent(live, { event: "host_done", host_id: "c", ok: false, error: "cancelled", status: "cancelled" }, t0);
    expect(live.log.find((l) => l.text.startsWith("S2: Failed"))?.stream).toBe("error");
    expect(live.log.map((l) => l.text)).toContain("Skipped: skipped: an earlier host failed");
    expect(tally(live.hosts)).toMatchObject({ failed: 1, skipped: 1, cancelled: 1, done: 3, pending: 0 });
    live = applyRunEvent(live, { event: "done", cancelled: true }, t0);
    expect(phaseAfter(live)).toBe("cancelled");
  });

  it("derives a status for an event from the older backend that carries only ok", () => {
    let live = startRun([{ id: "a", label: "alpha" }], names);
    live = applyRunEvent(live, { event: "host_done", host_id: "a", ok: true, error: null }, t0);
    expect(live.hosts[0].state).toBe("success");
    live = applyRunEvent(live, { event: "done", cancelled: false }, t0);
    expect(phaseAfter(live)).toBe("completed");
    let bad = startRun([{ id: "a", label: "alpha" }], names);
    bad = applyRunEvent(applyRunEvent(bad, { event: "host_done", host_id: "a", ok: false, error: "x" }, t0), { event: "done", cancelled: false }, t0);
    expect(phaseAfter(bad)).toBe("failed");
    expect(phaseAfter(startRun([], []))).toBe("running");
  });

  it("keeps the log bounded and says how many lines went", () => {
    let live = startRun([{ id: "a", label: "alpha" }], names);
    const piece = Array.from({ length: 1000 }, (_, i) => `line ${i}`).join("\n");
    for (let i = 0; i < 8; i++) live = applyRunEvent(live, { event: "output", host_id: "a", index: 0, stream: "stdout", text: piece, seq: i }, t0);
    expect(live.log).toHaveLength(MAX_LOG_LINES);
    expect(live.dropped).toBe(8000 - MAX_LOG_LINES);
    expect(live.log[0].n).toBe(live.dropped);
    expect(live.log.at(-1)?.text).toBe("line 999");
  });

  it("folds a batch of events the same as one at a time, and never changes the state it was given", () => {
    const events = [
      { event: "host_started", host_id: "a" },
      { event: "connected", host_id: "a" },
      { event: "step_started", host_id: "a", index: 0 },
      { event: "output", host_id: "a", index: 0, stream: "stdout", text: "x\ny\n", seq: 0 },
      { event: "step", host_id: "a", result: result(0, "ok") },
      { event: "host_done", host_id: "a", ok: true, error: null, status: "success", duration_ms: 5 },
      { event: "done", cancelled: false },
    ] as const;
    const start = startRun([{ id: "a", label: "alpha" }], names);
    const frozen = JSON.stringify(start);
    let one = start;
    for (const e of events) one = applyRunEvent(one, e, t0);
    const batch = applyRunEvents(start, [...events], t0);
    expect(batch).toEqual(one);
    expect(JSON.stringify(start)).toBe(frozen);
    expect(applyRunEvents(start, [], t0)).toBe(start);
  });

  it("splits output into lines without an empty last one", () => {
    expect(splitLines("a\nb\n")).toEqual(["a", "b"]);
    expect(splitLines("a\n\nb")).toEqual(["a", "", "b"]);
    expect(splitLines("")).toEqual([]);
  });

  it("draws a host's steps as a timeline: done, running, then the ones to come", () => {
    let live = startRun([{ id: "a", label: "alpha" }], names);
    live = applyRunEvent(live, { event: "step", host_id: "a", result: result(0, "ok") }, t0);
    live = applyRunEvent(live, { event: "step_started", host_id: "a", index: 1 }, t0);
    expect(timeline(live.hosts[0], names).map((s) => s.status)).toEqual(["ok", "running", "pending"]);
    expect(timeline(live.hosts[0], names)[0].durationMs).toBe(12);
    // A rollback step that did not run is not shown as waiting.
    expect(timeline(live.hosts[0], [...names, "Undo"], 3).map((s) => s.status)).toEqual(["ok", "running", "pending"]);
  });
});

describe("the log", () => {
  const live = (() => {
    let l = startRun([{ id: "a", label: "alpha" }, { id: "b", label: "beta" }], ["One", "Two"]);
    l = applyRunEvent(l, { event: "output", host_id: "a", index: 0, stream: "stdout", text: "hello world\n", seq: 0 }, 1_700_000_000_000);
    l = applyRunEvent(l, { event: "output", host_id: "a", index: 1, stream: "stderr", text: "warning: disk\n", seq: 1 }, 1_700_000_001_000);
    l = applyRunEvent(l, { event: "output", host_id: "b", index: 0, stream: "stdout", text: "Hello again\n", seq: 0 }, 1_700_000_002_000);
    return l.log;
  })();

  it("filters by host, step, problems and text, together", () => {
    expect(filterLog(live, noFilter())).toHaveLength(3);
    expect(filterLog(live, { ...noFilter(), hostId: "b" })).toHaveLength(1);
    expect(filterLog(live, { ...noFilter(), step: 1 })).toHaveLength(1);
    expect(filterLog(live, { ...noFilter(), problemsOnly: true }).map((l) => l.text)).toEqual(["warning: disk"]);
    expect(filterLog(live, { ...noFilter(), search: "HELLO" })).toHaveLength(2);
    expect(filterLog(live, { ...noFilter(), search: "hello", hostId: "a" })).toHaveLength(1);
    expect(filterLog(live, { ...noFilter(), search: "beta" })).toHaveLength(1);
  });

  it("copies lines with time, host, step and stream", () => {
    const text = logText(filterLog(live, { ...noFilter(), hostId: "a" }));
    const [first, second] = text.split("\n");
    expect(first).toMatch(/^\d\d:\d\d:\d\d \[alpha\] #1 hello world$/);
    expect(second).toMatch(/\[alpha\] #2 \(stderr\) warning: disk$/);
    expect(clock(0)).toMatch(/^\d\d:\d\d:\d\d$/);
    expect(elapsed(21_000)).toBe("00:21");
    expect(elapsed(3_725_000)).toBe("1:02:05");
    expect(elapsed(null)).toBe("");
  });
});

describe("the button and retrying", () => {
  it("allows a new run only when none is being set up or going", () => {
    for (const p of ["validating", "starting", "running", "cancelling"] as const) expect(canStart(p)).toBe(false);
    for (const p of ["idle", "completed", "failed", "cancelled"] as const) expect(canStart(p)).toBe(true);
  });

  it("retries the hosts that did not succeed, and starts from the earliest failed step", () => {
    let live = startRun([{ id: "a", label: "a" }, { id: "b", label: "b" }, { id: "c", label: "c" }, { id: "d", label: "d" }], ["1", "2", "3"]);
    live = applyRunEvent(live, { event: "step", host_id: "a", result: result(2, "failed") }, 0);
    live = applyRunEvent(live, { event: "step", host_id: "b", result: result(1, "timed_out") }, 0);
    live = applyRunEvent(live, { event: "step", host_id: "b", result: result(5, "failed", { phase: "rollback" }) }, 0);
    for (const [id, status] of [["a", "failed"], ["b", "timed_out"], ["c", "success"], ["d", "cancelled"]] as const) live = applyRunEvent(live, { event: "host_done", host_id: id, ok: status === "success", error: null, status }, 0);
    expect(retryHosts(live.hosts, "failed")).toEqual(["a", "b", "d"]);
    expect(retryHosts(live.hosts, "all")).toEqual(["a", "b", "c", "d"]);
    expect(firstFailedStep(live.hosts)).toBe(1);
    expect(firstFailedStep(startRun([{ id: "a", label: "a" }], []).hosts)).toBeNull();
  });

  it("calls out steps that are not known to be safe to run twice", () => {
    const steps = [
      { name: "Check", kind: "http", text: "GET /" },
      { name: "Restart", kind: "run", text: "systemctl restart x" },
      { name: "Wait", kind: "wait", text: "true" },
      { name: "Ship", kind: "upload", text: "up" },
      { name: "Clean", kind: "wait", text: "rm -rf /tmp/x" },
      { name: "Off", kind: "run", text: "x", disabled: true },
    ];
    const rx = compilePatterns(["rm\\s+-rf", "(", "drop table"]);
    expect(rx).toHaveLength(2);
    expect(riskySteps(steps, null, rx)).toEqual(["2. Restart", "4. Ship", "5. Clean"]);
    expect(riskySteps(steps, 3, rx)).toEqual(["4. Ship", "5. Clean"]);
    expect(destructiveSteps(steps, rx)).toEqual(["Clean"]);
  });
});

describe("a past run in the same console", () => {
  it("is turned back into hosts, step results and log lines", () => {
    const r: RunRecord = {
      id: "r1", runbook: "Deploy", started_at: 1_700_000_000, finished_at: 1_700_000_010, scheduled: false, cancelled: false, params: {}, step_names: ["Check", "Ship"],
      hosts: [
        { host_id: "a", label: "alpha", ok: true, error: null, status: "success", started_ms: 1_700_000_000_500, finished_ms: 1_700_000_003_500, steps: [result(0, "ok", { output: { stdout: "fine\n", stderr: "", exit_code: 0, truncated: false, duration_ms: 5 } })] },
        { host_id: "b", label: "beta", ok: false, error: "no route", steps: [] },
      ],
    };
    const live = fromRecord(r);
    expect(live.hosts.map((h) => [h.state, h.durationMs])).toEqual([["success", 3000], ["failed", null]]);
    expect(live.stepNames).toEqual(["Check", "Ship"]);
    expect(live.log.map((l) => l.text)).toEqual(["fine", "no route"]);
    expect(live.finished).toBe(true);
    // A record from before statuses existed still reads.
    expect(fromRecord({ ...r, hosts: [{ host_id: "x", label: "x", ok: null, error: null, steps: [] }], finished_at: null }).hosts[0].state).toBe("running");
  });
});
