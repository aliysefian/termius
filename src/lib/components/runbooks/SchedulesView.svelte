<script lang="ts">
  import { CalendarClock, Pencil, Play, Plus, Trash2 } from "lucide-svelte";
  import HostPicker from "./HostPicker.svelte";
  import StrategyPicker from "./StrategyPicker.svelte";
  import * as api from "$lib/api";
  import type { RunbookCheck } from "$lib/runbook";
  import { ask } from "$lib/dialogs.svelte";
  import { DAYS, describe, MAX_RETRIES, MIN_EVERY, nextRun, problem, scheduleProblem, validZone, type Schedule, type When } from "$lib/schedule";
  import { defaultStrategyForm, formOf, strategyOf, strategyProblem, type StrategyForm } from "$lib/runbookrun";
  import { blocker, runSchedule } from "$lib/stores/scheduler.svelte";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const runbooks = $derived(vaultStore.runbooks.filter((r) => !r.deleted && r.data));

  let editing = $state<string | "new" | null>(null);
  let name = $state("");
  let runbookId = $state("");
  let check = $state<RunbookCheck | null>(null);
  let params = $state<Record<string, string>>({});
  let chosen = $state<string[]>([]);
  let kind = $state<When["kind"]>("daily");
  let minutes = $state(60);
  let at = $state("02:00");
  let days = $state<number[]>([1]);
  let onceAt = $state("");
  let tz = $state("");
  let missed = $state<"skip" | "run_once">("skip");
  let retries = $state(0);
  let retryMinutes = $state(15);
  let rollback = $state(false);
  let allowProduction = $state(false);
  let notify = $state<"failure" | "always" | "never">("failure");
  let strategy = $state<StrategyForm>(defaultStrategyForm());
  let error = $state<string | null>(null);
  const zones = typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];

  const when = $derived<When>(kind === "every" ? { kind, minutes } : kind === "once" ? { kind, at: onceAt ? new Date(onceAt).getTime() : NaN } : kind === "daily" ? { kind, at } : { kind, days, at });
  const zoneName = $derived(tz.trim() || Intl.DateTimeFormat().resolvedOptions().timeZone);

  const PRESETS: { label: string; apply: () => void }[] = [
    { label: "Every day at 02:00", apply: () => ((kind = "daily"), (at = "02:00")) },
    { label: "Every Monday at 06:00", apply: () => ((kind = "weekly"), (days = [1]), (at = "06:00")) },
    { label: "Weekdays at 08:00", apply: () => ((kind = "weekly"), (days = [1, 2, 3, 4, 5]), (at = "08:00")) },
    { label: "Every 15 minutes", apply: () => ((kind = "every"), (minutes = 15)) },
    { label: "Every hour", apply: () => ((kind = "every"), (minutes = 60)) },
  ];

  async function chooseRunbook(id: string) {
    runbookId = id;
    params = {};
    check = null;
    const r = runbooks.find((x) => x.id === id)?.data;
    if (!r) return;
    check = await api.runbookCheck(r.body).catch(() => null);
    for (const p of check?.params ?? []) if ((p.kind === "text" || p.kind === "number" || p.kind === "boolean") && p.default !== null) params[p.name] = p.default;
    const d = check?.defaults;
    if (d?.strategy) strategy = formOf(d.strategy);
    if (d?.hosts?.length && chosen.length === 0) chosen = d.hosts.filter((h) => vaultStore.hostById.has(h));
  }

  function openNew() {
    editing = "new";
    name = "";
    runbookId = "";
    check = null;
    params = {};
    chosen = [];
    kind = "daily";
    at = "02:00";
    days = [1];
    minutes = 60;
    onceAt = "";
    tz = "";
    missed = "skip";
    retries = 0;
    retryMinutes = 15;
    rollback = false;
    allowProduction = false;
    notify = "failure";
    strategy = defaultStrategyForm();
    error = null;
  }

  async function openEdit(s: Schedule) {
    editing = s.id;
    name = s.name ?? "";
    await chooseRunbook(s.runbookId);
    params = { ...s.params };
    chosen = [...s.hostIds];
    kind = s.when.kind;
    if (s.when.kind === "every") minutes = s.when.minutes;
    else if (s.when.kind === "once") onceAt = new Date(s.when.at - new Date(s.when.at).getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
    else {
      at = s.when.at;
      if (s.when.kind === "weekly") days = [...s.when.days];
    }
    tz = s.tz ?? "";
    missed = s.missed ?? "skip";
    retries = s.retries ?? 0;
    retryMinutes = s.retryMinutes ?? 15;
    rollback = s.rollback === true;
    allowProduction = s.allowProduction;
    notify = s.notify ?? "failure";
    strategy = formOf(s.strategy ?? null);
    error = null;
  }

  async function save() {
    error = null;
    const rbk = runbooks.find((r) => r.id === runbookId)?.data;
    if (!rbk || !check) return (error = "Choose a runbook.");
    if (check.problems.length) return (error = "That runbook has problems; fix them first.");
    if (check.params.some((p) => p.kind === "secret" && !p.optional)) return (error = "That runbook asks for a secret each time it runs, which a schedule can't give (it never keeps one).");
    if (check.params.some((p) => (p.kind === "file" || p.kind === "dir") && !p.optional)) return (error = "That runbook needs a file or folder chosen each time it runs, which a schedule can't give.");
    if (chosen.length === 0) return (error = "Choose at least one host.");
    const bad = problem(when) ?? (tz.trim() && !validZone(tz.trim()) ? `"${tz.trim()}" is not a time zone this computer knows.` : null) ?? strategyProblem(strategy, chosen.length);
    if (bad) return (error = bad);
    if (when.kind === "once" && when.at <= Date.now()) return (error = "Choose a time in the future.");
    const prod = chosen.filter((h) => vaultStore.effectiveEnv(vaultStore.hostById.get(h)?.data) === "production");
    if (prod.length && allowProduction && !(await ask(`This schedule will run "${rbk.name}" by itself on ${prod.length} production host${prod.length === 1 ? "" : "s"}, without anyone watching.`, { title: "Production hosts", confirm: "Allow it", danger: true, requireText: "production" }))) return;
    const existing = editing && editing !== "new" ? settings.prefs.schedules.find((s) => s.id === editing) : undefined;
    const s: Schedule = {
      id: existing?.id ?? crypto.randomUUID(),
      ...(name.trim() ? { name: name.trim() } : {}),
      runbookId,
      params: Object.fromEntries(Object.entries($state.snapshot(params)).filter(([, v]) => v !== "")),
      hostIds: [...chosen],
      when: $state.snapshot(when) as When,
      enabled: existing?.enabled ?? true,
      allowProduction: allowProduction && prod.length > 0,
      lastRun: existing?.lastRun ?? Date.now(),
      ...(tz.trim() && when.kind !== "every" && when.kind !== "once" ? { tz: tz.trim() } : {}),
      ...(missed === "run_once" && when.kind !== "every" ? { missed } : {}),
      ...(retries > 0 ? { retries, retryMinutes } : {}),
      ...(rollback && (check.rollback_steps ?? 0) > 0 ? { rollback: true } : {}),
      ...(strategy.kind !== "parallel" ? { strategy: strategyOf(strategy) } : {}),
      ...(notify !== "failure" ? { notify } : {}),
    };
    const slip = scheduleProblem(s);
    if (slip) return (error = slip);
    settings.prefs.schedules = existing ? settings.prefs.schedules.map((x) => (x.id === s.id ? s : x)) : [...settings.prefs.schedules, s];
    editing = null;
  }

  const remove = async (s: Schedule) => {
    if (await ask("Delete this schedule?", { title: "Delete schedule", confirm: "Delete", danger: true })) settings.prefs.schedules = settings.prefs.schedules.filter((x) => x.id !== s.id);
  };
  const runbookName = (s: Schedule) => runbooks.find((r) => r.id === s.runbookId)?.data?.name ?? "(deleted runbook)";
  const stamp = (t: number) => new Date(t).toLocaleString();
  const toggleDay = (d: number) => (days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d].sort());
