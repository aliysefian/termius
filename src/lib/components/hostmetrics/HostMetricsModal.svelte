<script lang="ts">
  import { onMount } from "svelte";
  import { untrack } from "svelte";
  import { Activity, AlertTriangle, Loader2, RefreshCw } from "lucide-svelte";
  import Modal, { DISCARD } from "../Modal.svelte";
  import CpuPanel from "./CpuPanel.svelte";
  import DisksPanel from "./DisksPanel.svelte";
  import MemoryPanel from "./MemoryPanel.svelte";
  import NetworkPanel from "./NetworkPanel.svelte";
  import DashboardGrid from "./DashboardGrid.svelte";
  import GpuPanel from "./GpuPanel.svelte";
  import LoginsView from "./LoginsView.svelte";
  import PortTable from "./PortTable.svelte";
  import ProcessTable from "./ProcessTable.svelte";
  import TempsPanel from "./TempsPanel.svelte";
  import { baselineNote, isStale } from "$lib/hostsample";
  import { unsupportedReason } from "$lib/hostview";
  import { LIVE_MS, RANGES, loadPrefs, rangeMs, savePrefs, type ViewPrefs } from "$lib/hostviewprefs";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { hostSamples } from "$lib/stores/hostsamples.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Uuid } from "$lib/types";

  type Tab = "overview" | "cpu" | "memory" | "disks" | "network" | "gpu" | "temps" | "processes" | "ports" | "logins";
  const ALL_TABS: [Tab, string][] = [["overview", "Overview"], ["cpu", "CPU"], ["memory", "Memory"], ["disks", "Disks"], ["network", "Network"], ["gpu", "GPU"], ["temps", "Temperature"], ["processes", "Processes"], ["ports", "Ports"], ["logins", "Logins"]];

  let { id }: { id: Uuid } = $props();
  let hostId = $state<Uuid>(untrack(() => id));
  let tab = $state<Tab>("overview");
  let prefs = $state<ViewPrefs>(loadPrefs());
  let now = $state(Date.now());
  let sub: ReturnType<typeof hostSamples.subscribe> | null = null;
  let refreshing = $state(false);
  /** The dashboard layout is being edited: the host and the tab cannot change under it, and closing asks first. */
  let editing = $state(false);

  const host = $derived(vaultStore.hostById.get(hostId)?.data);
  const label = $derived(host?.label ?? "host");
  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.canMonitor(h.id)).sort((a, b) => (a.data!.label ?? "").localeCompare(b.data!.label ?? "")));
  const s = $derived(hostSamples.latest[hostId] ?? null);
  const frames = $derived(hostSamples.frames[hostId] ?? []);
  const error = $derived(hostSamples.errors[hostId] ?? null);
  const loading = $derived(!!hostSamples.loading[hostId] && !s);
  const windowMs = $derived(rangeMs(prefs.range));
  const age = $derived(hostSamples.receivedAt[hostId] ? Math.max(0, now - hostSamples.receivedAt[hostId]) : null);
  const stale = $derived(age !== null && isStale(age, prefs.live || 30_000));
  const canRead = $derived(hostMetrics.canMonitor(hostId));
  const unsupported = $derived(s ? unsupportedReason(s) : null);
  const note = $derived(s && !unsupported ? baselineNote(s.baseline) : null);
  // A tab for hardware the host does not have is not offered: no GPU tab for a host without a GPU, no temperatures for one
  // that exposes no sensors. The lists that are read on request (processes, ports, logins) are there for any Linux host.
  const TABS = $derived(ALL_TABS.filter(([t]) => (t === "gpu" ? !!s && s.gpus.length > 0 : t === "temps" ? !!s && s.capabilities.sensors : true)));
  $effect(() => {
    if (!TABS.some(([t]) => t === tab)) tab = "overview";
  });

  // One subscription per host this window is looking at; the store runs a single timer however many views share it.
  $effect(() => {
    const target = hostId;
    if (!canRead) return;
    // The store reads and writes its own state while starting; none of that is this effect's business.
    const sb = untrack(() => hostSamples.subscribe(target, prefs.live));
    sub = sb;
    return () => {
      sb();
      sub = null;
    };
  });
  $effect(() => {
    const ms = prefs.live;
    sub?.set(ms);
  });
  $effect(() => {
    savePrefs($state.snapshot(prefs));
  });
  onMount(() => {
    // The charts' right edge moves on between readings.
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const ageText = (ms: number) => (ms < 1500 ? "just now" : ms < 90_000 ? `${Math.round(ms / 1000)}s ago` : `${Math.round(ms / 60_000)} min ago`);

  async function refresh() {
    refreshing = true;
    try {
      await hostSamples.refresh(hostId, true);
    } finally {
      refreshing = false;
    }
  }

  function chooseIface(name: string) {
    prefs.ifaces = { ...prefs.ifaces, [hostId]: name };
  }

  const tabKey = (e: KeyboardEvent) => {
    const i = TABS.findIndex(([t]) => t === tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    tab = TABS[(to + TABS.length) % TABS.length][0];
    queueMicrotask(() => document.getElementById(`hm-tab-${tab}`)?.focus());
  };
</script>

<Modal title="Host Metrics · {label}" onclose={() => (ui.modal = null)} width="max-w-7xl" confirmClose={editing ? { ...DISCARD, message: "You are editing the dashboard layout. Close and discard the changes?" } : null}>
  <div class="flex h-[78vh] min-h-96 flex-col gap-3" data-testid="host-metrics">
    <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <label class="flex items-center gap-2 text-xs text-fg-muted">
        Host
        <select class="input h-8 w-52 py-0 text-xs" bind:value={hostId} aria-label="Host" disabled={editing} title={editing ? "Save or cancel the layout first" : ""}>
          {#if !hosts.some((h) => h.id === hostId)}<option value={hostId}>{label}</option>{/if}
          {#each hosts as h (h.id)}<option value={h.id}>{h.data?.label}</option>{/each}
        </select>
      </label>
      <div class="flex overflow-hidden rounded-md border border-line text-xs" role="tablist" aria-label="Host metrics sections">
        {#each TABS as [t, text] (t)}
          <button id="hm-tab-{t}" role="tab" aria-selected={tab === t} tabindex={tab === t ? 0 : -1} class="px-3 py-1 {tab === t ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'} disabled:cursor-not-allowed disabled:opacity-50" disabled={editing && tab !== t} onclick={() => (tab = t)} onkeydown={tabKey}>{text}</button>
        {/each}
      </div>
      <div class="ml-auto flex flex-wrap items-center gap-2 text-xs text-fg-muted">
        <span class="flex items-center gap-1.5" aria-live="polite">
          <span class="h-2 w-2 rounded-full {error ? 'bg-danger' : stale ? 'bg-warning' : prefs.live ? 'animate-pulse bg-success' : 'bg-fg-muted'}" aria-hidden="true"></span>
          {#if age !== null}Last update: {ageText(age)}{stale ? " (stale)" : ""}{:else if loading}Connecting…{:else}No reading yet{/if}
        </span>
        <label class="flex items-center gap-1.5">Live
          <select class="input h-7 w-24 py-0 text-xs" bind:value={prefs.live} aria-label="Live refresh">
            {#each LIVE_MS as ms (ms)}<option value={ms}>{ms ? `${ms / 1000} s` : "Off"}</option>{/each}
          </select>
        </label>
        <label class="flex items-center gap-1.5">Range
          <select class="input h-7 w-24 py-0 text-xs" bind:value={prefs.range} aria-label="Chart range">
            {#each RANGES as r (r.id)}<option value={r.id}>{r.label}</option>{/each}
          </select>
        </label>
        <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label="Speed unit">
          {#each [["bytes", "MB/s"], ["bits", "Mbps"]] as [u, text] (u)}
            <button class="px-2 py-0.5 {prefs.unit === u ? 'bg-accent text-white' : 'hover:bg-panel-hover'}" aria-pressed={prefs.unit === u} title={u === "bytes" ? "Megabytes per second" : "Megabits per second (8 times the number)"} onclick={() => (prefs.unit = u as ViewPrefs["unit"])}>{text}</button>
          {/each}
        </div>
        <button class="icon-btn h-7 w-7" title="Refresh now" aria-label="Refresh now" onclick={refresh} disabled={refreshing || !canRead}><RefreshCw size={14} class={refreshing ? "animate-spin" : ""} /></button>
        <button class="btn-secondary py-1 text-xs" title="The older monitor window: interfaces, system facts and the security audit" onclick={() => (ui.modal = { kind: "host-monitor", id: hostId })}><Activity size={13} /> Security audit</button>
      </div>
    </div>

    {#if !canRead}
      <p class="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning" role="alert">Host metrics are read without asking for a login, so {label} needs saved credentials (a saved password or key) first.</p>
    {/if}
    {#if error}
      <p class="flex items-start gap-2 whitespace-pre-wrap rounded-md border {s ? 'border-warning/40 bg-warning/10 text-warning' : 'border-danger/30 bg-danger/10 text-danger'} px-3 py-2 text-xs" role="alert">
        <AlertTriangle size={14} class="mt-0.5 shrink-0" />
        <span>{s ? `The last reading failed (${error}); showing the one before it.` : error}</span>
        <button class="btn-secondary ml-auto py-0.5 text-xs" onclick={refresh}>Try again</button>
      </p>
    {/if}
    {#if note}<p class="text-xs text-fg-muted">{note}</p>{/if}

    <div class="min-h-0 flex-1 overflow-auto pr-1" role="tabpanel" aria-labelledby="hm-tab-{tab}">
      {#if loading}
        <div class="flex h-full items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading {label}…</div>
      {:else if !s}
        <div class="flex h-full flex-col items-center justify-center gap-2 text-sm text-fg-muted">
          <p>{canRead ? "No reading yet." : "Nothing to show."}</p>
          {#if canRead}<button class="btn-secondary" onclick={refresh}>Read now</button>{/if}
        </div>
      {:else if unsupported}
        <p class="rounded-lg border border-line bg-panel p-4 text-sm text-fg-muted">{unsupported}</p>
      {:else if tab === "overview"}
        {#key hostId}<DashboardGrid {hostId} {s} {frames} {now} {windowMs} rateUnit={prefs.unit} goto={(t) => (tab = t)} bind:editing />{/key}
      {:else if tab === "processes"}
        {#key hostId}<ProcessTable {hostId} />{/key}
      {:else if tab === "ports"}
        {#key hostId}<PortTable {hostId} />{/key}
      {:else if tab === "logins"}
        {#key hostId}<LoginsView {hostId} />{/key}
      {:else if tab === "gpu"}
        <GpuPanel {s} {frames} {now} {windowMs} />
      {:else if tab === "temps"}
        <TempsPanel {s} {frames} {now} {windowMs} />
      {:else if tab === "cpu"}
        <CpuPanel {s} {frames} {now} {windowMs} />
      {:else if tab === "memory"}
        <MemoryPanel {s} {frames} {now} {windowMs} />
      {:else if tab === "disks"}
        <DisksPanel {s} {frames} {now} {windowMs} rateUnit={prefs.unit} />
      {:else}
        <NetworkPanel {s} {frames} {now} {windowMs} rateUnit={prefs.unit} saved={prefs.ifaces[hostId] ?? null} onchoose={chooseIface} />
      {/if}
    </div>
    <p class="text-[11px] text-fg-muted">Read over the SSH connection the app already keeps; nothing is installed on the host. Charts reach back up to an hour and are kept in memory only: locking the vault clears them. Longer history is not available yet.</p>
  </div>
</Modal>
