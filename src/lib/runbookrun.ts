// The run side of the Runbooks workspace, as pure functions: how hosts are chosen and grouped, what a strategy means,
// what "validate" reports, what is shown while a run goes (hosts, steps, a bounded log) and what a retry would do.
// The backend does the checking and the running; this folds what it says into what the window draws.
import type { HostRecord, HostStatus, Problem, RunRecord, RunbookCheck, RunbookEvent, RunbookParam, StepResult, Strategy } from "./runbook";

// -- choosing hosts -----------------------------------------------------------------------------------

export interface PickHost {
  id: string;
  label: string;
  hostname: string;
  group: string;
  tags: string[];
  /** Has credentials saved, so it can connect with nobody there to type anything. */
  unattended: boolean;
  production: boolean;
}

/** Hosts matching the words typed: every word must be in the label, address, group or a tag, in any case. */
export function searchHosts(hosts: PickHost[], query: string): PickHost[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return hosts;
  return hosts.filter((h) => {
    const hay = [h.label, h.hostname, h.group, ...h.tags].join("\n").toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** The groups hosts are in ("Production/Web" counts as both "Production" and "Production/Web"), with how many each holds. */
export function groupsOf(hosts: PickHost[]): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const h of hosts) {
    if (!h.group) continue;
    const parts = h.group.split("/").filter(Boolean);
    for (let i = 1; i <= parts.length; i++) {
      const g = parts.slice(0, i).join("/");
      counts.set(g, (counts.get(g) ?? 0) + 1);
    }
  }
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => a.name.localeCompare(b.name));
}

export const inGroup = (h: PickHost, group: string) => h.group === group || h.group.startsWith(`${group}/`);

export function tagsOf(hosts: PickHost[]): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const h of hosts) for (const t of h.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => a.name.localeCompare(b.name));
}

/** The chosen ids with `add` included, without repeats, in the order chosen. */
export function addIds(chosen: string[], add: string[]): string[] {
  const seen = new Set(chosen);
  return [...chosen, ...add.filter((id) => !seen.has(id) && seen.add(id))];
}

export const removeId = (chosen: string[], id: string) => chosen.filter((x) => x !== id);

/** What the picker says about the choice, for the line above the chips. */
export function chosenSummary(chosen: PickHost[]): string {
  if (chosen.length === 0) return "No hosts selected";
  const prod = chosen.filter((h) => h.production).length;
  return `${chosen.length} host${chosen.length === 1 ? "" : "s"} selected${prod ? `, ${prod} production` : ""}`;
}

// -- strategies ---------------------------------------------------------------------------------------

export type StrategyKind = "parallel" | "concurrent" | "sequential" | "rolling";

export interface StrategyForm {
  kind: StrategyKind;
  limit: number;
  batch: number;
  concurrency: number;
  stopOnFailure: boolean;
}

export const MAX_CONCURRENCY = 32;

export const defaultStrategyForm = (): StrategyForm => ({ kind: "parallel", limit: 4, batch: 2, concurrency: 2, stopOnFailure: false });

export function formOf(s: Strategy | null | undefined): StrategyForm {
  const f = defaultStrategyForm();
  if (!s || s === "parallel") return f;
  if ("concurrent" in s) return { ...f, kind: "concurrent", limit: s.concurrent.limit, stopOnFailure: !!s.concurrent.stop_on_failure };
  if ("sequential" in s) return { ...f, kind: "sequential", stopOnFailure: !!s.sequential.stop_on_failure };
  return { ...f, kind: "rolling", batch: s.rolling.batch_size, concurrency: s.rolling.concurrency ?? s.rolling.batch_size, stopOnFailure: !!s.rolling.stop_on_failure };
}

export function strategyOf(f: StrategyForm): Strategy {
  switch (f.kind) {
    case "parallel":
      return "parallel";
    case "concurrent":
      return { concurrent: { limit: f.limit, stop_on_failure: f.stopOnFailure } };
    case "sequential":
      return { sequential: { stop_on_failure: f.stopOnFailure } };
    case "rolling":
      return { rolling: { batch_size: f.batch, concurrency: f.concurrency >= f.batch ? null : f.concurrency, stop_on_failure: f.stopOnFailure } };
  }
}

