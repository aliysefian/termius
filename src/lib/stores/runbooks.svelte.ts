// The Runbooks workspace's state: which runbook is open and its text, what the backend says about it, what a run needs (values,
// hosts, strategy), the run that is going and the record of past ones. Saved runbooks are in the vault store; everything about
// a run is per window and goes away when the vault locks.
import { open, save } from "@tauri-apps/plugin-dialog";
import * as api from "$lib/api";
import { ask, askText, choose } from "$lib/dialogs.svelte";
import type { PlannedStep, RunRecord, RunSummary, RunbookCheck, RunbookEvent } from "$lib/runbook";
import { addStep, convert, duplicateStep, format as formatText, formatOf, kindOf, load, moveStep, newStep, positions, removeStep, setStepField, setTop, updateStep, type Edit, type Format, type Obj, type StepKind } from "$lib/runbookdoc";
import { buildLibrary, copyName, toggleFavorite, type LibraryItem } from "$lib/runbooklib";
import {
  applyRunEvents, canRun, canStart, compilePatterns, defaultStrategyForm, destructiveSteps, firstFailedStep, formOf, fromRecord, phaseAfter, retryHosts, riskySteps, startRun, strategyOf, validate,
  type PickHost, type RunLive, type RunPhase, type StepInfo, type StrategyForm, type ValidationItem,
} from "$lib/runbookrun";
import { BLANK } from "$lib/runbooktemplates";
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Uuid } from "$lib/types";

export type WorkspaceTab = "runbooks" | "history" | "schedules" | "templates";
export type EditorMode = "builder" | "yaml" | "variables" | "settings";
export type ConsoleTab = "logs" | "output" | "summary";

/** What a run was started with, kept so "run again" repeats exactly that, whatever has been edited since. */
interface Snapshot {
  body: string;
  values: Record<string, string>;
  files: Record<string, string>;
  hostIds: string[];
  strategy: StrategyForm;
  rollback: boolean;
  stepInfo: StepInfo[];
  runId: string;
  name: string;
}

export interface HostPlan {
  hostId: string;
  label: string;
  steps: PlannedStep[];
  error: string | null;
}

const FLUSH_MS = 80;

class RunbooksStore {
  tab = $state<WorkspaceTab>("runbooks");
  selectedId = $state<Uuid | null>(null);
  text = $state("");
  saved = $state("");
  mode = $state<EditorMode>("builder");
  check = $state<RunbookCheck | null>(null);
  /** The step being edited in the Builder (an index into steps; null none), and whether it is a rollback step. */
  editing = $state<{ index: number; rollback: boolean } | null>(null);

  // What a run needs.
  values = $state<Record<string, string>>({});
  files = $state<Record<string, string>>({});
  chosen = $state<string[]>([]);
  strategy = $state<StrategyForm>(defaultStrategyForm());
  rollbackOn = $state(false);
  connected = $state<Record<string, { ok: boolean; error: string | null }> | null>(null);
  testing = $state(false);

  // Validation and the preview.
  report = $state<ValidationItem[] | null>(null);
  previewOpen = $state(false);
  plans = $state<HostPlan[] | null>(null);
  planning = $state(false);

  // The run.
  phase = $state<RunPhase>("idle");
  live = $state<RunLive | null>(null);
  runId = $state<string | null>(null);
  consoleTab = $state<ConsoleTab>("logs");
  #last: Snapshot | null = null;
  #queue: RunbookEvent[] = [];
  #flush: ReturnType<typeof setTimeout> | undefined;
  #checkTimer: ReturnType<typeof setTimeout> | undefined;

  // History.
  history = $state<RunSummary[]>([]);
  viewing = $state<RunRecord | null>(null);

  // Panels that have no room beside the editor open over it, on request.
  libraryDrawer = $state(false);
  execDrawer = $state(false);

