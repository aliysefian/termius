<script lang="ts">
  import { Pencil, RefreshCw, SquareTerminal, X } from "lucide-svelte";
  import HealthBadge from "./HealthBadge.svelte";
  import NetworkTab from "./NetworkTab.svelte";
  import { ago, describeDeliveries, formatValue, STATUS_TEXT, statusClass } from "$lib/monitoring";
  import { checkLines, type HostRow } from "$lib/monitoringview";
  import { formatRate, formatTotal } from "$lib/netdata";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  export type DetailTab = "overview" | "network" | "resources" | "checks" | "alerts" | "history";

  let { row, now, tab = $bindable(), onclose }: { row: HostRow; now: number; tab: DetailTab; onclose: () => void } = $props();

  const TABS: { id: DetailTab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "network", label: "Network" },
    { id: "resources", label: "Resources" },
    { id: "checks", label: "Checks" },
    { id: "alerts", label: "Alerts" },
    { id: "history", label: "History" },
  ];

  const t = $derived(row.target);
  const st = $derived(row.status);
  const lines = $derived(checkLines(st));
  const problems = $derived((st?.states ?? []).filter((s) => s.status !== "ok"));
  const alerts = $derived(monitoring.history.filter((a) => a.target_id === row.id));
  const hostMissing = $derived(!!t.ssh_host_id && !vaultStore.hostById.has(t.ssh_host_id));
  const unit = $derived(settings.prefs.netUnit);
  const samples = $derived(t.ssh_host_id ? (hostMetrics.history[t.ssh_host_id] ?? []) : []);
  /** Bytes the main interface has carried since the host started (received plus sent). */
  function netTotal(r: HostRow): number | null {
    const m = r.status?.metrics;
    const i = m?.net?.interfaces.find((x) => x.name === m.net_headline);
    return i ? i.rx_total + i.tx_total : null;
  }
  const pill = (s: Parameters<typeof statusClass>[0]) => `inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium ${statusClass(s)}`;

  function tabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((x) => x.id === tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    tab = next.id;
    queueMicrotask(() => document.getElementById(`mon-tab-${next.id}`)?.focus());
  }

  /** A polyline of a 0-100 series over the samples that have it. */
  function line(pick: (s: (typeof samples)[number]) => number | null): string {
    const pts = samples.map((s, i) => [i, pick(s)] as const).filter((p): p is readonly [number, number] => p[1] !== null);
    if (pts.length < 2) return "";
    const n = samples.length - 1 || 1;
    return pts.map(([i, v]) => `${((i / n) * 100).toFixed(1)},${(38 - (Math.min(100, v) / 100) * 36 - 1).toFixed(1)}`).join(" ");
  }
</script>