/** What is wrong with the settings, or null. */
export function strategyProblem(f: StrategyForm, hosts: number): string | null {
  const bad = (n: number) => !Number.isInteger(n) || n < 1 || n > MAX_CONCURRENCY;
  if (f.kind === "concurrent" && bad(f.limit)) return `The limit is a whole number from 1 to ${MAX_CONCURRENCY}.`;
  if (f.kind === "rolling") {
    if (bad(f.batch)) return `The batch size is a whole number from 1 to ${MAX_CONCURRENCY}.`;
    if (!Number.isInteger(f.concurrency) || f.concurrency < 1 || f.concurrency > f.batch) return "The concurrency is from 1 to the batch size.";
  }
  void hosts;
  return null;
}

/** The choices, with what each does, in the words the panel shows. */
export const STRATEGIES: { kind: StrategyKind; label: string; about: string }[] = [
  { kind: "parallel", label: "All at once", about: "Every host at the same time, up to eight at a time." },
  { kind: "concurrent", label: "Limited concurrency", about: "At most this many hosts at the same time." },
  { kind: "sequential", label: "One at a time", about: "One host after another, in the order chosen." },
  { kind: "rolling", label: "Rolling", about: "Batches of hosts, one batch after another. Good for changing a fleet without taking all of it down." },
];

/** One sentence that says what the settings will do, including the part people confuse: steps always run in order on each host. */
export function explainStrategy(f: StrategyForm, hosts: number): string {
  const n = hosts;
  const stop = f.stopOnFailure;
  const head = "Each host runs the steps in order.";
  switch (f.kind) {
    case "parallel":
      return `${head} ${n <= 1 ? "" : `${Math.min(n, 8)} host${Math.min(n, 8) === 1 ? "" : "s"} at a time${n > 8 ? `, ${n} in all` : ""}.`} A host that fails doesn't stop the others.`.replace("  ", " ").trim();
    case "concurrent":
      return `${head} Up to ${f.limit} hosts at a time${n ? `, ${n} in all` : ""}. ${stop ? "The first host that fails stops the run: hosts not yet started are skipped and running ones stop after their current step." : "A host that fails doesn't stop the others."}`;
    case "sequential":
      return `${head} One host at a time, in the order chosen. ${stop ? "The first host that fails ends the run; the rest are skipped." : "A host that fails doesn't stop the next."}`;
    case "rolling": {
      const batches = n ? Math.ceil(n / Math.max(1, f.batch)) : 0;
      return `${head} Hosts go in batches of ${f.batch}${batches ? ` (${batches} batch${batches === 1 ? "" : "es"})` : ""}, up to ${Math.min(f.concurrency, f.batch)} at a time inside a batch. ${stop ? "A batch with a failed host is the last one; later batches are skipped." : "A failed host doesn't stop later batches."}`;
    }
  }
}

/** A short name for a strategy, for a record. */
export function strategyLabel(s: Strategy | null | undefined): string {
  const f = formOf(s);
  switch (f.kind) {
    case "parallel":
      return "All at once";
    case "concurrent":
      return `${f.limit} at a time${f.stopOnFailure ? ", stop on failure" : ""}`;
    case "sequential":
      return `One at a time${f.stopOnFailure ? ", stop on failure" : ""}`;
    case "rolling":
      return `Rolling, batches of ${f.batch}${f.stopOnFailure ? ", stop after a failed batch" : ""}`;
  }
}

// -- validating ---------------------------------------------------------------------------------------

export type CheckLevel = "ok" | "warn" | "error" | "info";
export interface ValidationItem {
  level: CheckLevel;
  text: string;
  /** Where to look: a step (0-based) or the document. */
  step?: number | null;
}

export interface ValidateInput {
  check: RunbookCheck | null;
  hosts: PickHost[];
  /** Hosts chosen that don't exist (deleted). */
  missingHosts: number;
  values: Record<string, string>;
  files: Record<string, string>;
  strategy: StrategyForm;
  /** Hosts really connected to just now, by id, if a connection test was made. */
  connected?: Record<string, { ok: boolean; error: string | null }> | null;
  /** The steps that act on this computer and read or write a local path (so the path can't be checked from here). */
  usesRemotePaths?: boolean;
}

/** What is still missing from the values a runbook needs to run. */
export function missingValues(params: RunbookParam[], values: Record<string, string>, files: Record<string, string>): string[] {
  const out: string[] = [];
  for (const p of params) {
    if (p.optional) continue;
    const label = p.label || p.name;
    if (p.kind === "file" || p.kind === "dir") {
      if (!files[p.name]) out.push(label);
    } else if (!(values[p.name] ?? "").trim() && p.default === null) out.push(label);
  }
  return out;
}