</script>

<div class="mx-auto flex h-full max-w-4xl min-h-0 flex-col gap-4 overflow-y-auto p-5" data-testid="schedules">
  <div class="flex items-start gap-3">
    <CalendarClock size={18} class="mt-0.5 text-accent" />
    <div class="min-w-0 flex-1">
      <h2 class="text-[15px] font-semibold">Schedules</h2>
      <p class="mt-1 rounded-md border border-line bg-base/40 p-3 text-xs text-fg-muted">
        Schedules run <strong class="text-fg">only while SSHVault is open</strong>, with the vault unlocked. There is no background service: a time that passes while the app is closed (or the computer sleeps for more than ten minutes) is skipped, unless the schedule says to run once when it opens. A locked vault has no credentials, so nothing runs then. Schedules are kept on this computer and not synced. A run that includes a production host is skipped unless you allowed it.
      </p>
    </div>
    <button class="btn-primary shrink-0 text-xs" onclick={openNew} data-testid="schedule-new"><Plus size={13} /> New schedule</button>
  </div>

  {#if editing}
    <div class="space-y-4 rounded-xl border border-line bg-panel p-4" data-testid="schedule-form">
      <h3 class="text-sm font-semibold">{editing === "new" ? "New schedule" : "Edit schedule"}</h3>
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="block text-xs"><span class="mb-1 block font-medium">Name <span class="font-normal text-fg-muted">(optional)</span></span><input class="input h-8 w-full text-sm" bind:value={name} placeholder="Nightly cleanup" maxlength="80" /></label>
        <label class="block text-xs"><span class="mb-1 block font-medium">Runbook</span>
          <select class="input h-8 w-full text-sm" value={runbookId} onchange={(e) => chooseRunbook(e.currentTarget.value)} aria-label="Runbook">
            <option value="">Choose…</option>{#each runbooks as r (r.id)}<option value={r.id}>{r.data!.name}</option>{/each}
          </select>
        </label>
      </div>

      {#each (check?.params ?? []).filter((p) => p.kind === "text" || p.kind === "number" || p.kind === "boolean") as p (p.name)}
        <label class="block text-xs"><span class="mb-1 block font-medium">{p.label || p.name}</span>
          {#if p.choices.length}<select class="input h-8 w-full text-sm" bind:value={params[p.name]}>{#each p.choices as c (c)}<option value={c}>{c}</option>{/each}</select>
          {:else}<input class="input h-8 w-full font-mono text-xs" bind:value={params[p.name]} />{/if}
        </label>
      {/each}

      <div class="grid gap-5 lg:grid-cols-2">
        <div><h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">Target hosts</h4><HostPicker hosts={rb.hosts} {chosen} onchange={(ids) => (chosen = ids)} /></div>
        <div><h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">Execution strategy</h4><StrategyPicker bind:form={strategy} hosts={chosen.length} /></div>
      </div>

      <div class="space-y-3 border-t border-line pt-4">
        <h4 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">When</h4>
        <div class="flex flex-wrap gap-1.5" role="group" aria-label="Examples">
          {#each PRESETS as p (p.label)}<button class="rounded-md border border-line px-2 py-0.5 text-[11px] text-fg-muted hover:bg-panel-hover hover:text-fg" onclick={p.apply}>{p.label}</button>{/each}
        </div>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <label class="block text-xs"><span class="mb-1 block font-medium">Repeat</span>
            <select class="input h-8 w-full text-sm" bind:value={kind}><option value="every">Every so often</option><option value="daily">Every day</option><option value="weekly">On some days</option><option value="once">Once</option></select>
          </label>
          {#if kind === "once"}
            <label class="col-span-2 block text-xs"><span class="mb-1 block font-medium">Date and time ({zoneName})</span><input class="input h-8 w-full text-sm" type="datetime-local" bind:value={onceAt} /></label>
          {:else if kind === "every"}
            <label class="block text-xs"><span class="mb-1 block font-medium">Minutes (at least {MIN_EVERY})</span><input class="input h-8 w-full text-sm" type="number" min={MIN_EVERY} bind:value={minutes} /></label>
          {:else}
            <label class="block text-xs"><span class="mb-1 block font-medium">At</span><input class="input h-8 w-full font-mono text-sm" bind:value={at} placeholder="02:00" /></label>
          {/if}
        </div>
        {#if kind === "weekly"}
          <div class="flex gap-1" role="group" aria-label="Days">
            {#each DAYS as d, i (d)}<button type="button" class="rounded-md border px-2.5 py-1 text-xs {days.includes(i) ? 'border-accent bg-accent/15 text-accent' : 'border-line text-fg-muted'}" aria-pressed={days.includes(i)} onclick={() => toggleDay(i)}>{d}</button>{/each}
          </div>
        {/if}
        {#if kind === "daily" || kind === "weekly"}
          <label class="block text-xs"><span class="mb-1 block font-medium">Time zone <span class="font-normal text-fg-muted">(empty: this computer's, {Intl.DateTimeFormat().resolvedOptions().timeZone})</span></span>
            <input class="input h-8 w-72 font-mono text-xs" list="sc-zones" bind:value={tz} placeholder="Europe/Berlin" />
            <datalist id="sc-zones">{#each zones as z (z)}<option value={z}></option>{/each}</datalist>
          </label>
          <p class="text-[11px] text-fg-muted">Clock times follow the time zone's own rules, including daylight saving: "02:30" on a day that skips it runs at the next valid moment, and on a day that repeats it runs once.</p>
        {/if}
        {#if kind !== "every"}
          <label class="flex items-start gap-2 text-xs text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" checked={missed === "run_once"} onchange={(e) => (missed = e.currentTarget.checked ? "run_once" : "skip")} /> If the time passes while SSHVault is closed, run once when it opens (instead of skipping it)</label>
        {/if}
        <p class="text-[11px] text-fg-muted">A run is counted once when it starts, so restarting SSHVault never runs the same time twice.</p>
      </div>

      <div class="space-y-2.5 border-t border-line pt-4 text-xs">
        <h4 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">If something fails</h4>
        <div class="flex flex-wrap items-center gap-2 text-fg-muted">
          <label for="sc-retries">Try the hosts that failed again up to</label>
          <input id="sc-retries" class="input h-7 w-16 text-xs" type="number" min="0" max={MAX_RETRIES} bind:value={retries} />
          <span>times, every</span>
          <input class="input h-7 w-20 text-xs" type="number" min="1" bind:value={retryMinutes} aria-label="Minutes between retries" />
          <span>minutes. Only for commands that are safe to repeat. Retries stop if SSHVault is closed.</span>
        </div>
        {#if (check?.rollback_steps ?? 0) > 0}
          <label class="flex items-start gap-2 text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" bind:checked={rollback} /> On a host where it fails, run the runbook's {check?.rollback_steps} rollback step{check?.rollback_steps === 1 ? "" : "s"} (they change that host again, unattended)</label>
        {/if}
        <label class="flex items-center gap-2"><span class="font-medium">Tell me</span>
          <select class="input h-7 w-auto text-xs" bind:value={notify}><option value="failure">only if something failed</option><option value="always">every time it runs</option><option value="never">never</option></select>
        </label>
        <label class="flex items-start gap-2 text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" bind:checked={allowProduction} /> Allow this schedule to run on production hosts, unattended</label>
      </div>

      {#if error}<p class="text-xs text-danger" role="alert">{error}</p>{/if}
      <div class="flex gap-2">
        <button class="btn-primary" onclick={save} data-testid="schedule-save">{editing === "new" ? "Create the schedule" : "Save the schedule"}</button>
        <button class="btn-ghost" onclick={() => (editing = null)}>Cancel</button>
      </div>
    </div>
  {/if}

  <ul class="space-y-2" aria-label="Schedules">
    {#each settings.prefs.schedules as s (s.id)}
      {@const why = blocker(s.id)}
      {@const next = nextRun(s, Date.now())}
      <li class="flex items-center gap-3 rounded-xl border border-line bg-panel p-3 text-sm" data-testid="schedule-row">
        <input type="checkbox" class="accent-input" checked={s.enabled} aria-label="Enabled" onchange={() => (s.enabled = !s.enabled)} />
        <div class="min-w-0 flex-1">
          <div class="truncate font-medium">{s.name || runbookName(s)} <span class="font-normal text-fg-muted">{s.name ? `· ${runbookName(s)} ` : ""}on {s.hostIds.length} host{s.hostIds.length === 1 ? "" : "s"}</span></div>
          <div class="text-xs text-fg-muted">{describe(s.when, s.tz)}{s.missed === "run_once" ? " · catches up once" : ""}{s.retries ? ` · retries ${s.retries}×` : ""}{s.rollback ? " · rolls back a failed host" : ""}</div>
          <div class="text-xs {s.enabled && next ? 'text-fg' : 'text-fg-muted'}" data-testid="next-run">{s.enabled && next ? `Next run ${stamp(next)}` : s.when.kind === "once" && !s.enabled ? "Done" : s.enabled ? "No upcoming run" : "Paused"}{s.lastRun ? ` · last started ${stamp(s.lastRun)}` : ""}</div>
          {#if why}<div class="text-xs text-warning">Won't run: {why}.</div>{/if}
        </div>
        <button class="icon-btn h-7 w-7" title="Run it now" aria-label="Run {s.name || runbookName(s)} now" disabled={!!why} onclick={() => runSchedule(s.id)}><Play size={13} /></button>
        <button class="icon-btn h-7 w-7" title="Edit" aria-label="Edit {s.name || runbookName(s)}" onclick={() => openEdit(s)}><Pencil size={13} /></button>
        <button class="icon-btn h-7 w-7" aria-label="Delete this schedule" onclick={() => remove(s)}><Trash2 size={13} /></button>
      </li>
    {/each}
    {#if !settings.prefs.schedules.length && !editing}<li class="rounded-lg border border-dashed border-line px-4 py-8 text-center text-sm text-fg-muted">No schedules yet. Pick a runbook, hosts and a time.</li>{/if}
  </ul>
</div>
