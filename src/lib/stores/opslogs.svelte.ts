// Operations → Logs: what is being followed, what has arrived, and how it is being looked at. The streaming itself is
// the backend's (src-tauri/src/opslogs.rs); this keeps the selection, turns the backend's events into rows, keeps a
// bounded buffer, filters it, and exports it. It lives in a store so that switching tabs, or leaving Operations,
// never stops a stream: only "Stop following" (or locking the vault) does.
import { untrack } from "svelte";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { render, rowJson, exportName, type ExportFormat } from "$lib/ops/logexport";
import { applyFilter, compile, isFiltered, noFilter, similarTo, type LogFilter } from "$lib/ops/logfilter";
import { DEFAULT_LOG_RULES, LogAlertDetector, type LogAlertRules } from "$lib/ops/logalerts";
import { LogBuffer, Rate, toRow, type HostState, type OpsEvent, type Row, type WireEntry } from "$lib/ops/logrows";
import { UNIT } from "$lib/ops/logs";
import type { PickHost } from "$lib/runbookrun";
import { alerts } from "$lib/stores/alerts.svelte";
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Uuid } from "$lib/types";

export type Phase = "idle" | "connecting" | "streaming" | "reconnecting" | "stopping" | "stopped" | "failed";

export interface HostStream {
  state: HostState;
  attempt: number;
  /** When the next try is due, while waiting for it (milliseconds since 1970). */
  retryAt: number | null;
  message: string | null;
  /** What the command said that isn't a log line. */
  notice: string | null;
  /** Entries received from this host in this stream. */
  count: number;
  lastAt: number | null;
}

export interface Gap {
  hostId: string;
  host: string;
  at: number;
  reason: string;
}

export interface Around {
  state: "loading" | "ready" | "error" | "unavailable";
  before: Row[];
  after: Row[];
  error?: string;
}

/** How long the window waits for the backend to say every host has stopped. */
const STOP_WAIT_MS = 10_000;
const FLUSH_MS = 100;

export const HISTORY_CHOICES: { value: string; label: string; file: boolean }[] = [
  { value: "last:100", label: "Last 100", file: true },
  { value: "last:500", label: "Last 500", file: true },
  { value: "last:1000", label: "Last 1000", file: true },
  { value: "since:900", label: "Last 15 minutes", file: false },
  { value: "since:3600", label: "Last hour", file: false },
  { value: "since:21600", label: "Last 6 hours", file: false },
  { value: "since:86400", label: "Last 24 hours", file: false },
];

export const PRIORITY_CHOICES: { value: number; label: string }[] = [
  { value: -1, label: "All priorities" },
  { value: 7, label: "Debug and above" },
  { value: 6, label: "Info and above" },
  { value: 5, label: "Notice and above" },
  { value: 4, label: "Warning and above" },
  { value: 3, label: "Error and above" },
  { value: 2, label: "Critical and above" },
  { value: 1, label: "Alert and above" },
  { value: 0, label: "Emergency only" },
];

export function historyOf(choice: string, nowSecs = Math.floor(Date.now() / 1000)): { last: number } | { since: number } {
  const [kind, n] = choice.split(":");
  const v = Math.max(0, Math.floor(Number(n)) || 0);
  if (kind === "since" && v > 0) return { since: nowSecs - v };
  return { last: kind === "last" && v > 0 ? Math.min(v, 5000) : 100 };
}

export function logRulesFromPrefs(p: typeof settings.prefs): LogAlertRules {
  return {
    ...DEFAULT_LOG_RULES,
    critical: p.alertLogCritical,
    auth: p.alertLogAuth,
    authCount: p.alertLogAuthCount,
    authWindowSecs: Math.max(1, p.alertLogAuthMins) * 60,
    service: p.alertLogService,
  };
}

class OpsLogs {
  // -- what to follow (kept between visits) ----------------------------------------------------------------
  get hosts(): Uuid[] {
    return settings.prefs.opsLogHosts;
  }
  set hosts(v: Uuid[]) {
    settings.prefs.opsLogHosts = v;
  }