/** Whether a typed value suits the variable's type. */
export function valueProblem(p: RunbookParam, value: string): string | null {
  const v = value.trim();
  if (v === "") return null;
  if (p.kind === "number" && (!Number.isFinite(Number(v)) || v === "")) return "Not a number.";
  if (p.kind === "boolean" && v !== "true" && v !== "false") return "Choose true or false.";
  if (p.choices.length && !p.choices.includes(v)) return `One of: ${p.choices.join(", ")}.`;
  if (v.length > 4096) return "Too long.";
  return null;
}

/** The report the Validate action shows. Every line is something that was checked; what was not checked is said as a note. */
export function validate(i: ValidateInput): ValidationItem[] {
  const out: ValidationItem[] = [];
  const c = i.check;
  if (!c) return [{ level: "error", text: "The runbook hasn't been checked yet." }];
  const problems: Problem[] = c.problems;
  if (problems.length === 0) {
    out.push({ level: "ok", text: "Runbook syntax and schema are valid" });
    out.push({ level: "ok", text: `${c.steps} configured step${c.steps === 1 ? "" : "s"}${c.rollback_steps ? `, ${c.rollback_steps} rollback step${c.rollback_steps === 1 ? "" : "s"}` : ""}` });
  } else {
    for (const p of problems) out.push({ level: "error", step: p.step, text: `${p.step !== null ? `Step ${p.step + 1}: ` : p.line ? `Line ${p.line}: ` : ""}${p.message}` });
  }
  const miss = problems.length ? [] : missingValues(c.params, i.values, i.files);
  for (const p of c.params) {
    const v = i.values[p.name] ?? p.default ?? "";
    const bad = p.kind === "file" || p.kind === "dir" ? null : valueProblem(p, v);
    if (bad) out.push({ level: "error", text: `${p.label || p.name}: ${bad}` });
  }
  if (!problems.length) {
    if (miss.length) out.push({ level: "error", text: `Required ${miss.length === 1 ? "variable" : "variables"} missing: ${miss.join(", ")}` });
    else if (c.params.length) out.push({ level: "ok", text: "Required variables supplied" });
  }
  if (i.hosts.length === 0) out.push({ level: "error", text: "No target hosts selected" });
  else out.push({ level: "ok", text: `${i.hosts.length} target host${i.hosts.length === 1 ? "" : "s"} selected` });
  if (i.missingHosts) out.push({ level: "error", text: `${i.missingHosts} selected host${i.missingHosts === 1 ? " no longer exists" : "s no longer exist"}` });
  const attended = i.hosts.filter((h) => !h.unattended);
  if (attended.length) out.push({ level: "warn", text: `No saved credentials, so they can't connect on their own: ${attended.map((h) => h.label).join(", ")}` });
  else if (i.hosts.length) out.push({ level: "ok", text: "Connection settings are saved for every host" });
  const prob = strategyProblem(i.strategy, i.hosts.length);
  if (prob) out.push({ level: "error", text: `Execution strategy: ${prob}` });
  if (i.connected) {
    const bad = i.hosts.filter((h) => i.connected![h.id] && !i.connected![h.id].ok);
    const tried = i.hosts.filter((h) => i.connected![h.id]);
    if (tried.length) out.push(bad.length ? { level: "error", text: `Could not connect to ${bad.map((h) => `${h.label} (${i.connected![h.id].error ?? "no answer"})`).join(", ")}` } : { level: "ok", text: `Connected to ${tried.length} host${tried.length === 1 ? "" : "s"} just now` });
  } else if (i.hosts.length) {
    out.push({ level: "info", text: "Connectivity not tested yet. Use Test connections to really connect." });
  }
  if (i.usesRemotePaths) out.push({ level: "info", text: "Remote paths and permissions are not verified until the run" });
  return out;
}

export const canRun = (items: ValidationItem[]) => items.length > 0 && !items.some((x) => x.level === "error");

// -- a run that is going -------------------------------------------------------------------------------

export type HostState = HostStatus;

export interface StepLive {
  index: number;
  name: string;
  status: "pending" | "running" | "ok" | "failed" | "skipped" | "timed_out" | "error";
  durationMs: number | null;
  note: string;
}

