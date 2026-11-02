<script lang="ts">
  import { onDestroy } from "svelte";
  import { ArrowDown, ArrowUp, Loader2, Radio, TriangleAlert } from "lucide-svelte";
  import NetChart from "./NetChart.svelte";
  import { formatRate, formatTotal, rangeAvailable, RANGES, sortInterfaces, sparkline, updatedAgo, utilization, type NetPoint } from "$lib/netdata";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { netStore } from "$lib/stores/netstore.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import type { HostRow } from "$lib/monitoringview";

  let { row, now, active }: { row: HostRow; now: number; /** Whether this tab is showing: the live reading runs only then. */ active: boolean } = $props();

  const id = $derived(row.id);
  const sshHost = $derived(row.target.ssh_host_id);
  const canLive = $derived(!!sshHost && hostMetrics.canMonitor(sshHost));
  const latest = $derived(netStore.latest[id]);
  const unit = $derived(settings.prefs.netUnit);
  const live = $derived(netStore.liveRunning && netStore.liveTarget === id);
  const range = $derived(RANGES.find((r) => r.id === settings.prefs.monitoringRange) ?? RANGES[1]);
  const keep = $derived(settings.prefs.metricsKeep);
  const longRange = $derived(range.ms > 3_600_000);

  // The interface being looked at: the main one unless another was picked.
  let picked = $state<string | null>(null);
  const ifaces = $derived(latest ? sortInterfaces(latest.interfaces, latest.headline) : []);
  const chosen = $derived(ifaces.find((i) => i.name === picked) ?? ifaces.find((i) => i.name === latest?.headline) ?? ifaces[0] ?? null);
  $effect(() => {
    void id;
    picked = null;
  });

  // Read the counters live while this tab is open on a host that can be reached over SSH.
  $effect(() => {
    if (active && canLive && sshHost) void netStore.startLive(id, sshHost, settings.prefs.monitoringLiveSecs);
    return () => {
      if (netStore.liveTarget === id) netStore.stopLive();
    };
  });

  const points = $derived<NetPoint[]>(chosen ? netStore.points(id, chosen.name) : []);
  const to = $derived(Math.max(now, latest?.at ?? 0));
  const from = $derived(to - range.ms);
  // Longer than the hour kept in memory: the history the person asked to keep (the main interface only).
  let kept = $state<NetPoint[]>([]);
  $effect(() => {
    if (!longRange || !sshHost || !rangeAvailable(range, keep).ok) {
      kept = [];
      return;
    }
    let stale = false;
    void hostMetrics.archived(sshHost, range.ms).then((s) => !stale && (kept = s.map((x) => ({ t: x.t, rx: x.rx, tx: x.tx }))));
    return () => (stale = true);
  });
  const chartPoints = $derived(longRange ? kept : points);
  const rangeNote = $derived(longRange ? rangeAvailable(range, keep) : { ok: true as const });
  const spark = (pick: (p: NetPoint) => number | null) => sparkline(points, pick, 120, 28, 40);
  const util = $derived(chosen ? utilization(Math.max(chosen.rx_bps ?? 0, chosen.tx_bps ?? 0), chosen.link_mbps) : null);
  const empty = $derived(
    !canLive && !latest ? "Network speed is read over SSH. Edit this monitor and choose a saved SSH host."
    : longRange && !rangeNote.ok ? (rangeNote.why ?? "")
    : longRange ? "No history has been kept for this range yet."
    : latest ? "Waiting for more readings…" : "Waiting for the first reading…",
  );

  onDestroy(() => {
    if (netStore.liveTarget === id) netStore.stopLive();
  });
</script>

