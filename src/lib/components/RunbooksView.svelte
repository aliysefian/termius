<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { CircleStop, Download, FilePlus2, FolderOpen, ListChecks, Loader2, Play, Plus, Save, Trash2, Upload } from "lucide-svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import RunResults from "./RunResults.svelte";
  import SchedulesPanel from "./SchedulesPanel.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { EXAMPLES, applyEvent, duration, type HostRun, type PlannedStep, type RunRecord, type RunSummary, type RunbookCheck } from "$lib/runbook";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  type Tab = "runbooks" | "history" | "schedules";
  let tab = $state<Tab>("runbooks");

  // ---- the list and the editor ----
  let selected = $state<Uuid | null>(null);
  let body = $state("");
  let checked = $state<RunbookCheck | null>(null);
  let saved = $state("");
  const list = $derived(vaultStore.runbooks.filter((r) => !r.deleted && r.data).sort((a, b) => a.data!.name.localeCompare(b.data!.name)));
  const dirty = $derived(body !== saved);

  let checkTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const text = body;
    clearTimeout(checkTimer);
    checkTimer = setTimeout(async () => {
      try {
        checked = await api.runbookCheck(text);
      } catch (e) {
        checked = { name: null, description: "", params: [], steps: 0, rollback_steps: 0, problems: [{ step: null, message: errorMessage(e) }] };
      }
    }, 200);
  });
  onDestroy(() => clearTimeout(checkTimer));

  async function pick(id: Uuid | null) {
    if (dirty && !(await ask("Discard the changes to this runbook?", { title: "Unsaved changes", confirm: "Discard", danger: true }))) return;
    selected = id;
    body = id ? (vaultStore.runbooks.find((r) => r.id === id)?.data?.body ?? "") : "";
    saved = body;
    reset();
  }

  async function newFrom(text: string) {
    if (dirty && !(await ask("Discard the changes to this runbook?", { title: "Unsaved changes", confirm: "Discard", danger: true }))) return;
    selected = null;
    body = text;
    saved = "";
    reset();
  }

  async function saveIt() {
    const name = checked?.name?.trim();
    if (!name) return ui.notify("error", "Give the runbook a name (the \"name\" in the document) before saving.");
    try {
      const rec = await vaultStore.saveRunbook(selected, { name, body });
      selected = rec.id;
      saved = body;
      ui.notify("info", checked?.problems.length ? "Saved. It has problems, so it can't be run yet." : "Saved.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function remove() {
    if (!selected) return;
    const name = vaultStore.runbooks.find((r) => r.id === selected)?.data?.name ?? "this runbook";
    if (!(await ask(`Delete "${name}"?`, { title: "Delete runbook", confirm: "Delete", danger: true }))) return;
    await vaultStore.deleteRunbook(selected).catch((e) => ui.notify("error", errorMessage(e)));
    selected = null;
    body = "";
    saved = "";
  }

  async function importFile() {
    const f = await open({ multiple: false, directory: false, title: "Choose a runbook", filters: [{ name: "Runbook (JSON)", extensions: ["json"] }] });
    if (typeof f !== "string") return;
    try {
      await newFrom(await api.readTextFile(f));
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function exportFile() {
    const name = (checked?.name ?? "runbook").replace(/[^A-Za-z0-9._-]+/g, "-").toLowerCase();
    const path = await save({ title: "Export this runbook", defaultPath: `${name}.json`, filters: [{ name: "Runbook (JSON)", extensions: ["json"] }] });
    if (!path) return;
    try {
      await api.exportTextFile(path, body);
      ui.notify("info", "Exported.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // ---- running ----
  let params = $state<Record<string, string>>({});
  let files = $state<Record<string, string>>({});
  let picked = $state<Set<Uuid>>(new Set());
  let filter = $state("");
  let plan = $state<PlannedStep[] | null>(null);
  let planError = $state<string | null>(null);
  let runId = $state<string | null>(null);
  let running = $state(false);
  let live = $state<HostRun[]>([]);
  let liveSteps = $state<{ name: string; phase?: string }[]>([]);
  /** Run the runbook's rollback steps on a host where the run fails. Asked for each run, never remembered. */
  let rollbackOn = $state(false);
  let hostOrder = $state<"parallel" | "stop" | "all">("parallel");

  const hosts = $derived(
    vaultStore.hosts
      .filter((h) => h.data && !h.deleted && (!h.data.protocol || h.data.protocol === "ssh"))
      .filter((h) => {
        const q = filter.trim().toLowerCase();
        return !q || [h.data!.label, h.data!.hostname, h.data!.group, ...h.data!.tags].some((s) => s.toLowerCase().includes(q));
      }),
  );
  const unattended = (id: Uuid) => !!vaultStore.effectiveIdentity(vaultStore.hostById.get(id)?.data);
  const valid = $derived(!!checked && checked.problems.length === 0 && checked.steps > 0);

  function reset() {
    params = {};
    files = {};
    plan = null;
    planError = null;
  }

  // A parameter gets its default shown, so what will be used is visible.
  $effect(() => {
    for (const p of checked?.params ?? []) if (p.kind === "text" && p.default !== null && !(p.name in params)) params[p.name] = p.default;
  });

  const textParams = () => Object.fromEntries(Object.entries($state.snapshot(params)).filter(([, v]) => v !== ""));

  const toggle = (id: Uuid) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    picked = next;
  };

  const groupsOf = $derived([...new Set(hosts.map((h) => h.data!.group).filter(Boolean))].sort());
  function pickGroup(g: string) {
    if (!g) return;
    picked = new Set([...picked, ...hosts.filter((h) => h.data!.group === g || h.data!.group.startsWith(`${g}/`)).map((h) => h.id)]);
  }

  async function chooseFile(name: string) {
    const f = await open({ multiple: false, directory: false, title: `Choose the file for ${name}` });
    if (typeof f === "string") files[name] = f;
  }

  async function dryRun() {
    plan = null;
    planError = null;
    const first = [...picked].map((id) => vaultStore.hostById.get(id)?.data).find(Boolean) ?? { hostname: "host.example", label: "host" };
    try {
      plan = await api.runbookPlan(body, textParams(), first.hostname, first.label);
    } catch (e) {
      planError = errorMessage(e);
    }
  }

  async function run(scheduled = false) {
    const ids = [...picked];
    if (!valid || ids.length === 0) return;
    const prod = ids.map((id) => vaultStore.hostById.get(id)?.data).filter((d) => d && vaultStore.effectiveEnv(d) === "production").map((d) => d!.label);
    const name = checked?.name ?? "this runbook";
    if (prod.length) {
      const text = `"${name}" will run its ${checked?.steps} steps on ${prod.length} production host${prod.length === 1 ? "" : "s"}:\n\n${prod.join("\n")}${rollbackOn && checked?.rollback_steps ? `\n\nOn a host where it fails, its ${checked.rollback_steps} rollback step${checked.rollback_steps === 1 ? "" : "s"} will run too.` : ""}`;
      if (!(await ask(text, { title: "Production hosts", confirm: "Run", danger: true, requireText: name }))) return;
    }
    const id = crypto.randomUUID();
    runId = id;
    running = true;
    plan = null;
    liveSteps = (await api.runbookPlan(body, textParams(), "h", "h").catch(() => [])).map((s) => ({ name: s.name, phase: s.phase }));
    live = ids.map((h) => ({ hostId: h, label: vaultStore.hostById.get(h)?.data?.label ?? h, state: "waiting", current: null, steps: [], error: null }));
    try {
      await api.runbookStart(id, body, textParams(), $state.snapshot(files), ids, scheduled, (e) => {
        if (runId !== id) return;
        live = applyEvent(live, e);
        if (e.event === "done") running = false;
      }, rollbackOn && (checked?.rollback_steps ?? 0) > 0, hostOrder === "parallel" ? "parallel" : { sequential: { stop_on_failure: hostOrder === "stop" } });
    } catch (e) {
      running = false;
      ui.notify("error", errorMessage(e));
    }
  }

  async function cancel() {
    if (!runId) return;
    await api.runbookCancel(runId);
    running = false;
  }

  const finished = $derived(live.length > 0 && !running);
  const failedHosts = $derived(live.filter((h) => h.state === "failed").length);

  // ---- history ----
  let history = $state<RunSummary[]>([]);
  let record = $state<RunRecord | null>(null);
  async function loadHistory() {
    history = await api.runbookHistory.list().catch(() => []);
  }
  async function showRecord(id: string) {
    record = await api.runbookHistory.get(id).catch(() => null);
  }
  const recordHosts = $derived<HostRun[]>(
    (record?.hosts ?? []).map((h) => ({ hostId: h.host_id, label: h.label, state: h.ok === null ? "running" : h.ok ? "ok" : "failed", current: null, steps: h.steps, error: h.error })),
  );
  const stamp = (t: number) => new Date(t * 1000).toLocaleString();
  async function deleteRecord(id: string) {
    await api.runbookHistory.delete(id);
    if (record?.id === id) record = null;
    await loadHistory();
  }
  async function clearHistory() {
    if (!(await ask("Delete the record of every run? It is kept only on this computer.", { title: "Clear the history", confirm: "Delete all", danger: true }))) return;
    await api.runbookHistory.clear();
    record = null;
    await loadHistory();
  }
  $effect(() => {
    if (tab === "history") void loadHistory();
  });
  // A run that ends is in the record.
  $effect(() => {
    if (finished && tab === "history") void loadHistory();
  });

  onMount(() => {
    if (list.length && !selected) void pick(list[0].id);
  });
</script>

<div class="flex h-full min-h-0 flex-col" data-testid="runbooks">
  <header class="flex items-center gap-3 border-b border-line px-5 py-3">
    <ListChecks size={18} class="text-accent" />
    <h2 class="text-base font-semibold">Runbooks</h2>
    <div class="ml-4 flex gap-1" role="tablist" aria-label="Runbooks">
      {#each [["runbooks", "Runbooks"], ["history", "History"], ["schedules", "Schedules"]] as [id, label] (id)}
        <button role="tab" aria-selected={tab === id} class="rounded-md px-3 py-1 text-sm {tab === id ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:text-fg'}" onclick={() => (tab = id as Tab)}>{label}</button>
      {/each}
    </div>
  </header>

  {#if tab === "runbooks"}
    <div class="flex min-h-0 flex-1">
      <aside class="w-60 shrink-0 space-y-2 overflow-auto border-r border-line p-3">
        <div class="flex gap-1">
          <button class="btn-secondary flex-1 py-1 text-xs" onclick={() => newFrom(EXAMPLES[0].body)}><Plus size={13} /> New</button>
          <button class="btn-ghost py-1 text-xs" title="Open a runbook file" onclick={importFile}><FolderOpen size={13} /></button>
        </div>
        <select class="input py-1 text-xs" aria-label="Start from an example" onchange={(e) => { const i = Number(e.currentTarget.value); if (!Number.isNaN(i) && e.currentTarget.value !== "") void newFrom(EXAMPLES[i].body); e.currentTarget.value = ""; }}>
          <option value="">Start from an example…</option>
          {#each EXAMPLES as ex, i (ex.label)}<option value={i}>{ex.label}</option>{/each}
        </select>
        <ul class="space-y-0.5">
          {#each list as r (r.id)}
            <li><button class="w-full truncate rounded px-2 py-1.5 text-left text-sm {selected === r.id ? 'bg-accent/15 text-accent' : 'hover:bg-hover'}" onclick={() => pick(r.id)}>{r.data!.name}</button></li>
          {/each}
        </ul>
        {#if !list.length}<p class="text-xs text-fg-muted">No saved runbooks yet. Start from an example, edit it and save.</p>{/if}
      </aside>

      <div class="min-w-0 flex-1 overflow-auto p-5">
        <div class="grid gap-5 xl:grid-cols-2">
          <section aria-label="Runbook document" class="min-w-0 space-y-2">
            <div class="flex items-center gap-2">
              <h3 class="min-w-0 flex-1 truncate text-sm font-semibold">{checked?.name ?? "Untitled"}{#if dirty}<span class="ml-1 text-xs font-normal text-fg-muted">(unsaved)</span>{/if}</h3>
              <button class="btn-primary py-1 text-xs" disabled={!dirty || !checked?.name} onclick={saveIt}><Save size={13} /> Save</button>
              <button class="btn-ghost py-1 text-xs" title="Export as a file" disabled={!body} onclick={exportFile}><Upload size={13} /></button>
              <button class="btn-ghost py-1 text-xs" title="Delete" disabled={!selected} onclick={remove}><Trash2 size={13} /></button>
            </div>
            <label class="sr-only" for="rb-body">Runbook document (JSON)</label>
            <textarea id="rb-body" class="input h-96 w-full font-mono text-xs leading-5" spellcheck="false" bind:value={body} placeholder={"Choose a runbook, or start from an example."}></textarea>
            {#if checked && checked.problems.length}
              <ul class="space-y-1 rounded-md border border-danger/30 bg-danger/10 p-2 text-xs text-danger" role="alert" data-testid="runbook-problems">
                {#each checked.problems as p (p.message + p.step)}<li>{p.step !== null ? `Step ${p.step + 1}: ` : ""}{p.message}</li>{/each}
              </ul>
            {:else if checked?.name}
              <p class="text-xs text-success" data-testid="runbook-ok">Valid: {checked.steps} step{checked.steps === 1 ? "" : "s"}{checked.params.length ? `, ${checked.params.length} parameter${checked.params.length === 1 ? "" : "s"}` : ""}.</p>
            {/if}
            <p class="text-xs text-fg-muted">Steps are <code>run</code> (a command), <code>wait</code> (repeat a command until it succeeds) or <code>upload</code> (a file you choose when it runs). <code>{"{{name}}"}</code> puts a parameter in; <code>{"{{name|q}}"}</code> quotes it for the shell. A step can run <code>when</code> an earlier step exited a certain way, and carries on after a failure with <code>"on_error": "continue"</code>.</p>
          </section>

          <section aria-label="Run" class="min-w-0 space-y-3">
            <h3 class="text-sm font-semibold">Run it</h3>
            {#if checked?.params.length}
              <div class="space-y-2" data-testid="runbook-params">
                {#each checked.params as p (p.name)}
                  <div>
                    <label class="label" for="rp-{p.name}">{p.label || p.name}{#if p.optional}<span class="font-normal text-fg-muted"> (optional)</span>{/if}</label>
                    {#if p.kind === "file"}
                      <div class="flex items-center gap-2">
                        <button id="rp-{p.name}" class="btn-secondary py-1 text-xs" onclick={() => chooseFile(p.name)}>Choose a file…</button>
                        <span class="min-w-0 flex-1 truncate font-mono text-xs text-fg-muted">{files[p.name] ?? "none chosen"}</span>
                      </div>
                    {:else if p.kind === "secret"}
                      <input id="rp-{p.name}" type="password" class="input font-mono text-xs" bind:value={params[p.name]} spellcheck="false" autocomplete="off" data-testid="runbook-secret" />
                      <p class="mt-0.5 text-[11px] text-fg-muted">Asked each time. Not saved in a schedule, and hidden in the record of the run.</p>
                    {:else if p.choices.length}
                      <select id="rp-{p.name}" class="input" bind:value={params[p.name]}>{#each p.choices as c (c)}<option value={c}>{c}</option>{/each}</select>
                    {:else}
                      <input id="rp-{p.name}" class="input font-mono text-xs" bind:value={params[p.name]} spellcheck="false" autocomplete="off" />
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}

            <div>
              <div class="mb-1 flex items-center gap-2">
                <span class="label mb-0">Hosts ({picked.size} chosen)</span>
                <select class="input ml-auto w-auto py-0.5 text-xs" aria-label="Add a whole group" onchange={(e) => { pickGroup(e.currentTarget.value); e.currentTarget.value = ""; }}>
                  <option value="">Add a group…</option>
                  {#each groupsOf as g (g)}<option value={g}>{g}</option>{/each}
                </select>
                {#if picked.size}<button class="text-xs text-accent hover:underline" onclick={() => (picked = new Set())}>Clear</button>{/if}
              </div>
              <input class="input mb-1 py-1 text-xs" placeholder="Filter hosts" aria-label="Filter hosts" bind:value={filter} />
              <ul class="max-h-48 overflow-auto rounded-md border border-line p-1" data-testid="runbook-hosts">
                {#each hosts as h (h.id)}
                  <li>
                    <label class="flex items-center gap-2 rounded px-2 py-0.5 text-sm hover:bg-hover">
                      <input type="checkbox" class="accent-input" checked={picked.has(h.id)} onchange={() => toggle(h.id)} />
                      <span class="min-w-0 flex-1 truncate">{h.data!.label}</span>
                      {#if vaultStore.effectiveEnv(h.data) === "production"}<span class="rounded bg-danger/15 px-1.5 text-[10px] text-danger">production</span>{/if}
                      {#if !unattended(h.id)}<span class="text-[10px] text-warning" title="No saved credentials, so it can't run unattended">needs sign-in</span>{/if}
                    </label>
                  </li>
                {/each}
              </ul>
            </div>

            {#if checked && checked.rollback_steps > 0}
              <label class="flex items-start gap-2 rounded-md border border-line bg-base/40 p-2.5 text-xs" data-testid="runbook-rollback">
                <input type="checkbox" class="mt-0.5 accent-input" bind:checked={rollbackOn} disabled={running} />
                <span><span class="font-medium">If a host fails, run the rollback steps on it</span>
                  <span class="block text-fg-muted">{checked.rollback_steps} step{checked.rollback_steps === 1 ? "" : "s"} written to undo the run. They change that host again, so look at them in the dry run first. A host that got through is left alone, and a failed host still counts as failed.</span></span>
              </label>
            {/if}

            <label class="flex items-center gap-2 text-xs" data-testid="runbook-order">
              <span class="font-medium">Hosts</span>
              <select class="input w-auto py-1 text-xs" bind:value={hostOrder} disabled={running}>
                <option value="parallel">All at once</option>
                <option value="stop">One at a time, stop at the first failure</option>
                <option value="all">One at a time, carry on after a failure</option>
              </select>
            </label>

            <div class="flex items-center gap-2">
              <button class="btn-secondary" disabled={!valid} onclick={dryRun} data-testid="runbook-dry">Dry run</button>
              {#if running}
                <button class="btn-secondary" onclick={cancel}><CircleStop size={14} /> Stop</button>
                <span class="flex items-center gap-1 text-xs text-fg-muted"><Loader2 size={13} class="animate-spin" /> Running</span>
              {:else}
                <button class="btn-primary" disabled={!valid || !picked.size} onclick={() => run()} data-testid="runbook-run"><Play size={14} /> Run on {picked.size || "…"} host{picked.size === 1 ? "" : "s"}</button>
              {/if}
            </div>

            {#if planError}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{planError}</p>{/if}
            {#if plan}
              <div class="rounded-md border border-line p-3" data-testid="runbook-plan">
                <div class="mb-1 text-xs font-medium text-fg-muted">What would run (nothing was)</div>
                <ol class="space-y-1.5 text-xs">
                  {#each plan as s (s.index)}
                    <li>
                      <span class="font-medium">{s.index + 1}. {s.name}</span>
                      {#if s.phase === "rollback"}<span class="ml-1 rounded bg-warning/15 px-1.5 text-[10px] text-warning">rollback</span>{/if}
                      {#if s.condition}<span class="text-fg-muted"> · {s.condition}</span>{/if}
                      {#if s.on_error === "continue"}<span class="text-fg-muted"> · carries on if it fails</span>{/if}
                      <code class="mt-0.5 block break-all rounded bg-base px-2 py-1 font-mono text-[11px]">{s.text}</code>
                    </li>
                  {/each}
                </ol>
              </div>
            {/if}

            {#if live.length}
              <div class="space-y-2" data-testid="runbook-live">
                {#if finished}<p class="text-xs {failedHosts ? 'text-danger' : 'text-success'}" role="status">{failedHosts ? `${failedHosts} of ${live.length} hosts failed.` : `Done on ${live.length} host${live.length === 1 ? "" : "s"}.`}</p>{/if}
                <RunResults hosts={live} steps={liveSteps} />
              </div>
            {/if}
          </section>
        </div>
      </div>
    </div>
  {:else if tab === "history"}
    <div class="flex min-h-0 flex-1">
      <aside class="w-80 shrink-0 space-y-1 overflow-auto border-r border-line p-3" data-testid="runbook-history">
        <div class="mb-2 flex items-center justify-between text-xs text-fg-muted">
          <span>Kept on this computer only</span>
          {#if history.length}<button class="text-danger hover:underline" onclick={clearHistory}>Clear</button>{/if}
        </div>
        {#each history as r (r.id)}
          <div class="flex items-center gap-1">
            <button class="min-w-0 flex-1 rounded px-2 py-1.5 text-left text-sm {record?.id === r.id ? 'bg-accent/15' : 'hover:bg-hover'}" onclick={() => showRecord(r.id)}>
              <span class="block truncate font-medium">{r.runbook}</span>
              <span class="block truncate text-xs text-fg-muted">{stamp(r.started_at)}{r.scheduled ? " · scheduled" : ""}{r.cancelled ? " · stopped" : ""}{r.running ? " · running" : ""}</span>
              <span class="block text-xs {r.failed ? 'text-danger' : 'text-success'}">{r.ok} ok{r.failed ? `, ${r.failed} failed` : ""} of {r.hosts}</span>
            </button>
            <button class="btn-ghost p-1" aria-label="Delete this record" onclick={() => deleteRecord(r.id)}><Trash2 size={12} /></button>
          </div>
        {/each}
        {#if !history.length}<p class="text-xs text-fg-muted">Nothing has run yet.</p>{/if}
      </aside>
      <div class="min-w-0 flex-1 overflow-auto p-5">
        {#if record}
          <h3 class="text-sm font-semibold">{record.runbook}</h3>
          <p class="mb-3 text-xs text-fg-muted">{stamp(record.started_at)}{record.finished_at ? ` · took ${duration((record.finished_at - record.started_at) * 1000)}` : ""}{record.cancelled ? " · stopped before it finished" : ""}</p>
          {#if Object.keys(record.params).length}
            <p class="mb-3 text-xs text-fg-muted">Values: {Object.entries(record.params).map(([k, v]) => `${k} = ${v}`).join(", ")}</p>
          {/if}
          <RunResults hosts={recordHosts} steps={record.hosts[0]?.steps ?? []} />
        {:else}
          <p class="text-sm text-fg-muted">Choose a run to see what each host printed. Outputs can contain secrets, so this record never leaves this computer.</p>
        {/if}
      </div>
    </div>
  {:else}
    <div class="min-h-0 flex-1 overflow-auto p-5">
      <SchedulesPanel />
    </div>
  {/if}
</div>