export interface HostRun {
  hostId: string;
  label: string;
  state: HostState;
  /** The step running now. */
  current: number | null;
  steps: StepResult[];
  error: string | null;
  startedAt: number | null;
  durationMs: number | null;
}

export interface LogLine {
  /** Counts up across the whole run, for keys and ordering. */
  n: number;
  at: number;
  hostId: string;
  host: string;
  step: number | null;
  stream: "stdout" | "stderr" | "info" | "error";
  text: string;
}

/** The most lines kept in the window for one run; older ones are dropped (the run's record keeps each step's output). */
export const MAX_LOG_LINES = 5000;

export interface RunLive {
  hosts: HostRun[];
  log: LogLine[];
  /** How many lines were dropped from the front of the log. */
  dropped: number;
  next: number;
  finished: boolean;
  cancelled: boolean;
  stepNames: string[];
}

export function startRun(hosts: { id: string; label: string }[], stepNames: string[]): RunLive {
  return {
    hosts: hosts.map((h) => ({ hostId: h.id, label: h.label, state: "pending", current: null, steps: [], error: null, startedAt: null, durationMs: null })),
    log: [],
    dropped: 0,
    next: 0,
    finished: false,
    cancelled: false,
    stepNames,
  };
}

const stepName = (live: RunLive, i: number) => live.stepNames[i] ?? `Step ${i + 1}`;

function push(live: RunLive, line: Omit<LogLine, "n">): RunLive {
  const log = [...live.log, { ...line, n: live.next }];
  const over = log.length - MAX_LOG_LINES;
  return { ...live, log: over > 0 ? log.slice(over) : log, dropped: live.dropped + Math.max(0, over), next: live.next + 1 };
}

/** The text of an output piece as lines (a trailing newline doesn't make an empty one). */
export function splitLines(text: string): string[] {
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines;
}

const hostLabel = (live: RunLive, id: string) => live.hosts.find((h) => h.hostId === id)?.label ?? id;

/** Fold one event into the live state. Pure: the same events give the same state. */
export function applyRunEvent(live: RunLive, e: RunbookEvent, now: number): RunLive {
  if (e.event === "done") return { ...live, finished: true, cancelled: e.cancelled };
  const patch = (f: (h: HostRun) => HostRun): RunLive => ({ ...live, hosts: live.hosts.map((h) => (h.hostId === e.host_id ? f(h) : h)) });
  const host = hostLabel(live, e.host_id);
  switch (e.event) {
    case "host_started":
      return push(patch((h) => ({ ...h, state: "connecting", startedAt: now })), { at: now, hostId: e.host_id, host, step: null, stream: "info", text: "Connecting…" });
    case "connected":
      return push(patch((h) => ({ ...h, state: "running" })), { at: now, hostId: e.host_id, host, step: null, stream: "info", text: "Connected" });
    case "step_started":
      return push(patch((h) => ({ ...h, current: e.index, state: h.state === "connecting" || h.state === "pending" ? "running" : h.state })), { at: now, hostId: e.host_id, host, step: e.index, stream: "info", text: `Step ${e.index + 1}/${live.stepNames.length || "?"}: ${stepName(live, e.index)}` });
    case "output": {
      let next = live;
      for (const text of splitLines(e.text)) next = push(next, { at: now, hostId: e.host_id, host, step: e.index, stream: e.stream, text });
      return next;
    }
    case "step": {
      const r = e.result;
      const done = patch((h) => ({ ...h, steps: [...h.steps, r], current: h.current === r.index ? null : h.current }));
      const bad = r.status === "failed" || r.status === "error" || r.status === "timed_out";
      const text = r.status === "skipped" ? `Skipped: ${r.note}` : r.status === "ok" ? `Done${r.note ? ` (${r.note})` : ""}${r.output.duration_ms ? ` in ${r.output.duration_ms} ms` : ""}` : `${r.status === "timed_out" ? "Timed out" : "Failed"}${r.note ? `: ${r.note}` : ""}`;
      return push(done, { at: now, hostId: e.host_id, host, step: r.index, stream: bad ? "error" : "info", text: `${r.name}: ${text}` });
    }
    case "host_done": {
      const status: HostStatus = e.status ?? (e.ok ? "success" : "failed");
      const next = patch((h) => ({ ...h, state: status, current: null, error: e.error, durationMs: e.duration_ms ?? (h.startedAt ? now - h.startedAt : null) }));
      return push(next, { at: now, hostId: e.host_id, host, step: null, stream: e.ok ? "info" : "error", text: e.ok ? "Finished" : `${status === "skipped" ? "Skipped" : status === "cancelled" ? "Cancelled" : status === "timed_out" ? "Timed out" : "Failed"}${e.error ? `: ${e.error}` : ""}` });
    }
  }
}

