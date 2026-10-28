<script lang="ts">
  import { BellRing, HeartPulse, Pencil, Plus, RefreshCw, Trash2, TriangleAlert } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import {
    CHANNEL_NAMES,
    METRICS,
    PRESETS,
    STATUS_TEXT,
    ago,
    describeDeliveries,
    describeLimits,
    describeScope,
    formatValue,
    ruleFromPreset,
    statusClass,
    worst,
    type AlertRule,
    type MonitorTarget,
    type Status,
  } from "$lib/monitoring";
  import { errorMessage, type Uuid, type VaultRecord } from "$lib/types";

  type Tab = "status" | "hosts" | "rules" | "history";
  let tab = $state<Tab>("status");
  let busy = $state(false);
  let now = $state(Date.now());

  // Keeps "5 s ago" moving.
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(t);
  });

  $effect(() => {
    // Looking at the history reads it.
    if (tab === "history") monitoring.unread = 0;
  });

  const TABS: { id: Tab; label: string }[] = [
    { id: "status", label: "Status" },
    { id: "hosts", label: "Hosts" },
    { id: "rules", label: "Rules" },
    { id: "history", label: "History" },
  ];

  const targets = $derived(monitoring.targets.filter((t) => t.data));
  const rules = $derived(monitoring.rules.filter((r) => r.data));
  const channelsOn = $derived(
    monitoring.channels ? [monitoring.channels.channels.telegram.enabled, monitoring.channels.channels.bale.enabled, monitoring.channels.channels.email.enabled].filter(Boolean).length : 0,
  );

  function pill(s: Status | null) {
    return `inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium ${statusClass(s)}`;
  }

  function summary(id: Uuid): { status: Status | null; text: string } {
    const st = monitoring.statuses[id];
    if (!st) return { status: null, text: "Waiting for the first check" };
    return { status: worst(st.states), text: "" };
  }

  async function run<T>(f: () => Promise<T>) {
    busy = true;
    try {
      return await f();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = false;
    }
  }

  async function removeTarget(rec: VaultRecord<MonitorTarget>) {
    if (!(await ask(`Stop watching "${rec.data?.label}"?`, { title: "Remove host", confirm: "Remove", danger: true }))) return;
    await run(() => monitoring.deleteTarget(rec.id, rec.rev));
  }

  async function removeRule(rec: VaultRecord<AlertRule>) {
    if (!(await ask(`Delete the rule "${rec.data?.name}"?`, { title: "Delete rule", confirm: "Delete", danger: true }))) return;
    await run(() => monitoring.deleteRule(rec.id, rec.rev));
  }

  async function toggleTarget(rec: VaultRecord<MonitorTarget>) {
    if (!rec.data) return;
    await run(() => monitoring.saveTarget(rec.id, rec.rev, { ...$state.snapshot(rec.data!), enabled: !rec.data!.enabled }));
  }

  async function toggleRule(rec: VaultRecord<AlertRule>) {
    if (!rec.data) return;
    await run(() => monitoring.saveRule(rec.id, rec.rev, { ...$state.snapshot(rec.data!), enabled: !rec.data!.enabled }));
  }

  async function addPreset(name: string) {
    await run(() => monitoring.saveRule(null, null, ruleFromPreset(name)));
  }

  const checkSummary = (t: MonitorTarget) =>
    t.checks
      .map((c) => (c.kind === "ping" ? "ping" : c.kind === "tcp" ? `port ${c.port}` : "CPU, memory, disk"))
      .join(", ");
  const hostLine = (t: MonitorTarget) => [`Every ${t.interval_secs} s`, checkSummary(t), t.tags.join(", ")].filter(Boolean).join(" · ");
  const ruleLine = (r: AlertRule) =>
    [describeLimits(r), `after ${r.attempts} in a row`, describeScope(r.scope, monitoring.targetName), r.channels.length ? `only ${r.channels.map((c) => CHANNEL_NAMES[c]).join(", ")}` : ""].filter(Boolean).join(" · ");
</script>