<div class="space-y-5">
  {#if !sshHost}
    <div class="rounded-[10px] border border-warning/30 bg-warning/10 p-4 text-sm">
      <p class="flex items-center gap-2 font-medium text-warning"><TriangleAlert size={15} /> Network speed needs an SSH host</p>
      <p class="mt-1 text-xs text-fg-muted">Speed is read from the host's own interface counters over SSH. This monitor has no SSH host chosen.</p>
      <button class="btn-secondary mt-3" onclick={() => (ui.modal = { kind: "monitor-target", id })}>Edit this monitor</button>
    </div>
  {:else if !canLive}
    <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">This SSH host has no saved credential, so it can't be read unattended. Attach an identity to it.</p>
  {/if}

  <!-- The two numbers -->
  <section aria-label="Current network traffic" class="space-y-3">
    <div class="flex flex-wrap items-center gap-2">
      <h3 class="text-xs font-medium uppercase tracking-wide text-fg-muted">Network traffic · {row.target.label}</h3>
      <span class="ml-auto inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] {live ? 'border-success/30 text-success' : 'border-line text-fg-muted'}" title={live ? `Read every ${settings.prefs.monitoringLiveSecs} s while this tab is open` : "Read by the background check"}>
        {#if live}<Radio size={11} class="animate-pulse motion-reduce:animate-none" /> Live · {settings.prefs.monitoringLiveSecs} s{:else}Background{/if}
      </span>
    </div>

    <div class="grid grid-cols-2 gap-3">
      {#each [{ key: "rx", label: "Download / RX", icon: ArrowDown, color: "#38bdf8", v: chosen?.rx_bps ?? null, total: chosen?.rx_total ?? null, pick: (p: NetPoint) => p.rx, tint: "text-sky-300" }, { key: "tx", label: "Upload / TX", icon: ArrowUp, color: "#a78bfa", v: chosen?.tx_bps ?? null, total: chosen?.tx_total ?? null, pick: (p: NetPoint) => p.tx, tint: "text-violet-300" }] as c (c.key)}
        {@const r = formatRate(c.v, unit)}
        <div class="rounded-[10px] border border-line bg-base/50 p-3">
          <div class="flex items-center gap-1.5 text-[11px] uppercase tracking-wide text-fg-muted"><c.icon size={13} style="color:{c.color}" aria-hidden="true" /> {c.label}</div>
          <div class="mt-1 flex items-baseline gap-1.5 {c.tint}">
            <span class="font-mono text-[28px] font-semibold leading-8 tabular-nums" data-testid="net-{c.key}">{r.value}</span>
            <span class="text-sm text-fg-muted">{r.unit}</span>
          </div>
          <svg viewBox="0 0 120 28" class="mt-1.5 h-7 w-full" preserveAspectRatio="none" aria-hidden="true">
            {#if spark(c.pick)}<polyline points={spark(c.pick)} fill="none" stroke={c.color} stroke-width="1.4" vector-effect="non-scaling-stroke" />{/if}
          </svg>
          <div class="mt-1 text-[11px] text-fg-muted">{c.key === "rx" ? "Downloaded" : "Uploaded"} total <span class="font-mono tabular-nums text-fg">{formatTotal(c.total)}</span> <span title="Counted by the host's interface since the host started">since boot</span></div>
        </div>
      {/each}
    </div>

    <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-fg-muted">
      {#if chosen}
        <span>Interface <span class="font-mono text-fg">{chosen.name}</span></span>
        {#if util !== null}<span title="Of the link speed the driver reports ({chosen.link_mbps} Mbit/s)">{util < 1 ? "<1" : util.toFixed(0)}% of link</span>{/if}
        <span aria-live="polite">Updated {latest ? updatedAgo(latest.at, now) : "–"}</span>
      {:else}
        <span class="flex items-center gap-1.5">{#if canLive}<Loader2 size={12} class="animate-spin" />{/if} {netStore.liveError ?? "No reading yet."}</span>
      {/if}
      <span class="ml-auto">RX = received by this host's interface, TX = sent by it</span>
    </div>
    {#if netStore.liveError && live}<p class="text-xs text-danger" role="alert">{netStore.liveError}</p>{/if}
  </section>

  <!-- Controls -->
  <section class="flex flex-wrap items-center gap-2" aria-label="Chart settings">
    <select class="input h-8 w-auto py-0 text-xs" aria-label="Interface" bind:value={picked} disabled={!ifaces.length}>
      <option value={null}>{latest?.headline ? `Main · ${latest.headline}` : "Main interface"}</option>
      {#each ifaces.filter((i) => i.name !== latest?.headline) as i (i.name)}<option value={i.name}>{i.name}{i.physical ? "" : " (virtual)"}</option>{/each}
    </select>
    <div class="flex overflow-hidden rounded-md border border-line text-xs" role="group" aria-label="Range">
      {#each RANGES as r (r.id)}
        {@const a = rangeAvailable(r, keep)}
        <button class="px-2 py-1 {range.id === r.id ? 'bg-accent/20 text-accent' : 'text-fg-muted hover:bg-panel-hover'} disabled:opacity-40 disabled:hover:bg-transparent" aria-pressed={range.id === r.id} disabled={!a.ok} title={a.ok ? "" : a.why} onclick={() => (settings.prefs.monitoringRange = r.id)}>{r.label}</button>
      {/each}
    </div>
    <div class="flex overflow-hidden rounded-md border border-line text-xs" role="group" aria-label="Unit">
      <button class="px-2 py-1 {unit === 'bytes' ? 'bg-accent/20 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" aria-pressed={unit === "bytes"} title="Bytes per second: B/s, KB/s, MB/s" onclick={() => (settings.prefs.netUnit = "bytes")}>Bytes</button>
      <button class="border-l border-line px-2 py-1 {unit === 'bits' ? 'bg-accent/20 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" aria-pressed={unit === "bits"} title="Bits per second: Kbit/s, Mbit/s, Gbit/s (eight times the bytes)" onclick={() => (settings.prefs.netUnit = "bits")}>Bits</button>
    </div>
  </section>

  <NetChart points={chartPoints} {from} {to} {unit} {empty} short={range.ms <= 3_600_000} />
  {#if longRange && rangeNote.ok}<p class="text-[11px] text-fg-muted">Longer ranges come from the history you chose to keep (main interface only, one value per minute).</p>{/if}

  <!-- Every interface -->
  {#if ifaces.length}
    <section aria-label="Interfaces">
      <h3 class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Interfaces</h3>
      <div class="overflow-x-auto rounded-[10px] border border-line">
        <table class="w-full text-left text-xs">
          <thead class="bg-base/50 text-fg-muted"><tr><th class="px-2.5 py-1.5 font-medium">Name</th><th class="px-2 py-1.5 text-right font-medium">↓ RX</th><th class="px-2 py-1.5 text-right font-medium">↑ TX</th><th class="px-2 py-1.5 text-right font-medium">Total ↓ / ↑</th><th class="px-2 py-1.5 text-right font-medium" title="Errors and dropped packets since boot">Err / drop</th></tr></thead>
          <tbody>
            {#each ifaces as i (i.name)}
              <tr class="border-t border-line/60 {chosen?.name === i.name ? 'bg-accent/10' : 'hover:bg-panel-hover'}">
                <td class="px-2.5 py-1.5"><button class="flex items-center gap-1.5 font-mono hover:text-accent" onclick={() => (picked = i.name)} title="Show this interface in the chart"><span class="h-1.5 w-1.5 rounded-full {i.state === 'up' ? 'bg-success' : i.state === 'down' ? 'bg-danger' : 'bg-fg-muted/50'}" title="Link {i.state}"></span>{i.name}{#if !i.physical}<span class="font-sans text-[10px] text-fg-muted">virtual</span>{/if}{#if i.name === latest?.headline}<span class="rounded bg-accent/15 px-1 font-sans text-[10px] text-accent">main</span>{/if}</button></td>
                <td class="px-2 py-1.5 text-right font-mono tabular-nums">{formatRate(i.rx_bps, unit).value} <span class="text-fg-muted">{formatRate(i.rx_bps, unit).unit}</span></td>
                <td class="px-2 py-1.5 text-right font-mono tabular-nums">{formatRate(i.tx_bps, unit).value} <span class="text-fg-muted">{formatRate(i.tx_bps, unit).unit}</span></td>
                <td class="px-2 py-1.5 text-right font-mono tabular-nums text-fg-muted">{formatTotal(i.rx_total)} / {formatTotal(i.tx_total)}</td>
                <td class="px-2 py-1.5 text-right font-mono tabular-nums {i.rx_errors + i.tx_errors + i.rx_drops + i.tx_drops ? 'text-warning' : 'text-fg-muted'}">{i.rx_errors + i.tx_errors} / {i.rx_drops + i.tx_drops}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
</div>
