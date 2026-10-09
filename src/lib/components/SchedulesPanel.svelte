<script lang="ts">
  import { CalendarClock, Play, Plus, Trash2 } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { DAYS, MAX_RETRIES, MIN_EVERY, describe, nextRun, problem, scheduleProblem, validZone, type Schedule, type When } from "$lib/schedule";
  import { blocker, runSchedule } from "$lib/stores/scheduler.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { RunbookCheck } from "$lib/runbook";
  import * as api from "$lib/api";

  const runbooks = $derived(vaultStore.runbooks.filter((r) => !r.deleted && r.data));
  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && !h.deleted && (!h.data.protocol || h.data.protocol === "ssh")));

  let adding = $state(false);
  let runbookId = $state("");
  let check = $state<RunbookCheck | null>(null);
  let params = $state<Record<string, string>>({});
  let picked = $state<Set<string>>(new Set());
  let kind = $state<When["kind"]>("daily");
  let minutes = $state(60);
  let at = $state("02:30");
  let days = $state<number[]>([1, 2, 3, 4, 5]);
  let allowProduction = $state(false);
  let error = $state<string | null>(null);
  /** For "once": a local date and time, as the browser's date field gives it. */
  let onceAt = $state("");
  let tz = $state("");
  let missed = $state<"skip" | "run_once">("skip");
  let retries = $state(0);
  let retryMinutes = $state(15);
  let rollback = $state(false);
  const zones = typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];

  const when = $derived<When>(kind === "every" ? { kind, minutes } : kind === "once" ? { kind, at: onceAt ? new Date(onceAt).getTime() : NaN } : kind === "daily" ? { kind, at } : { kind, days, at });

  async function chooseRunbook(id: string) {
    runbookId = id;
    params = {};
    check = null;
    const rb = runbooks.find((r) => r.id === id)?.data;
    if (!rb) return;
    check = await api.runbookCheck(rb.body).catch(() => null);
    for (const p of check?.params ?? []) if (p.kind === "text" && p.default !== null) params[p.name] = p.default;
  }

  async function create() {
    error = null;
    const rb = runbooks.find((r) => r.id === runbookId)?.data;
    if (!rb || !check) return (error = "Choose a runbook.");
    if (check.problems.length) return (error = "That runbook has problems; fix them first.");
    if (check.params.some((p) => p.kind === "file" && !p.optional)) return (error = "That runbook needs a file chosen each time it runs, which a schedule can't give.");
    if (picked.size === 0) return (error = "Choose at least one host.");
    const bad = problem(when) ?? (tz.trim() && !validZone(tz.trim()) ? `"${tz.trim()}" is not a time zone this computer knows.` : null);
    if (bad) return (error = bad);
    if (when.kind === "once" && when.at <= Date.now()) return (error = "Choose a time in the future.");
    const prod = [...picked].filter((h) => vaultStore.effectiveEnv(vaultStore.hostById.get(h)?.data) === "production");
    if (prod.length && allowProduction && !(await ask(`This schedule will run "${rb.name}" by itself on ${prod.length} production host${prod.length === 1 ? "" : "s"}, without anyone watching.`, { title: "Production hosts", confirm: "Allow it", danger: true, requireText: "production" }))) return;
    const s: Schedule = { id: crypto.randomUUID(), runbookId, params: Object.fromEntries(Object.entries($state.snapshot(params)).filter(([, v]) => v !== "")), hostIds: [...picked], when: $state.snapshot(when) as When, enabled: true, allowProduction: allowProduction && prod.length > 0, lastRun: Date.now(), ...(tz.trim() && when.kind !== "every" && when.kind !== "once" ? { tz: tz.trim() } : {}), ...(missed === "run_once" && when.kind !== "every" ? { missed } : {}), ...(retries > 0 ? { retries, retryMinutes } : {}), ...(rollback && (check?.rollback_steps ?? 0) > 0 ? { rollback: true } : {}) };
    const slip = scheduleProblem(s);
    if (slip) return (error = slip);
    settings.prefs.schedules = [...settings.prefs.schedules, s];
    adding = false;
    picked = new Set();
  }

  const remove = async (s: Schedule) => {
    if (await ask("Delete this schedule?", { title: "Delete schedule", confirm: "Delete", danger: true })) settings.prefs.schedules = settings.prefs.schedules.filter((x) => x.id !== s.id);
  };
  const name = (s: Schedule) => runbooks.find((r) => r.id === s.runbookId)?.data?.name ?? "(deleted runbook)";
  const stamp = (t: number) => new Date(t).toLocaleString();
  const toggleHost = (id: string) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    picked = next;
  };
  const toggleDay = (d: number) => (days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d].sort());
