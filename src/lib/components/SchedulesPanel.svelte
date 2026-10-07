<script lang="ts">
  import { CalendarClock, Play, Plus, Trash2 } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { DAYS, MIN_EVERY, describe, nextRun, problem, type Schedule, type When } from "$lib/schedule";
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

  const when = $derived<When>(kind === "every" ? { kind, minutes } : kind === "daily" ? { kind, at } : { kind, days, at });

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
    const bad = problem(when);
    if (bad) return (error = bad);
    const prod = [...picked].filter((h) => vaultStore.effectiveEnv(vaultStore.hostById.get(h)?.data) === "production");
    if (prod.length && allowProduction && !(await ask(`This schedule will run "${rb.name}" by itself on ${prod.length} production host${prod.length === 1 ? "" : "s"}, without anyone watching.`, { title: "Production hosts", confirm: "Allow it", danger: true, requireText: "production" }))) return;
    const s: Schedule = { id: crypto.randomUUID(), runbookId, params: Object.fromEntries(Object.entries($state.snapshot(params)).filter(([, v]) => v !== "")), hostIds: [...picked], when: $state.snapshot(when) as When, enabled: true, allowProduction: allowProduction && prod.length > 0, lastRun: Date.now() };
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
    Schedules run <strong>only while SSHVault is open</strong>, with the vault unlocked. A time that passes while the app is closed (or the computer
    sleeps for more than ten minutes) is skipped, not caught up. They are kept on this computer and not synced. A run that includes a production host
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
          </select>
        </div>
        {#if kind === "every"}
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
          <div class="text-xs text-fg-muted">{describe(s.when)}{s.enabled && next ? ` · next ${stamp(next)}` : ""}{s.lastRun ? ` · last started ${stamp(s.lastRun)}` : ""}</div>
          {#if why}<div class="text-xs text-warning">Won't run: {why}.</div>{/if}
        </div>
        <button class="btn-ghost py-1 text-xs" title="Run it now" disabled={!!why} onclick={() => runSchedule(s.id)}><Play size={13} /></button>
        <button class="btn-ghost py-1 text-xs" aria-label="Delete this schedule" onclick={() => remove(s)}><Trash2 size={13} /></button>
      </li>
    {/each}
    {#if !settings.prefs.schedules.length && !adding}<li class="text-sm text-fg-muted">No schedules yet.</li>{/if}
  </ul>
</section>