{#snippet gauge(label: string, v: number | null, series: string)}
  <div class="rounded-[10px] border border-line bg-base/50 p-3">
    <div class="flex items-baseline justify-between"><span class="text-[11px] uppercase tracking-wide text-fg-muted">{label}</span><span class="font-mono text-xl font-semibold tabular-nums {v !== null && v >= 90 ? 'text-danger' : v !== null && v >= 80 ? 'text-warning' : ''}">{v === null ? "–" : `${Math.round(v)}%`}</span></div>
    <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-fg-muted/10" role="progressbar" aria-label={label} aria-valuenow={v ?? undefined} aria-valuemin="0" aria-valuemax="100"><div class="h-full rounded-full {v !== null && v >= 90 ? 'bg-danger' : v !== null && v >= 80 ? 'bg-warning' : 'bg-accent'}" style="width:{v ?? 0}%"></div></div>
    {#if series}<svg viewBox="0 0 100 38" class="mt-2 h-10 w-full" preserveAspectRatio="none" aria-hidden="true"><polyline points={series} fill="none" stroke="var(--color-accent)" stroke-width="1.3" vector-effect="non-scaling-stroke" /></svg>{/if}
  </div>
{/snippet}

<section class="flex h-full min-h-0 flex-col" aria-label="Details of {t.label}">
  <header class="space-y-3 border-b border-line px-4 pb-3 pt-4">
    <div class="flex items-start gap-2">
      <div class="min-w-0 flex-1">
        <h2 class="truncate text-[16px] font-semibold leading-6" title={t.label}>{t.label}</h2>
        <p class="truncate font-mono text-xs text-fg-muted">{t.address}</p>
      </div>
      <button class="icon-btn h-8 w-8 shrink-0" title="Edit this monitor" aria-label="Edit {t.label}" onclick={() => (ui.modal = { kind: "monitor-target", id: row.id })}><Pencil size={15} /></button>
      <button class="icon-btn h-8 w-8 shrink-0" title="Close details (Esc)" aria-label="Close details" onclick={onclose}><X size={16} /></button>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <HealthBadge health={row.health} />
      <span class="text-xs text-fg-muted">{st ? `checked ${ago(st.checked_at, now)}` : "not checked yet"}</span>
      <div class="flex-1"></div>
      <button class="icon-btn h-8 w-8 border border-line" title="Check now" aria-label="Check {t.label} now" disabled={!t.enabled || !monitoring.running} onclick={() => void monitoring.runNow(row.id)}><RefreshCw size={15} /></button>
      <button class="btn-secondary h-8 text-xs" disabled={!t.ssh_host_id || hostMissing} title={t.ssh_host_id ? "Open a terminal on this host" : "This monitor has no SSH host"} onclick={() => t.ssh_host_id && ui.openTerminal(t.ssh_host_id, t.label)}><SquareTerminal size={14} /> Open SSH Terminal</button>
    </div>
  </header>

  <div class="flex gap-1 overflow-x-auto border-b border-line px-3 pt-2" role="tablist" aria-label="Host details">
    {#each TABS as x (x.id)}
      <button id="mon-tab-{x.id}" role="tab" aria-selected={tab === x.id} aria-controls="mon-panel" tabindex={tab === x.id ? 0 : -1} class="-mb-px shrink-0 rounded-t-md border-b-2 px-2.5 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent {tab === x.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}" onclick={() => (tab = x.id)} onkeydown={tabKey}>
        {x.label}{#if x.id === "alerts" && problems.length}<span class="ml-1.5 rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{problems.length}</span>{/if}
      </button>
    {/each}
  </div>

  <div id="mon-panel" role="tabpanel" aria-labelledby="mon-tab-{tab}" class="min-h-0 flex-1 overflow-y-auto p-4">
    {#if tab === "overview"}
      <div class="space-y-5">
        {#if problems.length || st?.errors.length}
          <div class="space-y-1.5 rounded-[10px] border border-line bg-base/50 p-3 text-sm">
            {#each problems as s (s.rule_id + s.subject)}<div class="flex items-center gap-2"><span class={pill(s.status)}>{STATUS_TEXT[s.status]}</span><span>{s.rule}</span></div>{/each}
            {#each st?.errors ?? [] as e (e)}<div class="text-xs text-fg-muted">{e}</div>{/each}
          </div>
        {:else if row.health === "healthy"}
          <p class="rounded-[10px] border border-success/30 bg-success/10 px-3 py-2.5 text-sm text-success">Every check passed and no rule is raised.</p>
        {:else if row.health === "waiting"}
          <p class="rounded-[10px] border border-line bg-base/50 px-3 py-2.5 text-sm text-fg-muted">Waiting for the first check.</p>
        {/if}

        <section aria-labelledby="mo-now">
          <h3 id="mo-now" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Right now</h3>
          <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-sm">
            <dt class="text-fg-muted">CPU</dt><dd class="font-mono text-xs">{formatValue("cpu", st?.metrics?.cpu ?? null)}</dd>
            <dt class="text-fg-muted">Memory</dt><dd class="font-mono text-xs">{formatValue("mem", st?.metrics?.mem ?? null)}</dd>
            <dt class="text-fg-muted">Disk (/)</dt><dd class="font-mono text-xs">{formatValue("disk", st?.metrics?.disk ?? null)}</dd>
            <dt class="text-fg-muted">Ping</dt><dd class="font-mono text-xs">{#if st?.ping}{formatValue("ping_loss", st.ping.loss_pct)} lost{#if st.ping.avg_ms !== null}, {formatValue("ping_latency", st.ping.avg_ms)}{/if}{:else}–{/if}</dd>
            <dt class="text-fg-muted">Download</dt><dd class="font-mono text-xs">{row.net ? `${formatRate(row.net.rx, unit).value} ${formatRate(row.net.rx, unit).unit}` : "–"}{#if row.net}<span class="text-fg-muted"> on {row.net.name}</span>{/if}</dd>
            <dt class="text-fg-muted">Upload</dt><dd class="font-mono text-xs">{row.net ? `${formatRate(row.net.tx, unit).value} ${formatRate(row.net.tx, unit).unit}` : "–"}</dd>
          </dl>
          {#if row.net}<button class="mt-2 text-xs text-accent hover:underline" onclick={() => (tab = "network")}>See network traffic…</button>{/if}
        </section>

        <section aria-labelledby="mo-cfg">
          <h3 id="mo-cfg" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Monitor</h3>
          <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-sm">
            <dt class="text-fg-muted">Address</dt><dd class="font-mono text-xs">{t.address}</dd>
            <dt class="text-fg-muted">Checked every</dt><dd class="text-xs">{t.interval_secs} s <span class="text-fg-muted">(readings over SSH: at least 30 s)</span></dd>
            <dt class="text-fg-muted">SSH host</dt><dd class="text-xs">{t.ssh_host_id ? (vaultStore.hostById.get(t.ssh_host_id)?.data?.label ?? "missing host") : "none"}</dd>
            <dt class="text-fg-muted">Tags</dt><dd class="text-xs">{t.tags.join(", ") || "–"}</dd>
            {#if t.notes}<dt class="text-fg-muted">Notes</dt><dd class="whitespace-pre-wrap text-xs">{t.notes}</dd>{/if}
          </dl>
        </section>
      </div>
    {:else if tab === "network"}
      <NetworkTab {row} {now} active={tab === "network"} />
    {:else if tab === "resources"}
      <div class="space-y-3">
        {#if !t.ssh_host_id}
          <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">CPU, memory and disk are read over SSH. Edit this monitor and choose a saved SSH host.</p>
        {/if}
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
          {@render gauge("CPU", st?.metrics?.cpu ?? null, line((s) => s.cpu))}
          {@render gauge("Memory", st?.metrics?.mem ?? null, line((s) => s.mem))}
          {@render gauge("Disk (/)", st?.metrics?.disk ?? null, "")}
        </div>
        <p class="text-[11px] text-fg-muted">Charts show the readings taken since the vault was unlocked (up to 15 minutes). CPU is a share of all cores over one second; memory is used minus available.</p>
      </div>
    {:else if tab === "checks"}
      {#if lines.length === 0}
        <p class="text-sm text-fg-muted">No check has run yet.</p>
      {:else}
        <ul class="divide-y divide-line rounded-[10px] border border-line">
          {#each lines as c (c.kind + c.name)}
            <li class="flex items-center gap-3 px-3 py-2 text-sm">
              <span class="h-2 w-2 shrink-0 rounded-full {c.ok === null ? 'bg-fg-muted/50' : c.ok ? 'bg-success' : 'bg-danger'}"></span>
              <span class="min-w-0 flex-1 truncate">{c.name}</span>
              <span class="max-w-[60%] truncate text-xs {c.ok === false ? 'text-danger' : 'text-fg-muted'}" title={c.detail}>{c.ok ? "OK" : "Failing"} · {c.detail}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if st && st.states.length}
        <h3 class="mb-2 mt-5 text-xs font-medium uppercase tracking-wide text-fg-muted">Rules judging this host</h3>
        <ul class="space-y-1.5 text-sm">{#each st.states as s (s.rule_id + s.subject)}<li class="flex items-center gap-2"><span class={pill(s.status)}>{STATUS_TEXT[s.status]}</span><span class="min-w-0 flex-1 truncate">{s.rule}</span><span class="text-xs text-fg-muted">since {ago(s.since, now)}</span></li>{/each}</ul>
      {/if}
    {:else if tab === "alerts"}
      {#if problems.length === 0}
        <p class="rounded-[10px] border border-success/30 bg-success/10 px-3 py-2.5 text-sm text-success">No rule is raised for this host.</p>
      {:else}
        <ul class="space-y-2">{#each problems as s (s.rule_id + s.subject)}<li class="rounded-[10px] border border-line bg-base/50 p-3 text-sm"><div class="flex items-center gap-2"><span class={pill(s.status)}>{STATUS_TEXT[s.status]}</span><span class="font-medium">{s.rule}</span></div><div class="mt-1 text-xs text-fg-muted">since {ago(s.since, now)}{#if s.value !== null} · reading {s.value.toFixed(1)}{/if}</div></li>{/each}</ul>
      {/if}
    {:else}
      {#if alerts.length === 0}
        <p class="text-sm text-fg-muted">No alert was raised for this host since the vault was unlocked. The list isn't kept when it locks.</p>
      {:else}
        <ul class="divide-y divide-line rounded-[10px] border border-line">
          {#each alerts as a, i (a.at + a.rule + i)}
            <li class="px-3 py-2.5 text-sm">
              <div class="flex flex-wrap items-center gap-2"><span class={pill(a.status)}>{a.status === "ok" ? "Recovered" : STATUS_TEXT[a.status]}</span><span class="font-medium">{a.rule}</span><span class="ml-auto text-xs text-fg-muted">{ago(a.at, now)}</span></div>
              <div class="mt-0.5 text-xs">{a.summary}</div>
              <div class="mt-0.5 text-xs text-fg-muted">{describeDeliveries(a.deliveries)}</div>
            </li>
          {/each}
        </ul>
      {/if}
      <p class="mt-3 text-[11px] text-fg-muted">Readings are kept {settings.prefs.metricsKeep === "off" ? "only in memory (turn on \"Keep history\" in Settings to keep them for a day or a week)" : `for a ${settings.prefs.metricsKeep}`}. Totals so far: {formatTotal(row.net ? (netTotal(row)) : null)}.</p>
    {/if}
  </div>
</section>