  loaded = $derived(load(this.text));
  where = $derived(positions(this.text));
  dirty = $derived(this.text !== this.saved);
  items = $derived<LibraryItem[]>(buildLibrary(vaultStore.runbooks, settings.prefs.rbFavorites));
  name = $derived(this.check?.name ?? this.loaded.model?.name ?? "");
  stepNames = $derived([...(this.loaded.model?.steps ?? []), ...(this.loaded.model?.rollback ?? [])].map((s, i) => (typeof s.name === "string" && s.name ? s.name : `Step ${i + 1}`)));
  viewLive = $derived(this.viewing ? fromRecord(this.viewing) : null);

  hosts = $derived<PickHost[]>(
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
  chosenHosts = $derived(this.chosen.map((id) => this.hosts.find((h) => h.id === id)).filter((h): h is PickHost => !!h));

  constructor() {
    $effect.root(() => {
      // The backend's check of the text, a moment after typing stops.
      $effect(() => {
        const text = this.text;
        clearTimeout(this.#checkTimer);
        this.#checkTimer = setTimeout(() => void this.#runCheck(text), 200);
      });
      // A variable's default is shown, so what will be used is visible.
      $effect(() => {
        for (const p of this.check?.params ?? []) {
          if ((p.kind === "text" || p.kind === "number" || p.kind === "boolean") && p.default !== null && !(p.name in this.values)) this.values[p.name] = p.default;
        }
      });
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  async #runCheck(text: string) {
    if (text.trim() === "") {
      this.check = null;
      return;
    }
    try {
      const c = await api.runbookCheck(text);
      if (text === this.text) this.check = c;
    } catch (e) {
      if (text === this.text) this.check = { name: null, description: "", params: [], steps: 0, rollback_steps: 0, problems: [{ step: null, message: errorMessage(e) }] };
    }
  }

  /** Check now and wait for it (the Validate action must not report on stale text). */
  async checkNow(): Promise<RunbookCheck | null> {
    clearTimeout(this.#checkTimer);
    await this.#runCheck(this.text);
    return this.check;
  }

  reset() {
    this.selectedId = null;
    this.text = "";
    this.saved = "";
    this.check = null;
    this.values = {};
    this.files = {};
    this.chosen = [];
    this.connected = null;
    this.report = null;
    this.plans = null;
    this.previewOpen = false;
    this.live = null;
    this.viewing = null;
    this.phase = "idle";
    this.runId = null;
    this.#last = null;
    this.#queue = [];
    this.editing = null;
  }

  // -- the library ---------------------------------------------------------------------------------

  async #leave(): Promise<boolean> {
    if (!this.dirty || this.text === "") return true;
    return ask("Discard the changes to this runbook?", { title: "Unsaved changes", confirm: "Discard", danger: true });
  }

  /** Forget what belongs to the runbook that was open: the values typed for it, the plan, the validation. */
  #fresh() {
    this.values = {};
    this.files = {};
    this.connected = null;
    this.report = null;
    this.plans = null;
    this.editing = null;
    this.mode = "builder";
  }

  async select(id: Uuid | null): Promise<boolean> {
    if (id === this.selectedId) return true;
    if (!(await this.#leave())) return false;
    this.selectedId = id;
    const body = id ? (vaultStore.runbooks.find((r) => r.id === id)?.data?.body ?? "") : "";
    this.text = body;
    this.saved = body;
    this.#fresh();
    // The hosts it is usually run on, and how, when it says.
    if (id && this.loaded.model) this.#applyDefaults();
    return true;
  }

  #applyDefaults() {
    const d = this.loaded.model?.defaults as { hosts?: string[]; strategy?: unknown; notify?: string } | undefined;
    if (d?.hosts?.length) this.chosen = d.hosts.filter((h) => this.hosts.some((x) => x.id === h));
    if (d?.strategy) this.strategy = formOf(d.strategy as never);
  }

  async newFrom(body: string): Promise<boolean> {
    if (!(await this.#leave())) return false;
    this.selectedId = null;
    this.text = body;
    this.saved = "";
    this.#fresh();
    this.tab = "runbooks";
    return true;
  }

  newBlank = () => this.newFrom(BLANK);

  async save(): Promise<boolean> {
    const name = this.check?.name?.trim() || this.loaded.model?.name.trim();
    if (!name) {
      ui.notify("error", "Give the runbook a name before saving.");
      return false;
    }
    try {
      const rec = await vaultStore.saveRunbook(this.selectedId, { name, body: this.text });
      this.selectedId = rec.id;
      this.saved = this.text;
      const problems = this.check?.problems.length;
      ui.notify(problems ? "warning" : "info", problems ? "Saved. It has problems, so it can't be run yet." : "Saved.");
      return true;
    } catch (e) {
      ui.notify("error", errorMessage(e));
      return false;
    }
  }

  async rename(id: Uuid) {
    const rec = vaultStore.runbooks.find((r) => r.id === id)?.data;
    if (!rec) return;
    const name = (await askText("Name", rec.name, { title: "Rename runbook", confirm: "Rename" }))?.trim();
    if (!name || name === rec.name) return;
    const edited = setTop(rec.body, "name", name);
    if ("error" in edited) {
      ui.notify("error", edited.error);
      return;
    }
    try {
      await vaultStore.saveRunbook(id, { name, body: edited.text });
      if (this.selectedId === id) {
        // What was typed since stays; only the name changes.
        const same = this.text === this.saved;
        const now = setTop(this.text, "name", name);
        this.saved = edited.text;
        this.text = same ? edited.text : "error" in now ? this.text : now.text;
      }
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async duplicate(id: Uuid) {
    const rec = vaultStore.runbooks.find((r) => r.id === id)?.data;
    if (!rec) return;
    const name = copyName(rec.name, vaultStore.runbooks.filter((r) => !r.deleted && r.data).map((r) => r.data!.name));
    const edited = setTop(rec.body, "name", name);
    const body = "error" in edited ? rec.body : edited.text;
    try {
      const made = await vaultStore.saveRunbook(null, { name, body });
      await this.select(made.id);
      ui.notify("info", `Duplicated as "${name}".`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async remove(id: Uuid) {
    const name = vaultStore.runbooks.find((r) => r.id === id)?.data?.name ?? "this runbook";
    if (!(await ask(`Delete "${name}"? Its schedules stop working. Past runs stay in the history.`, { title: "Delete runbook", confirm: "Delete", danger: true }))) return;
    try {
      await vaultStore.deleteRunbook(id);
      settings.prefs.rbFavorites = settings.prefs.rbFavorites.filter((f) => f !== id);
      if (this.selectedId === id) {
        this.selectedId = null;
        this.text = "";
        this.saved = "";
        this.#fresh();
      }
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async setCategory(id: Uuid, category: string) {
    const rec = vaultStore.runbooks.find((r) => r.id === id)?.data;
    if (!rec) return;
    const edited = setTop(rec.body, "category", category === "Custom" ? undefined : category);
    if ("error" in edited) return void ui.notify("error", edited.error);
    try {
      await vaultStore.saveRunbook(id, { name: rec.name, body: edited.text });
      if (this.selectedId === id) {
        const same = this.text === this.saved;
        this.saved = edited.text;
        if (same) this.text = edited.text;
      }
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  favorite(id: Uuid) {
    settings.prefs.rbFavorites = toggleFavorite(settings.prefs.rbFavorites, id);
  }

  async importFile() {
    const f = await open({ multiple: false, directory: false, title: "Choose a runbook", filters: [{ name: "Runbook (YAML or JSON)", extensions: ["yaml", "yml", "json"] }] });
    if (typeof f !== "string") return;
    try {
      const text = await api.readTextFile(f);
      if (!load(text).model && !(await ask("That file doesn't read as a runbook. Open it anyway, to fix it?", { title: "Not a runbook", confirm: "Open it" }))) return;
      await this.newFrom(text);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async exportFile(id: Uuid | null = this.selectedId) {
    const body = id && id !== this.selectedId ? (vaultStore.runbooks.find((r) => r.id === id)?.data?.body ?? "") : this.text;
    if (!body) return;
    const fmt = formatOf(body);
    const slug = (this.name || "runbook").replace(/[^A-Za-z0-9._-]+/g, "-").toLowerCase();
    const path = await save({ title: "Export this runbook", defaultPath: `${slug}.${fmt === "json" ? "json" : "yaml"}`, filters: [{ name: "YAML", extensions: ["yaml", "yml"] }, { name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    // The file's extension decides the form it is written in.
    const want: Format = path.toLowerCase().endsWith(".json") ? "json" : "yaml";
    const out = convert(body, want);
    if ("error" in out) return void ui.notify("error", out.error);
    try {
      await api.exportTextFile(path, out.text);
      ui.notify("info", "Exported.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- editing -------------------------------------------------------------------------------------

  /** Apply an edit to the text, or say why it couldn't be made. */
  apply(edit: Edit): boolean {
    if ("error" in edit) {
      ui.notify("error", edit.error);
      return false;
    }
    this.text = edit.text;
    return true;
  }

  addStepOf(kind: StepKind, index?: number, rollback = false) {
    const at = index ?? (rollback ? this.loaded.model?.rollback.length : this.loaded.model?.steps.length) ?? 0;
    if (this.apply(addStep(this.text, newStep(kind), { index: at, rollback }))) this.editing = { index: at, rollback };
  }

  removeStepAt(index: number, rollback = false) {
    if (this.apply(removeStep(this.text, index, rollback))) this.editing = null;
  }

  move(index: number, delta: number, rollback = false) {
    const to = index + delta;
    if (this.apply(moveStep(this.text, index, to, rollback)) && this.editing?.index === index && this.editing.rollback === rollback) this.editing = { index: to, rollback };
  }

  duplicateStepAt(index: number, rollback = false) {
    if (this.apply(duplicateStep(this.text, index, rollback))) this.editing = { index: index + 1, rollback };
  }

  toggleStep(index: number, rollback = false) {
    const cur = (rollback ? this.loaded.model?.rollback : this.loaded.model?.steps)?.[index]?.disabled === true;
    this.apply(setStepField(this.text, index, "disabled", cur ? undefined : true, rollback));
  }

  replaceStep(index: number, step: Obj, rollback = false) {
    this.apply(updateStep(this.text, index, step, rollback));
  }

  /** The text written out again, the same document with normal indentation. */
  tidy() {
    this.apply(formatText(this.text));
  }

  async toYaml(to: Format) {
    if (formatOf(this.text) === to) return;
    const out = convert(this.text, to);
    if ("error" in out) return void ui.notify("error", out.error);
    this.text = out.text;
  }

  // -- hosts and values ------------------------------------------------------------------------------

  choose(ids: string[]) {
    const seen = new Set(this.chosen);
    this.chosen = [...this.chosen, ...ids.filter((id) => !seen.has(id) && seen.add(id))];
    this.connected = null;
  }

  unchoose(id: string) {
    this.chosen = this.chosen.filter((x) => x !== id);
    this.connected = null;
  }

  clearChosen() {
    this.chosen = [];
    this.connected = null;
  }

  /** The text values a run is given: everything typed or chosen except files, which go separately. */
  textParams(): Record<string, string> {
    const kinds = new Map((this.check?.params ?? []).map((p) => [p.name, p.kind]));
    return Object.fromEntries(Object.entries($state.snapshot(this.values)).filter(([k, v]) => v !== "" && kinds.get(k) !== "file"));
  }

  async chooseFile(name: string, kind: "file" | "dir") {
    const f = await open({ multiple: false, directory: kind === "dir", title: kind === "dir" ? "Choose the folder" : "Choose the file" });
    if (typeof f !== "string") return;
    if (kind === "dir") this.values[name] = f;
    else this.files[name] = f;
    this.connected = null;
  }

  /** The chosen file or folder for a variable, as shown. */
  chosenPath(name: string, kind: string): string {
    return kind === "file" ? (this.files[name] ?? "") : (this.values[name] ?? "");
  }

  async testConnections() {
    if (!this.chosen.length || this.testing) return;
    this.testing = true;
    try {
      const results = await api.runbookPreflight([...this.chosen]);
      this.connected = Object.fromEntries(results.map((r) => [r.host_id, { ok: r.ok, error: r.error }]));
      this.report = this.#validation();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      this.testing = false;
    }
  }

  // -- validating and previewing -------------------------------------------------------------------------

  #stepInfo(): StepInfo[] {
    return (this.loaded.model?.steps ?? []).map((s, i) => {
      const kind = kindOf(s) ?? "run";
      const body = (typeof s[kind] === "object" && s[kind] ? (s[kind] as Obj) : {}) as Obj;
      const text = kind === "run" ? String(s.run ?? "") : kind === "wait" ? String(body.run ?? "") : kind === "docker" ? `docker ${String(body.action ?? "")} ${String(body.container ?? body.image ?? body.file ?? "")}` : kind === "upload" ? `upload ${String(body.remote ?? "")}` : `${kind} ${JSON.stringify(body)}`;
      return { name: typeof s.name === "string" && s.name ? s.name : `Step ${i + 1}`, kind, text, disabled: s.disabled === true };
    });
  }

  #validation(): ValidationItem[] {
    const present = new Set(this.hosts.map((h) => h.id));
    return validate({
      check: this.check,
      hosts: this.chosenHosts,
      missingHosts: this.chosen.filter((id) => !present.has(id)).length,
      values: this.values,
      files: { ...this.files, ...Object.fromEntries((this.check?.params ?? []).filter((p) => p.kind === "dir").map((p) => [p.name, this.values[p.name] ?? ""])) },
      strategy: this.strategy,
      connected: this.connected,
      usesRemotePaths: this.#stepInfo().some((s) => !s.disabled && ["upload", "download"].includes(s.kind)),
    });
  }

  /** Check the text with the backend and report on everything a run needs. Nothing is run. */
  async validateNow(): Promise<boolean> {
    this.phase = this.phase === "idle" ? "validating" : this.phase;
    await this.checkNow();
    this.report = this.#validation();
    if (this.phase === "validating") this.phase = "idle";
    return canRun(this.report);
  }

  /** The "Preview execution plan" step: the commands each host would get, with secrets as <name>. Nothing is run. */
  async preview() {
    this.planning = true;
    this.plans = null;
    const params = this.textParams();
    const out: HostPlan[] = [];
    try {
      for (const h of this.chosenHosts.slice(0, 20)) {
        try {
          out.push({ hostId: h.id, label: h.label, steps: await api.runbookPlan(this.text, params, h.hostname, h.label), error: null });
        } catch (e) {
          out.push({ hostId: h.id, label: h.label, steps: [], error: errorMessage(e) });
        }
      }
      this.plans = out;
    } finally {
      this.planning = false;
    }
  }

  /** The first step of "Run now": validate, then show the plan to confirm. */
  async runNow() {
    if (!canStart(this.phase)) return;
    if (this.dirty && this.selectedId && !(await this.save())) return;
    const ok = await this.validateNow();
    if (!ok) {
      ui.notify("error", "Fix what the validation reports, then run.");
      return;
    }
    await this.preview();
    this.previewOpen = true;
  }

  /** The steps of the open runbook that match the vault's destructive-command patterns. */
  destructive(): string[] {
    const patterns = compilePatterns(vaultStore.settings?.settings.destructive_patterns ?? []);
    return destructiveSteps(this.#stepInfo(), patterns);
  }

  /** Start the run that was previewed. Production hosts and destructive steps are confirmed first. */
  async confirmRun(): Promise<void> {
    if (!canStart(this.phase)) return;
    const prod = this.chosenHosts.filter((h) => h.production).map((h) => h.label);
    const bad = this.destructive();
    const name = this.name || "this runbook";
    if (prod.length || bad.length) {
      const parts = [`"${name}" will run on ${this.chosen.length} host${this.chosen.length === 1 ? "" : "s"}.`];
      if (prod.length) parts.push(`Production: ${prod.slice(0, 8).join(", ")}${prod.length > 8 ? ` and ${prod.length - 8} more` : ""}.`);
      if (bad.length) parts.push(`Steps that match a destructive pattern: ${bad.join(", ")}.`);
      const yes = await ask(parts.join("\n\n"), { title: prod.length ? "Production hosts" : "Destructive steps", confirm: "Run", danger: true, requireText: prod.length ? name : undefined });
      if (!yes) return;
    }
    this.previewOpen = false;
    await this.#start({ hostIds: [...this.chosen], fromStep: null, retryOf: null });
  }

  async #start(o: { hostIds: string[]; fromStep: number | null; retryOf: string | null; snapshot?: Snapshot }) {
    this.phase = "starting";
    const snap: Snapshot = o.snapshot ?? {
      body: this.text,
      values: $state.snapshot(this.values) as Record<string, string>,
      files: $state.snapshot(this.files) as Record<string, string>,
      hostIds: o.hostIds,
      strategy: $state.snapshot(this.strategy) as StrategyForm,
      rollback: this.rollbackOn && (this.check?.rollback_steps ?? 0) > 0,
      stepInfo: this.#stepInfo(),
      runId: "",
      name: this.name,
    };
    const runId = crypto.randomUUID();
    snap.runId = runId;
    this.#last = snap;
    const stepNames = [...(load(snap.body).model?.steps ?? []), ...(load(snap.body).model?.rollback ?? [])].map((s, i) => (typeof s.name === "string" && s.name ? s.name : `Step ${i + 1}`));
    this.live = startRun(o.hostIds.map((id) => ({ id, label: this.hosts.find((h) => h.id === id)?.label ?? id })), stepNames);
    this.runId = runId;
    this.viewing = null;
    this.consoleTab = "logs";
    settings.prefs.rbOutputOpen = true;
    this.#queue = [];
    const kinds = new Map((this.check?.params ?? []).map((p) => [p.name, p.kind]));
    const params = Object.fromEntries(Object.entries(snap.values).filter(([k, v]) => v !== "" && kinds.get(k) !== "file"));
    try {
      await api.runbookStart(runId, snap.body, params, snap.files, o.hostIds, false, (e) => this.#onEvent(runId, e), snap.rollback, strategyOf(snap.strategy), o.fromStep, o.retryOf);
      if (this.runId === runId && this.phase === "starting") this.phase = "running";
    } catch (e) {
      if (this.runId === runId) {
        this.phase = "failed";
        this.live = null;
      }
      ui.notify("error", errorMessage(e));
    }
  }

  #onEvent(runId: string, e: RunbookEvent) {
    if (this.runId !== runId) return;
    if (this.phase === "starting") this.phase = "running";
    this.#queue.push(e);
    // A burst of output is folded in a batch every fraction of a second, so the window redraws at that pace and not per line.
    if (e.event === "done") return this.#drain();
    this.#flush ??= setTimeout(() => this.#drain(), FLUSH_MS);
  }

  #drain() {
    clearTimeout(this.#flush);
    this.#flush = undefined;
    if (!this.live || this.#queue.length === 0) return;
    const events = this.#queue;
    this.#queue = [];
    this.live = applyRunEvents(this.live, events, Date.now());
    if (this.live.finished) this.#finished();
  }

  #finished() {
    const live = this.live!;
    this.phase = this.phase === "cancelling" ? "cancelled" : phaseAfter(live);
    void this.loadHistory();
    const notify = (this.#last && (load(this.#last.body).model?.defaults as { notify?: string } | undefined)?.notify) ?? "failure";
    const failed = live.hosts.filter((h) => h.state !== "success").length;
    const name = this.#last?.name ?? "The runbook";
    if (notify === "always" || (notify === "failure" && failed > 0)) {
      ui.notify(failed ? "error" : "info", this.phase === "cancelled" ? `"${name}" was cancelled.` : failed ? `"${name}": ${failed} of ${live.hosts.length} host${live.hosts.length === 1 ? "" : "s"} did not finish successfully.` : `"${name}" finished on ${live.hosts.length} host${live.hosts.length === 1 ? "" : "s"}.`);
    }
  }

  /** Stop the run: the backend ends what is running, and the console says which hosts were cancelled. */
  async cancel() {
    if (!this.runId || (this.phase !== "running" && this.phase !== "starting")) return;
    this.phase = "cancelling";
    try {
      await api.runbookCancel(this.runId);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- trying again ------------------------------------------------------------------------------

  /** What a retry would do, for the confirmation: the hosts, where it starts and which steps change things. */
  retryPlan(what: "failed" | "all" | "from-failed-step"): { hostIds: string[]; fromStep: number | null; risky: string[]; label: string } | null {
    if (!this.live || !this.#last) return null;
    const hostIds = retryHosts(this.live.hosts, what === "all" ? "all" : "failed");
    if (hostIds.length === 0) return null;
    const fromStep = what === "from-failed-step" ? firstFailedStep(this.live.hosts) : null;
    if (what === "from-failed-step" && fromStep === null) return null;
    const patterns = compilePatterns(vaultStore.settings?.settings.destructive_patterns ?? []);
    return {
      hostIds,
      fromStep,
      risky: riskySteps(this.#last.stepInfo, fromStep, patterns),
      label: what === "all" ? "Run it again on every host" : what === "failed" ? "Retry the failed hosts" : `Retry the failed hosts from step ${(fromStep ?? 0) + 1}`,
    };
  }

  async retry(what: "failed" | "all" | "from-failed-step") {
    if (!canStart(this.phase) || !this.#last) return;
    const plan = this.retryPlan(what);
    if (!plan) return;
    if (plan.risky.length) {
      const ok = await ask(
        `${plan.label}: ${plan.hostIds.length} host${plan.hostIds.length === 1 ? "" : "s"}.\n\nThese steps change things and are not known to be safe to run twice. On a host where they already ran, they run again:\n\n${plan.risky.join("\n")}`,
        { title: "Run again?", confirm: "Run again", danger: true },
      );
      if (!ok) return;
    }
    const previous = this.#last;
    await this.#start({ hostIds: plan.hostIds, fromStep: plan.fromStep, retryOf: previous.runId, snapshot: { ...previous } });
  }

  // -- history -----------------------------------------------------------------------------------

  async loadHistory() {
    this.history = await api.runbookHistory.list().catch(() => []);
  }

  async openRecord(id: string) {
    this.viewing = await api.runbookHistory.get(id).catch(() => null);
  }

  async deleteRecord(id: string) {
    await api.runbookHistory.delete(id).catch(() => {});
    if (this.viewing?.id === id) this.viewing = null;
    await this.loadHistory();
  }

  async clearHistory() {
    if (!(await ask("Delete the record of every run? It is kept only on this computer.", { title: "Clear the history", confirm: "Delete all", danger: true }))) return;
    await api.runbookHistory.clear().catch(() => {});
    this.viewing = null;
    await this.loadHistory();
  }

  /** Open a past run's runbook and hosts for running again. */
  async runAgainFromRecord(r: RunRecord) {
    const rb = vaultStore.runbooks.find((x) => !x.deleted && x.data?.name === r.runbook);
    if (!rb) return void ui.notify("error", `"${r.runbook}" isn't in the library any more.`);
    if (!(await this.select(rb.id))) return;
    this.chosen = r.hosts.map((h) => h.host_id).filter((id) => this.hosts.some((h) => h.id === id));
    this.tab = "runbooks";
  }

  async pickFormat(): Promise<Format | null> {
    const v = await choose("Which form?", [{ value: "yaml", label: "YAML" }, { value: "json", label: "JSON" }]);
    return v === "yaml" || v === "json" ? v : null;
  }
}

export const runbooks = new RunbooksStore();
