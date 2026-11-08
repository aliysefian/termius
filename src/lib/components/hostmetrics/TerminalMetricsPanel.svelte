<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Activity, AlertTriangle, ArrowDown, ArrowLeftRight, ArrowUp, PanelLeft, PanelRight, RefreshCw, X } from "lucide-svelte";
  import Spark from "./Spark.svelte";
  import UsageBar from "./UsageBar.svelte";
  import { formatUptime } from "$lib/hostmetrics";
  import { bytesText, cpuTemperature, gpuVramPct, isStale, memText, pctText, rateText, tempText } from "$lib/hostsample";
  import { loadPrefs as loadView } from "$lib/hostviewprefs";
  import { mainFilesystem, unsupportedReason } from "$lib/hostview";
  import { PANEL_MAX, PANEL_MIN, LIVE_CHOICES } from "$lib/terminalmetrics";
  import { hostDetails } from "$lib/stores/hostdetails.svelte";
  import { hostSamples } from "$lib/stores/hostsamples.svelte";
  import { terminalMetrics as tm } from "$lib/stores/terminalmetrics.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const WINDOW = 5 * 60_000;
  const unit = loadView().unit;
  let now = $state(Date.now());
  let dragging = $state(false);
  let startX = 0;
  let startW = 0;

  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const id = $derived(tm.hostId);
  // The top programs are read only while the panel is on screen (it is only built then), through the shared store.
  $effect(() => {
    const target = id;
    if (!target) return;
    return untrack(() => hostDetails.watch("processes", target));
  });
  const procs = $derived(id ? (hostDetails.processes[id]?.items ?? []).slice(0, 5) : []);
  const procsLoaded = $derived(id ? !!hostDetails.processes[id] : false);
  const label = $derived(id ? (vaultStore.hostById.get(id)?.data?.label ?? "host") : "");
  const s = $derived(id ? (hostSamples.latest[id] ?? null) : null);
  const frames = $derived(id ? (hostSamples.frames[id] ?? []) : []);
  const error = $derived(id ? (hostSamples.errors[id] ?? null) : null);
  const at = $derived(id ? hostSamples.receivedAt[id] : undefined);
  const age = $derived(at === undefined ? null : Math.max(0, now - at));
  const stale = $derived(age !== null && isStale(age, tm.every ?? 5000));
  const cpuTemp = $derived(s ? cpuTemperature(s) : null);
  const unsupported = $derived(s ? unsupportedReason(s) : null);
  const fs = $derived(s ? mainFilesystem(s.filesystems) : null);
  const total = $derived(s?.network?.total ?? null);
  const right = $derived(tm.prefs.side === "right");

  const ageText = (ms: number) => (ms < 1500 ? "just now" : ms < 90_000 ? `${Math.round(ms / 1000)}s ago` : `${Math.round(ms / 60_000)} min ago`);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startW = tm.prefs.width;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (dragging) tm.setWidth(startW + (right ? startX - e.clientX : e.clientX - startX));
  }
  function stop() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    // The handle is on the panel's inner edge: the arrow toward the terminal widens it.
    const toward = right ? "ArrowLeft" : "ArrowRight";
    const away = right ? "ArrowRight" : "ArrowLeft";
    if (e.key === toward) tm.setWidth(tm.prefs.width + step);
    else if (e.key === away) tm.setWidth(tm.prefs.width - step);
    else if (e.key === "Home" || e.key === "Enter") tm.resetWidth();
    else return;
    e.preventDefault();
  }
</script>