  // -- the running stream ----------------------------------------------------------------------------------
  streamId = $state<string | null>(null);
  stopping = $state(false);
  startedAt = $state<number | null>(null);
  /** What the stream was started with, to tell what has changed since. */
  applied = $state<{ spec: api.OpsLogSpec; hosts: Uuid[] } | null>(null);
  hostStates = $state<Record<string, HostStream>>({});
  gaps = $state<Gap[]>([]);
  startError = $state<string | null>(null);
  starting = $state(false);

  // -- what has arrived -----------------------------------------------------------------------------------
  /** Replaced (never changed in place) at each flush. */
  rows = $state.raw<Row[]>([]);
  received = $state(0);
  /** Entries the backend left out because a host wrote faster than they can be shown. */
  droppedRemote = $state(0);
  evicted = $state(0);
  lastEventAt = $state<number | null>(null);
  #buffer = new LogBuffer(settings.prefs.opsLogMax, settings.prefs.opsLogOrder);
  #rate = new Rate();
  #queue: WireEntry[] = [];
  #queued = new Map<string, string>();
  #flushTimer: ReturnType<typeof setTimeout> | undefined;
  #detector = new LogAlertDetector(() => logRulesFromPrefs(settings.prefs));
  /** The clock, ticked while a stream runs so "last event" and the rate stay current. */
  now = $state(Date.now());
  #clock: ReturnType<typeof setInterval> | undefined;

  constructor() {
    $effect.root(() => {
      // Locking the vault ends every stream and drops what was read.
      $effect(() => {
        if (!vaultStore.unlocked) untrack(() => this.reset());
        else untrack(() => this.prune());
      });
      $effect(() => {
        const max = settings.prefs.opsLogMax;
        untrack(() => this.resize(max));
      });
    });
  }

  // -- looking at it ---------------------------------------------------------------------------------------
  filter = $state<LogFilter>(noFilter());
  paused = $state(false);
  #frozen = $state.raw<Row[]>([]);
  follow = $state(true);
  expanded = $state<string | null>(null);
  selected = $state<string | null>(null);
  contexts = $state<Record<string, Around>>({});
  units = $state<Record<string, { at: number; names: string[] }>>({});