/** Counts of hosts by state, for the line at the top of the console. */
export function tally(hosts: HostRun[]): { total: number; pending: number; running: number; success: number; failed: number; skipped: number; cancelled: number; done: number } {
  const t = { total: hosts.length, pending: 0, running: 0, success: 0, failed: 0, skipped: 0, cancelled: 0, done: 0 };
  for (const h of hosts) {
    if (h.state === "pending") t.pending++;
    else if (h.state === "connecting" || h.state === "running") t.running++;
    else if (h.state === "success") t.success++;
    else if (h.state === "skipped") t.skipped++;
    else if (h.state === "cancelled") t.cancelled++;
    else t.failed++;
  }
  t.done = t.success + t.failed + t.skipped + t.cancelled;
  return t;
}

/** The steps of one host as a timeline: finished ones from their results, the running one, then the ones still to come. */
export function timeline(host: HostRun, stepNames: string[], rollbackFrom = stepNames.length): StepLive[] {
  const out: StepLive[] = [];
  const by = new Map(host.steps.map((r) => [r.index, r]));
  for (let i = 0; i < stepNames.length; i++) {
    const r = by.get(i);
    if (r) out.push({ index: i, name: stepNames[i], status: r.status, durationMs: r.output.duration_ms || null, note: r.note });
    else if (host.current === i) out.push({ index: i, name: stepNames[i], status: "running", durationMs: null, note: "" });
    else if (i >= rollbackFrom) continue;
    else out.push({ index: i, name: stepNames[i], status: "pending", durationMs: null, note: "" });
  }
  return out;
}

export interface LogFilter {
  hostId: string | null;
  step: number | null;
  /** Only errors and stderr. */
  problemsOnly: boolean;
  search: string;
}

export const noFilter = (): LogFilter => ({ hostId: null, step: null, problemsOnly: false, search: "" });

export function filterLog(log: LogLine[], f: LogFilter): LogLine[] {
  const q = f.search.trim().toLowerCase();
  return log.filter((l) => (f.hostId === null || l.hostId === f.hostId) && (f.step === null || l.step === f.step) && (!f.problemsOnly || l.stream === "stderr" || l.stream === "error") && (!q || l.text.toLowerCase().includes(q) || l.host.toLowerCase().includes(q)));
}

const pad = (n: number) => String(n).padStart(2, "0");
export const clock = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};

/** A log line as it is copied or exported: time, host, step, text. */
export function lineText(l: LogLine): string {
  return `${clock(l.at)} [${l.host}]${l.step !== null ? ` #${l.step + 1}` : ""}${l.stream === "stderr" ? " (stderr)" : ""} ${l.text}`;
}

export const logText = (log: LogLine[]) => log.map(lineText).join("\n");

