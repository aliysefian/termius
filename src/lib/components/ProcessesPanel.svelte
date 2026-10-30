<script lang="ts">
  // The busiest processes on the focused terminal's host, for the right panel. Read over its own monitor
  // session (the same script as the host detail view), every few seconds while the tab is in front.
  import { onDestroy } from "svelte";
  import { Loader2, Pause, Play, RefreshCw } from "lucide-svelte";
  import * as api from "$lib/api";
  import { DETAIL_SCRIPT, formatBytes, parseDetailOutput, sortProcesses, type HostDetail } from "$lib/hostdetail";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, isApiError, type Uuid } from "$lib/types";

  let { active }: { active: boolean } = $props();

  const tab = $derived(ui.activeTab);
  const target = $derived(tab?.panes.find((p) => p.id === tab.activePaneId)?.target);
  const hostId = $derived(target?.kind === "host" ? target.hostId : undefined);
  const can = $derived(!!hostId && hostMetrics.canMonitor(hostId));

  let sessionId = $state<Uuid | null>(null);
  let forHost: Uuid | null = null;
  let detail = $state<HostDetail | null>(null);
  let error = $state<string | null>(null);
  let sampling = $state(false);
  let paused = $state(false);
  let updatedAt = $state(0);
  const INTERVAL_MS = 5000;

  async function open(id: Uuid) {
    await close();
    forHost = id;
    try {
      const sid = await api.monitor.open(id);
      // The host changed while this opened: not wanted any more.
      if (forHost !== id) {
        void api.monitor.close(sid).catch(() => {});
        return;
      }
      sessionId = sid;
      error = null;
      await sample();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function close() {
    const sid = sessionId;
    sessionId = null;
    detail = null;
    forHost = null;
    if (sid) await api.monitor.close(sid).catch(() => {});
  }

  /** One reading; never overlaps itself. */
  async function sample() {
    if (!sessionId || sampling) return;
    sampling = true;
    try {
      const r = await api.monitor.exec(sessionId, DETAIL_SCRIPT, 30);
      if (r.code !== 0 && !r.stdout.includes("@@OS")) throw new Error(r.stderr.trim() || `the sampling command ended with status ${r.code ?? "?"}`);
      detail = parseDetailOutput(r.stdout);
      if (forHost) hostMetrics.recordDetail(forHost, detail);
      updatedAt = Date.now();
      error = null;
    } catch (e) {
      error = errorMessage(e);
      if (isApiError(e) && e.code === "no_session") {
        sessionId = null;
        if (forHost) void open(forHost);
      }
    } finally {
      sampling = false;
    }
  }

  // Follow the focused host while the tab is in front; let go of the session once it isn't.
  $effect(() => {
    if (!active || !hostId || !can) {
      if (sessionId) void close();
      return;
    }
    if (forHost !== hostId) void open(hostId);
  });
  $effect(() => {
    if (!sessionId || paused || !active) return;
    const t = setInterval(() => !document.hidden && void sample(), INTERVAL_MS);
    return () => clearInterval(t);
  });
  onDestroy(() => void close());

  const rows = $derived(detail ? sortProcesses(detail.processes, "cpu", "desc").slice(0, 40) : []);
  const note = $derived(detail?.unavailable.find((u) => u.panel === "processes")?.reason);
  const pct = (v: number | null) => (v === null ? "–" : `${Math.round(v)}%`);
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
  {#if !hostId}
    <p class="p-4 text-sm text-fg-muted">Processes are shown for a saved SSH host. Select one of those tabs.</p>
  {:else if !can}
    <p class="p-4 text-sm text-fg-muted">This host can't be sampled over SSH, so its processes aren't shown.</p>
  {:else}
    <div class="flex shrink-0 items-center gap-1 border-b border-line px-3 py-1.5 text-fg-muted">
      {#if detail}
        <span class="tabular-nums" title="CPU busy · memory used · load average">CPU {pct(detail.cpuPct)} · Mem {pct(detail.memPct)}{#if detail.load} · Load {detail.load.split(/\s+/).slice(0, 3).join(" ")}{/if}</span>
      {:else if !error}
        <span class="flex items-center gap-1.5"><Loader2 size={12} class="animate-spin" /> Reading…</span>
      {/if}
      <div class="flex-1"></div>
      <button class="icon-btn h-6 w-6" title={paused ? "Resume refreshing" : "Pause refreshing"} aria-pressed={paused} onclick={() => (paused = !paused)}>{#if paused}<Play size={12} />{:else}<Pause size={12} />{/if}</button>
      <button class="icon-btn h-6 w-6" title="Read again now" aria-label="Read again now" disabled={sampling || !sessionId} onclick={() => void sample()}><RefreshCw size={12} class={sampling ? "animate-spin" : ""} /></button>
    </div>
    {#if error}<p class="px-3 py-2 text-danger">{error}</p>{/if}
    {#if note}<p class="px-3 py-2 text-warning">{note}</p>{/if}
    {#if rows.length}
      <div class="grid shrink-0 grid-cols-[3.25rem_1fr_3.25rem_4rem] gap-2 px-3 py-1 text-fg-muted">
        <span class="text-right">PID</span><span>Command</span><span class="text-right">CPU%</span><span class="text-right">Memory</span>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto font-mono">
        {#each rows as p (p.pid)}
          <div class="grid grid-cols-[3.25rem_1fr_3.25rem_4rem] gap-2 px-3 py-[3px] hover:bg-panel-hover" title="{p.nameRaw} · {p.user}">
            <span class="text-right tabular-nums text-fg-muted">{p.pid}</span>
            <span class="flex min-w-0 items-baseline gap-1.5"><span class="truncate">{p.name}</span><span class="hidden shrink-0 text-[10px] text-fg-muted @min-[20rem]:inline">{p.user}</span></span>
            <span class="text-right tabular-nums {p.cpuPct >= 50 ? 'text-warning' : ''}">{p.cpuPct.toFixed(1)}</span>
            <span class="text-right tabular-nums text-fg-muted">{formatBytes(p.memKb * 1000)}</span>
          </div>
        {/each}
      </div>
      <p class="shrink-0 border-t border-line px-3 py-1 text-[10px] text-fg-muted">Top {rows.length} by CPU{updatedAt ? ` · read ${new Date(updatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}` : ""}. Stop a process from the host's detail view.</p>
    {/if}
  {/if}
</div>