  get compiled() {
    // A time range counted back from now is re-read as the clock ticks; nothing else depends on it.
    const relative = this.filter.range !== "live" && this.filter.range !== "custom";
    return compile(this.filter, relative ? this.now : Date.now());
  }
  /** What the viewer shows: the rows (or the ones frozen by Pause) that pass the filter. */
  filtered = $derived.by(() => applyFilter(this.paused ? this.#frozen : this.rows, this.compiled));
  get shown(): Row[] {
    return this.filtered.rows;
  }
  get isFiltered() {
    return isFiltered(this.filter);
  }
  /** Rows that came in while the view was paused. */
  get arrivedWhilePaused() {
    return this.paused ? Math.max(0, this.rows.length - this.#frozen.length) : 0;
  }

  // -- the choices that make up a stream ----------------------------------------------------------------
  hostOptions = $derived<PickHost[]>(
    vaultStore.hosts
      .filter((h) => h.data && !h.deleted && (!h.data.protocol || h.data.protocol === "ssh"))
      .map((h) => ({
        id: h.id,
        label: h.data!.label,
        hostname: h.data!.hostname,
        group: h.data!.group ?? "",
        tags: h.data!.tags ?? [],
        unattended: !!vaultStore.effectiveIdentity(h.data),
        production: vaultStore.effectiveEnv(h.data) === "production",
      })),
  );

  /** The spec the current choices make, or why they can't. */
  get spec(): { spec: api.OpsLogSpec } | { error: string } {
    const p = settings.prefs;
    let source: api.OpsLogSpec["source"];
    if (p.opsLogSource === "file") {
      const path = p.opsLogPath.trim();
      if (!path) return { error: "Give the path of a log file." };
      if (!path.startsWith("/")) return { error: "The path must be absolute (start with /)." };
      source = { kind: "file", path };
    } else {
      const unit = p.opsLogUnit.trim();
      if (unit && !UNIT.test(unit)) return { error: "The unit must look like nginx.service." };
      source = { kind: "journal", unit: unit || undefined, kernel: p.opsLogSource === "kernel" || undefined, priority: p.opsLogPriority >= 0 ? p.opsLogPriority : undefined };
    }
    let history = historyOf(p.opsLogHistory);
    if (source.kind === "file" && "since" in history) history = { last: 100 };
    return { spec: { source, history, auto_reconnect: p.opsLogAutoReconnect, redact: p.opsLogRedact } };
  }

  /** Why Follow can't be pressed, or null. */
  get blocker(): string | null {
    if (!this.hosts.length) return "Choose at least one host to follow.";
    const usable = this.hosts.filter((id) => this.hostOptions.find((h) => h.id === id)?.unattended);
    if (!usable.length) return "None of the chosen hosts has saved credentials. Logs are read over a second connection that never asks for a password.";
    const s = this.spec;
    return "error" in s ? s.error : null;
  }

  get running() {
    return this.streamId !== null;
  }

  get counts() {
    const c = { connecting: 0, streaming: 0, reconnecting: 0, stopped: 0, failed: 0, total: 0 };
    for (const h of Object.values(this.hostStates)) {
      c[h.state]++;
      c.total++;
    }
    return c;
  }

  get phase(): Phase {
    if (this.stopping) return "stopping";
    if (!this.running) return this.startedAt ? "stopped" : "idle";
    const c = this.counts;
    if (c.streaming > 0) return c.reconnecting + c.failed + c.connecting > 0 ? "reconnecting" : "streaming";
    if (c.reconnecting > 0) return "reconnecting";
    if (c.connecting > 0 || c.total === 0) return "connecting";
    return c.failed > 0 ? "failed" : "stopped";
  }

  /** Selection changes that haven't reached a running stream. */
  get pendingHosts(): { add: Uuid[]; remove: Uuid[] } {
    if (!this.applied) return { add: [], remove: [] };
    const now = new Set(this.hosts);
    const was = new Set(this.applied.hosts);
    return { add: this.hosts.filter((h) => !was.has(h) && this.usable(h)), remove: this.applied.hosts.filter((h) => !now.has(h)) };
  }

  /** Changes to the source, unit, priority, history or switches, which only a restart can apply. */
  get pendingSpec(): boolean {
    const s = this.spec;
    if (!this.applied || "error" in s) return false;
    const a = this.applied.spec;
    const strip = (x: api.OpsLogSpec) => JSON.stringify({ source: x.source, history: "last" in x.history ? x.history : "since", redact: x.redact });
    return strip(s.spec) !== strip(a);
  }

  usable(id: Uuid) {
    return !!this.hostOptions.find((h) => h.id === id)?.unattended;
  }

  label(id: string): string {
    return this.#queued.get(id) ?? vaultStore.hostById.get(id)?.data?.label ?? "host";
  }

  /** Events a minute, from what really arrived in the last minute; null until there is enough to say. */
  get perMinute(): number | null {
    return this.running ? this.#rate.perMinute(this.now) : null;
  }

  // -- starting and stopping ---------------------------------------------------------------------------------
  async start() {
    if (this.running || this.starting || this.blocker) return;
    const built = this.spec;
    if ("error" in built) return;
    const ids = this.hosts.filter((id) => this.usable(id));
    this.starting = true;
    this.startError = null;
    const id = crypto.randomUUID();
    this.#resetView();
    for (const h of ids) this.#queued.set(h, vaultStore.hostById.get(h)?.data?.label ?? "host");
    this.hostStates = Object.fromEntries(ids.map((h) => [h, blank("connecting")]));
    this.streamId = id;
    this.startedAt = Date.now();
    this.applied = { spec: built.spec, hosts: [...ids] };
    this.#tickClock();
    try {
      await api.opsLogs.start(id, built.spec, ids, (e) => this.#onEvent(id, e));
    } catch (e) {
      this.startError = errorMessage(e);
      this.streamId = null;
      this.startedAt = null;
      this.applied = null;
      this.hostStates = {};
      this.#stopClock();
    } finally {
      this.starting = false;
    }
  }

  /** End the remote commands. Each host reports when it has stopped; the window waits for all of them. */
  async stop() {
    const id = this.streamId;
    if (!id || this.stopping) return;
    this.stopping = true;
    try {
      await api.opsLogs.stop(id);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
    // The backend reports every host as stopped; if it somehow doesn't, don't wait for ever.
    setTimeout(() => {
      if (this.streamId === id && this.stopping) this.#finished();
    }, STOP_WAIT_MS);
    this.#maybeFinished();
  }

  async restart() {
    await this.stop();
    const end = Date.now() + STOP_WAIT_MS + 1000;
    while (this.running && Date.now() < end) await new Promise((r) => setTimeout(r, 50));
    await this.start();
  }

  /** Retry lost connections or not. Takes effect at once, in a running stream too. */
  async setAutoReconnect(on: boolean) {
    settings.prefs.opsLogAutoReconnect = on;
    if (this.applied) this.applied = { ...this.applied, spec: { ...this.applied.spec, auto_reconnect: on } };
    if (this.streamId) await api.opsLogs.autoReconnect(this.streamId, on).catch(() => {});
  }

  /** Follow the hosts newly chosen and stop following the ones taken away, without restarting the others. */
  async applyHosts() {
    const id = this.streamId;
    if (!id || !this.applied) return;
    const { add, remove } = this.pendingHosts;
    if (!add.length && !remove.length) return;
    try {
      await api.opsLogs.hosts(id, add, remove);
      for (const h of add) {
        this.#queued.set(h, vaultStore.hostById.get(h)?.data?.label ?? "host");
        this.hostStates = { ...this.hostStates, [h]: blank("connecting") };
      }
      this.applied = { ...this.applied, hosts: [...this.applied.hosts.filter((h) => !remove.includes(h)), ...add] };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  /** Discard what was chosen since the stream started. */
  revertHosts() {
    if (this.applied) this.hosts = [...this.applied.hosts];
  }

  async retryHost(hostId: Uuid) {
    if (this.streamId) await api.opsLogs.retry(this.streamId, hostId).catch(() => {});
  }

  #onEvent(id: string, e: OpsEvent) {
    if (id !== this.streamId) return;
    if (e.event === "state") {
      const was = this.hostStates[e.host_id];
      this.hostStates = {
        ...this.hostStates,
        [e.host_id]: {
          state: e.state,
          attempt: e.attempt,
          retryAt: e.retry_in_secs ? Date.now() + e.retry_in_secs * 1000 : null,
          message: e.message,
          notice: e.notice ?? was?.notice ?? null,
          count: was?.count ?? 0,
          lastAt: was?.lastAt ?? null,
        },
      };
      if (e.state === "stopped") {
        // A host taken out of the stream while it ran is just gone from the list.
        if (!this.stopping && this.applied && !this.applied.hosts.includes(e.host_id)) {
          const { [e.host_id]: _gone, ...rest } = this.hostStates;
          void _gone;
          this.hostStates = rest;
        }
        this.#maybeFinished();
      }
    } else if (e.event === "gap") {
      this.gaps = [...this.gaps, { hostId: e.host_id, host: this.label(e.host_id), at: Date.now(), reason: e.reason }].slice(-50);
    } else {
      if (e.dropped) this.droppedRemote += e.dropped;
      if (e.entries.length) {
        this.#queue.push(...e.entries);
        this.#scheduleFlush();
      }
    }
  }

  #scheduleFlush() {
    if (this.#flushTimer) return;
    this.#flushTimer = setTimeout(() => {
      this.#flushTimer = undefined;
      this.#flush();
    }, FLUSH_MS);
  }

  #flush() {
    const batch = this.#queue;
    if (!batch.length) return;
    this.#queue = [];
    const rows = batch.map((e) => toRow(e, this.label(e.host_id)));
    const now = Date.now();
    const added = this.#buffer.add(rows);
    this.#rate.add(added, now);
    this.received += added;
    this.evicted = this.#buffer.evicted;
    this.lastEventAt = now;
    this.rows = this.#buffer.rows;
    // Per-host counts.
    const counts = new Map<string, number>();
    for (const r of rows) counts.set(r.hostId, (counts.get(r.hostId) ?? 0) + 1);
    const next = { ...this.hostStates };
    for (const [h, n] of counts) if (next[h]) next[h] = { ...next[h], count: next[h].count + n, lastAt: now };
    this.hostStates = next;
    // Trouble in the lines goes to the same alert engine and notifiers as everything else.
    if (settings.prefs.alerts) {
      for (const s of this.#detector.feed(rows, now)) {
        const list = alerts.engine.fromLog(s.hostId, s.host, s.kind, s.key, s.message, s.at);
        alerts.announce(list);
      }
    }
  }

  #maybeFinished() {
    if (!this.streamId) return;
    const all = Object.values(this.hostStates);
    if (all.every((h) => h.state === "stopped")) this.#finished();
  }

  #finished() {
    this.#flush();
    this.streamId = null;
    this.stopping = false;
    this.applied = null;
    this.#stopClock();
    // Hosts keep their last state shown as stopped.
    this.hostStates = Object.fromEntries(Object.entries(this.hostStates).map(([h, s]) => [h, { ...s, state: "stopped" as HostState, retryAt: null }]));
  }

  #tickClock() {
    this.#stopClock();
    this.now = Date.now();
    this.#clock = setInterval(() => (this.now = Date.now()), 1000);
  }