<div class="flex-1 overflow-y-auto bg-base p-8" data-testid="monitoring">
  <div class="mx-auto max-w-6xl space-y-5">
    <div>
      <h1 class="flex items-center gap-2 text-lg font-semibold"><HeartPulse size={20} class="text-accent" /> Monitoring</h1>
      <p class="mt-1 text-sm text-fg-muted">
        Watch hosts with ping, port checks and CPU, memory and disk readings, and get a message on Telegram, Bale or email when one changes state. Checks run in the background
        while the vault is unlocked; they stop when it locks. This is separate from the alerts under Ops.
      </p>
    </div>

    {#if monitoring.configError}
      <p class="flex gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger"><TriangleAlert size={15} class="mt-0.5 shrink-0" /> The stored monitoring settings could not be read: {monitoring.configError}</p>
    {:else if !monitoring.running}
      <p class="flex gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning"><TriangleAlert size={15} class="mt-0.5 shrink-0" /> Monitoring is not running. It starts when the vault is unlocked.</p>
    {/if}
    {#if settings.prefs.autoLockMinutes > 0}
      <p class="text-xs text-fg-muted">Auto-lock is on ({settings.prefs.autoLockMinutes} min). When the vault locks, checks and alerts stop until it is unlocked again.</p>
    {/if}
    {#if channelsOn === 0}
      <p class="flex flex-wrap items-center gap-2 rounded-md border border-line bg-panel px-3 py-2 text-sm">
        <BellRing size={15} class="text-fg-muted" /> No notification channel is switched on, so alerts show only here.
        <button class="text-accent hover:underline" onclick={() => (ui.view = "settings")}>Set up Telegram, Bale or email…</button>
      </p>
    {/if}

    <div class="flex w-fit gap-1 rounded-lg border border-line bg-panel p-1 text-sm" role="tablist">
      {#each TABS as t (t.id)}
        <button
          role="tab"
          aria-selected={tab === t.id}
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition-colors {tab === t.id ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          onclick={() => (tab = t.id)}
        >
          {t.label}
          {#if t.id === "history" && monitoring.unread > 0}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{monitoring.unread}</span>{/if}
        </button>
      {/each}
    </div>

    {#if tab === "status"}
      {#if targets.length === 0}
        <EmptyState icon={HeartPulse} text="No host is being watched yet.">
          <button class="btn-primary mt-3" onclick={() => (ui.modal = { kind: "monitor-target", id: null })}><Plus size={14} /> Add a host</button>
        </EmptyState>
      {:else}
        <div class="overflow-x-auto rounded-xl border border-line bg-panel">
          <table class="w-full text-left text-sm">
            <thead class="border-b border-line text-xs text-fg-muted">
              <tr>
                <th class="px-3 py-2 font-medium">Host</th>
                <th class="px-3 py-2 font-medium">State</th>
                <th class="px-3 py-2 font-medium">Ping</th>
                <th class="px-3 py-2 font-medium">Ports</th>
                <th class="px-3 py-2 font-medium">CPU</th>
                <th class="px-3 py-2 font-medium">Memory</th>
                <th class="px-3 py-2 font-medium">Disk</th>
                <th class="px-3 py-2 font-medium">Checked</th>
                <th class="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody class="divide-y divide-line">
              {#each targets as rec (rec.id)}
                {@const t = rec.data!}
                {@const st = monitoring.statuses[rec.id]}
                {@const sum = summary(rec.id)}
                <tr class={t.enabled ? "" : "opacity-50"}>
                  <td class="px-3 py-2">
                    <div class="font-medium">{t.label}</div>
                    <div class="font-mono text-xs text-fg-muted">{t.address}</div>
                  </td>
                  <td class="px-3 py-2">
                    {#if !t.enabled}<span class="text-xs text-fg-muted">Paused</span>
                    {:else if sum.status}<span class={pill(sum.status)}>{STATUS_TEXT[sum.status]}</span>
                    {:else}<span class="text-xs text-fg-muted">{sum.text || (st ? "No rule applies" : "")}</span>{/if}
                  </td>
                  <td class="px-3 py-2 font-mono text-xs">
                    {#if st?.ping}{formatValue("ping_loss", st.ping.loss_pct)} lost{#if st.ping.avg_ms !== null}, {formatValue("ping_latency", st.ping.avg_ms)}{/if}{:else}–{/if}
                  </td>
                  <td class="px-3 py-2 font-mono text-xs">
                    {#if st?.ports.length}{#each st.ports as p (p.port)}<span class={p.open ? "text-success" : "text-danger"} title={p.error ?? ""}>{p.port}</span>{" "}{/each}{:else}–{/if}
                  </td>
                  <td class="px-3 py-2 font-mono text-xs">{formatValue("cpu", st?.metrics?.cpu ?? null)}</td>
                  <td class="px-3 py-2 font-mono text-xs">{formatValue("mem", st?.metrics?.mem ?? null)}</td>
                  <td class="px-3 py-2 font-mono text-xs">{formatValue("disk", st?.metrics?.disk ?? null)}</td>
                  <td class="px-3 py-2 text-xs text-fg-muted">{st ? ago(st.checked_at, now) : "–"}</td>
                  <td class="px-3 py-2 text-right">
                    <button class="icon-btn" title="Check now" aria-label="Check {t.label} now" disabled={!t.enabled || !monitoring.running} onclick={() => void run(() => monitoring.runNow(rec.id))}><RefreshCw size={14} /></button>
                  </td>
                </tr>
                {#if st && (st.errors.length || st.states.some((s) => s.status !== "ok"))}
                  <tr>
                    <td colspan="9" class="bg-base/50 px-3 py-1.5 text-xs">
                      {#each st.states.filter((s) => s.status !== "ok") as s (s.rule_id + s.subject)}
                        <span class="mr-3"><span class={pill(s.status)}>{STATUS_TEXT[s.status]}</span> {s.rule}</span>
                      {/each}
                      {#each st.errors as e (e)}<span class="mr-3 text-fg-muted">{e}</span>{/each}
                    </td>
                  </tr>
                {/if}
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}

    {#if tab === "hosts"}
      <div class="flex items-center justify-between">
        <p class="text-sm text-fg-muted">The hosts to watch. They don't have to be saved SSH hosts; a router or a printer will do.</p>
        <button class="btn-primary" data-testid="monitor-new" onclick={() => (ui.modal = { kind: "monitor-target", id: null })}><Plus size={14} /> Add a host</button>
      </div>
      {#if targets.length === 0}
        <EmptyState icon={HeartPulse} text="Nothing to watch yet." />
      {:else}
        <div class="divide-y divide-line rounded-xl border border-line bg-panel">
          {#each targets as rec (rec.id)}
            {@const t = rec.data!}
            <div class="flex items-center gap-3 px-4 py-2.5 {t.enabled ? '' : 'opacity-60'}">
              <input type="checkbox" class="accent-input" checked={t.enabled} disabled={busy} aria-label="Watch {t.label}" onchange={() => void toggleTarget(rec)} />
              <div class="min-w-0 flex-1">
                <div class="truncate text-sm font-medium">{t.label} <span class="font-mono text-xs font-normal text-fg-muted">{t.address}</span></div>
                <div class="truncate text-xs text-fg-muted">{hostLine(t)}</div>
              </div>
              <button class="icon-btn" title="Edit" aria-label="Edit {t.label}" onclick={() => (ui.modal = { kind: "monitor-target", id: rec.id })}><Pencil size={14} /></button>
              <button class="icon-btn" title="Remove" aria-label="Remove {t.label}" onclick={() => void removeTarget(rec)}><Trash2 size={14} /></button>
            </div>
          {/each}
        </div>
      {/if}
    {/if}

    {#if tab === "rules"}
      <div class="space-y-3">
        <p class="text-sm text-fg-muted">
          A rule says when a reading is a problem. A state changes only after several checks in a row agree, and you are told once when it changes, not on every check.
        </p>
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-xs text-fg-muted">Add:</span>
          {#each PRESETS as p (p.name)}
            <button class="btn-secondary py-1 text-xs" disabled={busy} onclick={() => void addPreset(p.name)}>{p.name}</button>
          {/each}
          <button class="btn-primary ml-auto" data-testid="rule-new" onclick={() => (ui.modal = { kind: "alert-rule", id: null })}><Plus size={14} /> New rule</button>
        </div>
        {#if rules.length === 0}
          <EmptyState icon={BellRing} text="No rules yet. Add one above to be told when something goes wrong." />
        {:else}
          <div class="divide-y divide-line rounded-xl border border-line bg-panel">
            {#each rules as rec (rec.id)}
              {@const r = rec.data!}
              <div class="flex items-center gap-3 px-4 py-2.5 {r.enabled ? '' : 'opacity-60'}">
                <input type="checkbox" class="accent-input" checked={r.enabled} disabled={busy} aria-label="Use {r.name}" onchange={() => void toggleRule(rec)} />
                <div class="min-w-0 flex-1">
                  <div class="truncate text-sm font-medium">{r.name}{#if r.name !== METRICS[r.metric].label} <span class="text-xs font-normal text-fg-muted">{METRICS[r.metric].label}</span>{/if}</div>
                  <div class="truncate text-xs text-fg-muted">{ruleLine(r)}</div>
                </div>
                <button class="icon-btn" title="Edit" aria-label="Edit {r.name}" onclick={() => (ui.modal = { kind: "alert-rule", id: rec.id })}><Pencil size={14} /></button>
                <button class="icon-btn" title="Delete" aria-label="Delete {r.name}" onclick={() => void removeRule(rec)}><Trash2 size={14} /></button>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    {#if tab === "history"}
      {#if monitoring.history.length === 0}
        <EmptyState icon={BellRing} text="No alert has been raised since the vault was unlocked. This list is not kept when it locks." />
      {:else}
        <div class="divide-y divide-line rounded-xl border border-line bg-panel">
          {#each monitoring.history as a, i (a.at + a.target_id + a.rule + i)}
            <div class="px-4 py-2.5">
              <div class="flex flex-wrap items-center gap-2 text-sm">
                <span class={pill(a.status)}>{a.status === "ok" ? "Recovered" : STATUS_TEXT[a.status]}</span>
                <span class="font-medium">{a.target}</span>
                <span class="text-fg-muted">{a.rule}</span>
                <span class="ml-auto text-xs text-fg-muted">{ago(a.at, now)}</span>
              </div>
              <div class="mt-0.5 text-xs">{a.summary}</div>
              <div class="mt-0.5 text-xs text-fg-muted">{describeDeliveries(a.deliveries)}</div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>