{#if id}
  <aside class="relative flex shrink-0 flex-col overflow-hidden bg-panel {right ? 'order-last border-l' : 'order-first border-r'} border-line" style:width="{tm.prefs.width}px" aria-label="Host metrics for {label}" data-testid="term-metrics-panel">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute top-0 z-20 h-full w-2 cursor-col-resize outline-none after:absolute after:top-0 after:h-full after:w-0.5 after:transition-colors {right ? 'left-0 after:left-0' : 'right-0 after:right-0'} {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the metrics panel"
      aria-valuenow={tm.prefs.width}
      aria-valuemin={PANEL_MIN}
      aria-valuemax={PANEL_MAX}
      tabindex="0"
      title="Drag to resize · double-click to reset"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={stop}
      onpointercancel={stop}
      onlostpointercapture={() => dragging && stop()}
      ondblclick={() => tm.resetWidth()}
      onkeydown={key}
    ></div>

    <header class="flex items-center gap-1 border-b border-line px-2.5 py-2">
      <Activity size={14} class="shrink-0 text-accent" />
      <div class="min-w-0 flex-1">
        <div class="truncate text-sm font-semibold" title={label}>{label}</div>
        <div class="flex items-center gap-1.5 text-[11px] text-fg-muted" aria-live="polite">
          <span class="h-1.5 w-1.5 rounded-full {error ? 'bg-danger' : stale ? 'bg-warning' : tm.every && tm.every < 5000 ? 'animate-pulse bg-success' : 'bg-success'}" aria-hidden="true"></span>
          {#if age !== null}{ageText(age)}{stale ? " (stale)" : ""}{:else if error}failed{:else}reading…{/if}
        </div>
      </div>
      <button class="icon-btn h-6 w-6" title="Refresh now" aria-label="Refresh now" onclick={() => id && void hostSamples.refresh(id, true)}><RefreshCw size={12} /></button>
      <button class="icon-btn h-6 w-6" title="Open the full host metrics" aria-label="Open the full host metrics" onclick={() => (ui.modal = { kind: "host-metrics", id })}><ArrowLeftRight size={12} /></button>
      <button class="icon-btn h-6 w-6" title={right ? "Move to the left side" : "Move to the right side"} aria-label={right ? "Move the panel to the left" : "Move the panel to the right"} onclick={() => tm.setSide(right ? "left" : "right")}>
        {#if right}<PanelLeft size={13} />{:else}<PanelRight size={13} />{/if}
      </button>
      <button class="icon-btn h-6 w-6" title="Close the panel (the toolbar stays)" aria-label="Close the metrics panel" onclick={() => tm.closePanel()}><X size={13} /></button>
    </header>

    <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3 text-xs">
      {#if error}
        <p class="flex items-start gap-1.5 whitespace-pre-wrap rounded-md border {s ? 'border-warning/40 bg-warning/10 text-warning' : 'border-danger/30 bg-danger/10 text-danger'} px-2 py-1.5" role="alert">
          <AlertTriangle size={13} class="mt-0.5 shrink-0" /><span>{s ? `The last reading failed (${error}); showing the one before it.` : error}</span>
        </p>
      {/if}
      {#if !s}
        <p class="py-6 text-center text-fg-muted">{error ? "" : "Reading the host…"}</p>
      {:else if unsupported}
        <p class="rounded-md border border-line p-2 text-fg-muted">{unsupported}</p>
      {:else}
        <section aria-label="CPU">
          <div class="flex items-baseline justify-between"><h3 class="font-semibold">CPU</h3><span class="text-lg font-semibold tabular-nums">{pctText(s.cpu?.total_pct ?? null)}</span></div>
          <Spark {frames} pick={(f) => f.cpu} {now} windowMs={WINDOW} slot={1} />
          {#if s.load}<div class="text-fg-muted">Load {s.load.m1.toFixed(2)} · {s.load.m5.toFixed(2)} · {s.load.m15.toFixed(2)}{s.cpu ? ` · ${s.cpu.logical} cores` : ""}</div>{/if}
        </section>
        <section aria-label="Memory">
          <div class="flex items-baseline justify-between"><h3 class="font-semibold">Memory</h3><span class="text-lg font-semibold tabular-nums">{pctText(s.memory?.used_pct ?? null)}</span></div>
          <UsageBar pct={s.memory?.used_pct ?? null} label="Memory" />
          {#if s.memory}
            <div class="mt-1 text-fg-muted">{memText(s.memory.used)} of {memText(s.memory.total)}{s.memory.swap_total > 0 ? ` · swap ${memText(s.memory.swap_used)}` : ""}</div>
          {/if}
        </section>
        <section aria-label="Disk">
          <div class="flex items-baseline justify-between"><h3 class="font-semibold">Disk {fs ? `(${fs.mount})` : ""}</h3><span class="text-lg font-semibold tabular-nums">{pctText(fs?.used_pct ?? null)}</span></div>
          <UsageBar pct={fs?.used_pct ?? null} label="Disk" />
          {#if fs}<div class="mt-1 text-fg-muted">{bytesText(fs.used)} of {bytesText(fs.total)}</div>{/if}
        </section>
        <section aria-label="Network">
          <h3 class="font-semibold">Network</h3>
          {#if total}
            <div class="mt-0.5 grid grid-cols-2 gap-2 tabular-nums">
              <div><div class="flex items-center gap-1 text-fg-muted"><ArrowDown size={11} /> RX</div><div class="text-sm font-semibold">{rateText(total.rx_bps, unit)}</div></div>
              <div><div class="flex items-center gap-1 text-fg-muted"><ArrowUp size={11} /> TX</div><div class="text-sm font-semibold">{rateText(total.tx_bps, unit)}</div></div>
            </div>
            <Spark {frames} pick={(f) => (f.rx === null && f.tx === null ? null : (f.rx ?? 0) + (f.tx ?? 0))} max={Math.max(1000, ...frames.map((f) => (f.rx ?? 0) + (f.tx ?? 0)))} {now} windowMs={WINDOW} slot={4} />
            <div class="text-fg-muted">Added from {total.from.join(", ")}</div>
          {:else}
            <p class="text-fg-muted">{s.network ? "Speeds appear from the second reading." : "This host does not report network counters."}</p>
          {/if}
        </section>
        {#if s.capabilities.sensors}
          <section aria-label="Temperature">
            <div class="flex items-baseline justify-between"><h3 class="font-semibold">Temperature</h3><span class="text-lg font-semibold tabular-nums">{tempText(cpuTemp)}</span></div>
            <div class="text-fg-muted">{cpuTemp !== null ? "CPU" : "No CPU sensor on this host"}</div>
          </section>
        {/if}
        {#each s.gpus.slice(0, 2) as g (g.id)}
          <section aria-label={g.name}>
            <div class="flex items-baseline justify-between gap-2"><h3 class="truncate font-semibold" title={g.name}>{g.name}</h3><span class="text-lg font-semibold tabular-nums">{pctText(g.util_pct)}</span></div>
            <UsageBar pct={g.util_pct} label="{g.name} utilization" tone={false} />
            <div class="mt-1 text-fg-muted">VRAM {pctText(gpuVramPct(g))} · {tempText(g.celsius)}{g.power_watts !== null ? ` · ${Math.round(g.power_watts)} W` : ""}</div>
          </section>
        {/each}
        <section aria-label="Top processes">
          <h3 class="font-semibold">Top processes</h3>
          {#if procs.length}
            <ul class="mt-1 space-y-0.5">
              {#each procs as p (p.pid)}
                <li class="grid grid-cols-[minmax(0,1fr)_3rem_3.5rem] gap-2 tabular-nums"><span class="truncate font-sans" title={p.command || p.name}>{p.name}</span><span class="text-right">{p.cpu_pct === null ? "—" : `${p.cpu_pct.toFixed(1)}%`}</span><span class="text-right text-fg-muted">{memText(p.rss)}</span></li>
              {/each}
            </ul>
          {:else}
            <p class="text-fg-muted">{procsLoaded ? "No processes." : "Reading…"}</p>
          {/if}
        </section>
        {#if s.uptime_secs !== null}<div class="flex justify-between border-t border-line pt-2"><span class="text-fg-muted">Uptime</span><span class="tabular-nums">{formatUptime(s.uptime_secs)}</span></div>{/if}
      {/if}
    </div>

    <footer class="flex items-center gap-2 border-t border-line px-2.5 py-1.5 text-[11px] text-fg-muted">
      <label class="flex items-center gap-1">Live
        <select class="input h-6 w-16 py-0 text-[11px]" value={tm.prefs.live} onchange={(e) => tm.setLive(Number(e.currentTarget.value) as (typeof LIVE_CHOICES)[number])} aria-label="Live refresh">
          {#each LIVE_CHOICES as ms (ms)}<option value={ms}>{ms ? `${ms / 1000} s` : "Off"}</option>{/each}
        </select>
      </label>
      <button class="btn-ghost ml-auto py-0 text-[11px]" title="Stop reading this host for the terminal" onclick={() => tm.turnOff()}>Turn off</button>
    </footer>
  </aside>
{/if}