  #stopClock() {
    clearInterval(this.#clock);
    this.#clock = undefined;
  }

  #resetView() {
    this.#buffer.clear();
    this.#rate.reset();
    this.#detector.reset();
    this.#queue = [];
    this.#queued.clear();
    this.rows = [];
    this.received = 0;
    this.droppedRemote = 0;
    this.evicted = 0;
    this.lastEventAt = null;
    this.gaps = [];
    this.contexts = {};
    this.expanded = null;
    this.selected = null;
    this.paused = false;
    this.#frozen = [];
  }

  // -- the viewer -----------------------------------------------------------------------------------------------
  /** Pause or resume the view. Collecting goes on; only what is drawn is held still. */
  togglePause() {
    this.paused = !this.paused;
    if (this.paused) this.#frozen = this.rows;
    else this.follow = true;
  }

  /** Clear what the viewer holds. Nothing on a host is touched. */
  clear() {
    this.#buffer.clear();
    this.#queue = [];
    this.rows = [];
    this.#frozen = [];
    this.evicted = 0;
    this.received = 0;
    this.contexts = {};
    this.expanded = null;
    this.selected = null;
  }

  resize(max: number) {
    this.#buffer.resize(max);
    this.rows = this.#buffer.rows;
    this.evicted = this.#buffer.evicted;
  }

  setOrder(order: "time" | "arrival") {
    this.#buffer.order = order;
    settings.prefs.opsLogOrder = order;
  }

  /** Open a row's details (and read its surroundings), or close it. */
  toggle(row: Row) {
    this.selected = row.id;
    if (this.expanded === row.id) {
      this.expanded = null;
      return;
    }
    this.expanded = row.id;
    void this.loadContext(row);
  }

  async loadContext(row: Row) {
    if (this.contexts[row.id]?.state === "ready" || this.contexts[row.id]?.state === "loading") return;
    if (!row.cursor || !this.streamId) {
      this.contexts = { ...this.contexts, [row.id]: { state: "unavailable", before: [], after: [], error: row.cursor ? "The stream has ended, so the host can't be asked." : "Only journal entries have a position to read around." } };
      return;
    }
    const id = row.id;
    this.contexts = { ...this.contexts, [id]: { state: "loading", before: [], after: [] } };
    try {
      const redact = this.applied?.spec.redact ?? true;
      const r = await api.opsLogs.context(this.streamId, row.hostId, row.cursor, 5, 5, null, redact);
      const label = this.label(row.hostId);
      this.contexts = { ...this.contexts, [id]: { state: "ready", before: r.before.map((e) => toRow({ ...e, host_id: row.hostId, seq: -1 }, label)), after: r.after.map((e) => toRow({ ...e, host_id: row.hostId, seq: -1 }, label)) } };
    } catch (e) {
      this.contexts = { ...this.contexts, [id]: { state: "error", before: [], after: [], error: errorMessage(e) } };
    }
  }

  /** Unit names on a host, for the picker (asked once in a while, and only when the picker is used). */
  async loadUnits(hostId: Uuid) {
    const known = this.units[hostId];
    if (known && Date.now() - known.at < 5 * 60_000) return;
    this.units = { ...this.units, [hostId]: { at: Date.now(), names: known?.names ?? [] } };
    try {
      this.units = { ...this.units, [hostId]: { at: Date.now(), names: await api.opsLogs.units(hostId) } };
    } catch {
      // No suggestions; a unit can still be typed.
    }
  }

  unitNames(): string[] {
    const ids = this.hosts.length ? this.hosts : [];
    const all = new Set<string>();
    for (const id of ids) for (const n of this.units[id]?.names ?? []) all.add(n);
    return [...all].sort();
  }

  // -- acting on rows ----------------------------------------------------------------------------------------------
  filterHost(row: Row) {
    this.filter = { ...this.filter, hosts: [row.hostId] };
  }
  filterUnit(row: Row) {
    const u = row.unit || row.ident;
    if (u) this.filter = { ...this.filter, units: [u] };
  }
  searchSimilar(row: Row) {
    this.filter = { ...this.filter, text: similarTo(row.message), regex: false };
  }

  async copy(text: string, what: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied ${what}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
  copyMessage(row: Row) {
    return this.copy(row.message, "the message");
  }
  copyEvent(row: Row) {
    return this.copy(rowJson(row), "the event");
  }
  copyId(row: Row) {
    return this.copy(row.cursor ?? row.id, row.cursor ? "the journal position" : "the event id");
  }

  /** Copy the rows shown. */
  async copyShown() {
    const rows = this.shown;
    if (!rows.length) return;
    await this.#confirmRaw();
    await this.copy(render(rows, "text"), `${rows.length.toLocaleString()} line${rows.length === 1 ? "" : "s"}`);
  }

  /** True if what is held was read with secret hiding off, which exporting should say. */
  get rawHeld() {
    return this.applied ? !this.applied.spec.redact : !settings.prefs.opsLogRedact && this.rows.length > 0;
  }

  async #confirmRaw(): Promise<boolean> {
    if (!this.rawHeld) return true;
    return ask("These logs were read with secret hiding off, so passwords, tokens or keys in them are not hidden. Continue?", { title: "Unhidden secrets", confirm: "Continue", danger: true });
  }

  async exportShown(format: ExportFormat) {
    const rows = this.shown;
    if (!rows.length) return;
    if (!(await this.#confirmRaw())) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ title: "Save the events shown", defaultPath: exportName(format), filters: [{ name: format.toUpperCase(), extensions: [format === "text" ? "log" : format] }] });
      if (!path) return;
      await api.exportTextFile(path, render(rows, format));
      ui.notify("info", `Saved ${rows.length.toLocaleString()} event${rows.length === 1 ? "" : "s"}${this.rawHeld ? " (secrets not hidden)" : ""}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- from elsewhere ------------------------------------------------------------------------------------------------
  /** Look at one host (and unit): follow it if nothing is being followed, else add it to what is. Applies the filters. */
  async inspect(hostId: Uuid, unit?: string, extra?: { units?: string[]; text?: string; regex?: boolean }) {
    const p = settings.prefs;
    const shown = { text: extra?.text ?? "", regex: extra?.regex ?? false, units: extra?.units ?? [] };
    if (!this.running) {
      this.hosts = [hostId];
      p.opsLogSource = "journal";
      p.opsLogUnit = unit ?? "";
      this.filter = { ...noFilter(), ...shown };
      await this.start();
      return;
    }
    if (!this.hosts.includes(hostId)) {
      this.hosts = [...this.hosts, hostId];
      await this.applyHosts();
    }
    this.filter = { ...noFilter(), ...shown, hosts: [hostId], units: unit ? [unit] : shown.units };
  }

  /** Keep the saved host list to hosts that still exist. */
  prune() {
    const have = new Set(vaultStore.hosts.filter((h) => h.data && !h.deleted).map((h) => h.id));
    if (have.size && this.hosts.some((h) => !have.has(h))) this.hosts = this.hosts.filter((h) => have.has(h));
  }

  /** The vault locked, or the app is closing: nothing may keep running or keep what it read. */
  reset() {
    if (this.streamId) void api.opsLogs.stop(this.streamId).catch(() => {});
    this.streamId = null;
    this.stopping = false;
    this.applied = null;
    this.startedAt = null;
    this.hostStates = {};
    this.#stopClock();
    this.#resetView();
  }
}

const blank = (state: HostState): HostStream => ({ state, attempt: 0, retryAt: null, message: null, notice: null, count: 0, lastAt: null });

export const opsLogs = new OpsLogs();