</script>

<section aria-label="Schedules" class="max-w-3xl space-y-4" data-testid="schedules">
  <div class="flex items-center gap-2">
    <CalendarClock size={16} class="text-accent" />
    <h3 class="text-sm font-semibold">Scheduled runs</h3>
    <button class="btn-secondary ml-auto py-1 text-xs" onclick={() => (adding = !adding)} data-testid="schedule-new"><Plus size={13} /> New schedule</button>
  </div>
  <p class="rounded-md border border-line bg-base/40 p-3 text-xs text-fg-muted">
    Schedules run <strong>only while SSHVault is open</strong>, with the vault unlocked; there is no background service. A time that passes while
    the app is closed (or the computer sleeps for more than ten minutes) is skipped, unless the schedule says to run once when it opens. They are kept on this computer and not synced. A run that includes a production host
    is skipped unless you allowed it when you made the schedule. Failures are reported here and in Runbooks → History.
  </p>

  {#if adding}
    <div class="space-y-3 rounded-lg border border-line p-4" data-testid="schedule-form">
      <div>
        <label class="label" for="sc-rb">Runbook</label>
        <select id="sc-rb" class="input" value={runbookId} onchange={(e) => chooseRunbook(e.currentTarget.value)}>
          <option value="">Choose…</option>
          {#each runbooks as r (r.id)}<option value={r.id}>{r.data!.name}</option>{/each}
        </select>
      </div>
      {#each (check?.params ?? []).filter((p) => p.kind === "text") as p (p.name)}
        <div>
          <label class="label" for="sp-{p.name}">{p.label || p.name}</label>
          {#if p.choices.length}<select id="sp-{p.name}" class="input" bind:value={params[p.name]}>{#each p.choices as c (c)}<option value={c}>{c}</option>{/each}</select>
          {:else}<input id="sp-{p.name}" class="input font-mono text-xs" bind:value={params[p.name]} />{/if}
        </div>
      {/each}
      <div>
        <span class="label">Hosts ({picked.size})</span>
        <ul class="max-h-40 overflow-auto rounded-md border border-line p-1">
          {#each hosts as h (h.id)}
            <li><label class="flex items-center gap-2 rounded px-2 py-0.5 text-sm hover:bg-hover"><input type="checkbox" class="accent-input" checked={picked.has(h.id)} onchange={() => toggleHost(h.id)} /> <span class="min-w-0 flex-1 truncate">{h.data!.label}</span>{#if vaultStore.effectiveEnv(h.data) === "production"}<span class="rounded bg-danger/15 px-1.5 text-[10px] text-danger">production</span>{/if}</label></li>
          {/each}
        </ul>
      </div>
      <div class="grid grid-cols-3 gap-3">
        <div>
          <label class="label" for="sc-kind">When</label>
          <select id="sc-kind" class="input" bind:value={kind}>
            <option value="every">Every so often</option>
            <option value="daily">Every day</option>
            <option value="weekly">On some days</option>
            <option value="once">Once</option>
          </select>
        </div>
        {#if kind === "once"}
          <div class="col-span-2">
            <label class="label" for="sc-once">Date and time (this computer's clock)</label>
            <input id="sc-once" class="input" type="datetime-local" bind:value={onceAt} />
          </div>
        {:else if kind === "every"}
          <div>
            <label class="label" for="sc-min">Minutes</label>
            <input id="sc-min" class="input" type="number" min={MIN_EVERY} bind:value={minutes} />
          </div>
        {:else}
          <div>
            <label class="label" for="sc-at">At</label>
            <input id="sc-at" class="input font-mono" bind:value={at} placeholder="02:30" />
          </div>
        {/if}
      </div>
      {#if kind === "weekly"}
        <div class="flex gap-1" role="group" aria-label="Days">
          {#each DAYS as d, i (d)}
            <button type="button" class="rounded-md border px-2.5 py-1 text-xs {days.includes(i) ? 'border-accent bg-accent/15 text-accent' : 'border-line text-fg-muted'}" aria-pressed={days.includes(i)} onclick={() => toggleDay(i)}>{d}</button>
          {/each}
        </div>
      {/if}
      {#if kind === "daily" || kind === "weekly"}
        <div>
          <label class="label" for="sc-tz">Time zone <span class="font-normal text-fg-muted">(empty: this computer's)</span></label>
          <input id="sc-tz" class="input font-mono text-xs" list="sc-zones" bind:value={tz} placeholder="Europe/Berlin" />
          <datalist id="sc-zones">{#each zones as z (z)}<option value={z}></option>{/each}</datalist>
        </div>
      {/if}
      {#if kind !== "every"}
        <label class="flex items-start gap-2 text-xs text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" checked={missed === "run_once"} onchange={(e) => (missed = e.currentTarget.checked ? "run_once" : "skip")} /> If the time passes while SSHVault is closed, run once when it opens (instead of skipping it)</label>
      {/if}
      <div class="flex flex-wrap items-center gap-2 text-xs text-fg-muted">
        <label for="sc-retries">If hosts fail, try those again up to</label>
        <input id="sc-retries" class="input w-16" type="number" min="0" max={MAX_RETRIES} bind:value={retries} />
        <span>times, every</span>
        <input id="sc-retry-min" class="input w-20" type="number" min="1" bind:value={retryMinutes} aria-label="Minutes between retries" />
        <span>minutes. Only for commands that are safe to repeat; retries stop if SSHVault is closed.</span>
      </div>
      {#if (check?.rollback_steps ?? 0) > 0}
        <label class="flex items-start gap-2 text-xs text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" bind:checked={rollback} /> On a host where it fails, run the runbook's {check?.rollback_steps} rollback step{check?.rollback_steps === 1 ? "" : "s"} (they change that host again, unattended)</label>
      {/if}
      <label class="flex items-start gap-2 text-xs text-fg-muted"><input type="checkbox" class="mt-0.5 accent-input" bind:checked={allowProduction} /> Allow this schedule to run on production hosts, unattended</label>
      {#if error}<p class="text-xs text-danger" role="alert">{error}</p>{/if}
      <div class="flex gap-2">
        <button class="btn-primary" onclick={create} data-testid="schedule-save">Create the schedule</button>
        <button class="btn-ghost" onclick={() => (adding = false)}>Cancel</button>
      </div>
    </div>
  {/if}

  <ul class="space-y-2">
    {#each settings.prefs.schedules as s (s.id)}
      {@const why = blocker(s.id)}
      {@const next = nextRun(s, Date.now())}
      <li class="flex items-center gap-3 rounded-lg border border-line p-3 text-sm" data-testid="schedule-row">
        <input type="checkbox" class="accent-input" checked={s.enabled} aria-label="Enabled" onchange={() => (s.enabled = !s.enabled)} />
        <div class="min-w-0 flex-1">
          <div class="truncate font-medium">{name(s)} <span class="font-normal text-fg-muted">on {s.hostIds.length} host{s.hostIds.length === 1 ? "" : "s"}</span></div>
          <div class="text-xs text-fg-muted">{describe(s.when, s.tz)}{s.missed === "run_once" ? " · catches up once" : ""}{s.retries ? ` · retries ${s.retries}×` : ""}{s.rollback ? " · rolls back a failed host" : ""}{s.enabled && next ? ` · next ${stamp(next)}` : s.when.kind === "once" && !s.enabled ? " · done" : ""}{s.lastRun ? ` · last started ${stamp(s.lastRun)}` : ""}</div>
          {#if why}<div class="text-xs text-warning">Won't run: {why}.</div>{/if}
        </div>
        <button class="btn-ghost py-1 text-xs" title="Run it now" disabled={!!why} onclick={() => runSchedule(s.id)}><Play size={13} /></button>
        <button class="btn-ghost py-1 text-xs" aria-label="Delete this schedule" onclick={() => remove(s)}><Trash2 size={13} /></button>
      </li>
    {/each}
    {#if !settings.prefs.schedules.length && !adding}<li class="text-sm text-fg-muted">No schedules yet.</li>{/if}
  </ul>
</section>