/** A mm:ss (or h:mm:ss) clock for a duration. */
export function elapsed(ms: number | null): string {
  if (ms === null || ms < 0) return "";
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${pad(m)}:${pad(s % 60)}`;
}

/** A finished run's record turned back into the live shape, so a past run is read in the same console. */
export function fromRecord(r: RunRecord): RunLive {
  const names = r.step_names?.length ? r.step_names : (r.hosts[0]?.steps ?? []).map((s) => s.name);
  const hosts: HostRun[] = r.hosts.map((h: HostRecord) => ({
    hostId: h.host_id,
    label: h.label,
    state: h.status ?? (h.ok === null ? "running" : h.ok ? "success" : "failed"),
    current: null,
    steps: h.steps,
    error: h.error,
    startedAt: h.started_ms ?? null,
    durationMs: h.started_ms && h.finished_ms ? h.finished_ms - h.started_ms : null,
  }));
  // The output each step kept, as log lines in the order of the steps.
  let live: RunLive = { hosts, log: [], dropped: 0, next: 0, finished: r.finished_at !== null, cancelled: r.cancelled, stepNames: names };
  for (const h of r.hosts) {
    for (const s of h.steps) {
      const at = (h.started_ms ?? r.started_at * 1000) + 0;
      for (const text of splitLines(s.output.stdout)) live = push(live, { at, hostId: h.host_id, host: h.label, step: s.index, stream: "stdout", text });
      for (const text of splitLines(s.output.stderr)) live = push(live, { at, hostId: h.host_id, host: h.label, step: s.index, stream: "stderr", text });
      if (s.status !== "ok" && s.status !== "skipped") live = push(live, { at, hostId: h.host_id, host: h.label, step: s.index, stream: "error", text: `${s.name}: ${s.status === "timed_out" ? "timed out" : "failed"}${s.note ? `: ${s.note}` : ""}` });
    }
    if (h.error) live = push(live, { at: h.finished_ms ?? r.started_at * 1000, hostId: h.host_id, host: h.label, step: null, stream: "error", text: h.error });
  }
  return live;
}

// -- the button ----------------------------------------------------------------------------------------

export type RunPhase = "idle" | "validating" | "starting" | "running" | "cancelling" | "completed" | "failed" | "cancelled";

export const RUN_LABEL: Record<RunPhase, string> = {
  idle: "Run now",
  validating: "Validating…",
  starting: "Starting…",
  running: "Running…",
  cancelling: "Cancelling…",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};

/** Whether a new run may be started from here: not while one is being set up or going. */
export const canStart = (p: RunPhase) => p === "idle" || p === "completed" || p === "failed" || p === "cancelled";

export function phaseAfter(live: RunLive): RunPhase {
  if (!live.finished) return "running";
  if (live.cancelled) return "cancelled";
  return live.hosts.some((h) => h.state === "failed" || h.state === "timed_out" || h.state === "cancelled" || h.state === "skipped") ? "failed" : "completed";
}

// -- trying again --------------------------------------------------------------------------------------

export interface RetryPlan {
  hostIds: string[];
  /** The first step to run again, or null for the whole runbook. */
  fromStep: number | null;
  /** Steps that would run again on a host where they already did, and that change something. */
  risky: string[];
}

/** Which hosts a retry takes: those that failed, timed out or were cancelled or skipped. */
export function retryHosts(hosts: HostRun[], what: "failed" | "all"): string[] {
  return hosts.filter((h) => what === "all" || ["failed", "timed_out", "cancelled", "skipped"].includes(h.state)).map((h) => h.hostId);
}

/** The first step that failed (or timed out) on any of the hosts, which is where "retry from the failed step" starts. */
export function firstFailedStep(hosts: HostRun[]): number | null {
  let first: number | null = null;
  for (const h of hosts) for (const s of h.steps) if ((s.status === "failed" || s.status === "error" || s.status === "timed_out") && s.phase !== "rollback" && (first === null || s.index < first)) first = s.index;
  return first;
}

/** The step kinds that are safe to run twice: reading and checking. Everything else is asked about. */
const REPEATABLE = new Set(["wait", "delay", "http", "tcp", "download"]);

export interface StepInfo {
  name: string;
  kind: string;
  /** The text of a command, for matching against destructive patterns. */
  text: string;
  disabled?: boolean;
}

/** The steps a retry from `from` would run again, that aren't known to be safe to repeat. */
export function riskySteps(steps: StepInfo[], from: number | null, destructive: RegExp[]): string[] {
  return steps
    .map((s, i) => ({ s, i }))
    .filter(({ s, i }) => !s.disabled && (from === null || i >= from) && (!REPEATABLE.has(s.kind) || destructive.some((r) => r.test(s.text))))
    .map(({ s, i }) => `${i + 1}. ${s.name}`);
}

/** The destructive-command patterns of the vault as regular expressions; a pattern that isn't one is skipped. */
export function compilePatterns(patterns: string[]): RegExp[] {
  const out: RegExp[] = [];
  for (const p of patterns) {
    try {
      out.push(new RegExp(p, "i"));
    } catch {
      // A pattern that can't be compiled just doesn't match.
    }
  }
  return out;
}

/** The steps that match a destructive pattern, for the confirmation before a run. */
export function destructiveSteps(steps: StepInfo[], patterns: RegExp[]): string[] {
  return steps.filter((s) => !s.disabled && patterns.some((r) => r.test(s.text))).map((s) => s.name);
}
