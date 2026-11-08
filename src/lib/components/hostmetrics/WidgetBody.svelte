<script lang="ts">
  import { onMount } from "svelte";
  import { Cpu, HardDrive, MemoryStick, Network as NetIcon, Thermometer } from "lucide-svelte";
  import MetricChart from "./MetricChart.svelte";
  import StatCard from "./StatCard.svelte";
  import UsageBar from "./UsageBar.svelte";
  import { formatUptime } from "$lib/hostmetrics";
  import type { Frame } from "$lib/hostframes";
  import { agoText, sortPorts } from "$lib/hostlists";
  import { bytesText, cpuTemperature, gpuVramPct, memText, pctText, rateText, tempText, UNAVAILABLE, type HostSample, type RateUnit } from "$lib/hostsample";
  import { loadPerCpu, mainFilesystem, memoryBar } from "$lib/hostview";
  import type { Item } from "$lib/dashlayout";
  import { hostDetails } from "$lib/stores/hostdetails.svelte";
  import type { Uuid } from "$lib/types";

  type Tab = "overview" | "cpu" | "memory" | "disks" | "network" | "gpu" | "temps" | "processes" | "ports" | "logins";
  let { item, s, frames, now, windowMs, rateUnit, hostId, goto, height }: { item: Item; s: HostSample; frames: Frame[]; now: number; windowMs: number; rateUnit: RateUnit; hostId: Uuid; goto: (t: Tab) => void; height: number } = $props();

  // The widgets that read on request start reading when they are on the screen, and stop when they are not.
  onMount(() => (item.type === "processes" || item.type === "ports" || item.type === "logins" ? hostDetails.watch(item.type, hostId) : undefined));

  const chartH = $derived(Math.max(90, height - 64));
  const m = $derived(s.memory);
  const fs = $derived(mainFilesystem(s.filesystems));
  const total = $derived(s.network?.total ?? null);
  const gpu = $derived(s.gpus[0] ?? null);
  const cpuTemp = $derived(cpuTemperature(s));
  const info = $derived(s.info);
  const disks = $derived(s.disks.filter((d) => !d.virtual_device));
  const procs = $derived((hostDetails.processes[hostId]?.items ?? []).slice(0, Math.max(3, Math.floor((height - 60) / 26))));
  const ports = $derived(sortPorts(hostDetails.ports[hostId]?.items ?? [], "port", "asc"));
  const logins = $derived(hostDetails.logins[hostId] ?? null);
  const sysRows = $derived<[string, string | null][]>([
    ["Hostname", info.hostname],
    ["Operating system", info.os_name],
    ["Kernel", info.kernel],
    ["Architecture", info.arch],
    ["CPU", info.cpu_model],
    ["Cores", info.logical_cpus ? `${info.logical_cpus}${info.physical_cores ? ` (${info.physical_cores} physical)` : ""}` : null],
    ["Memory", m ? memText(m.total) : null],
    ["Uptime", s.uptime_secs != null ? formatUptime(s.uptime_secs) : null],
  ]);
  const bar = $derived(m ? memoryBar(m) : null);
  const box = "rounded-lg border border-line bg-panel p-3";
</script>

<div class="h-full min-h-0 w-full" data-widget={item.type} data-view={item.view}>
  {#if item.type === "cpu"}
    {#if item.view === "chart"}
      <MetricChart title="CPU usage" {frames} {now} {windowMs} unit="percent" series={[{ label: "CPU", slot: 1, pick: (f) => f.cpu }]} height={chartH} peaks />
    {:else if item.view === "cores"}
      <section class="{box} h-full overflow-auto" aria-label="CPU per core">
        <div class="mb-2 flex items-baseline justify-between"><h3 class="text-sm font-semibold">CPU per core</h3><span class="text-xs text-fg-muted">{pctText(s.cpu?.total_pct ?? null)} overall</span></div>
        <ul class="grid gap-x-5 gap-y-1.5 sm:grid-cols-2">
          {#each s.cpu?.cores ?? [] as c (c.index)}
            <li class="grid grid-cols-[3.2rem_1fr_3.2rem] items-center gap-2 text-xs"><span class="text-fg-muted">Core {c.index}</span><UsageBar pct={c.pct} label="Core {c.index}" /><span class="text-right tabular-nums">{pctText(c.pct)}</span></li>
          {:else}<li class="text-xs text-fg-muted">This host does not report CPU counters.</li>{/each}
        </ul>
      </section>
    {:else}
      <StatCard title="CPU" value={pctText(s.cpu?.total_pct ?? null)} sub={s.cpu ? `${s.cpu.logical} cores${s.load ? ` · load ${s.load.m1.toFixed(2)}${loadPerCpu(s.load.m1, s.cpu.logical) !== null ? ` (${loadPerCpu(s.load.m1, s.cpu.logical)!.toFixed(2)}/CPU)` : ""}` : ""}` : "Not reported by this host"} {frames} pick={(f) => f.cpu} {now} {windowMs} slot={1} onopen={() => goto("cpu")}>
        {#snippet icon()}<Cpu size={15} />{/snippet}
      </StatCard>
    {/if}
  {:else if item.type === "memory"}
    {#if item.view === "chart"}
      <MetricChart title="Memory usage" {frames} {now} {windowMs} unit="percent" series={[{ label: "Memory", slot: 1, pick: (f) => f.mem }]} height={chartH} peaks />
    {:else if item.view === "bar" && m && bar}
      <section class="{box} h-full overflow-auto" aria-label="Memory composition">
        <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Memory</h3><span class="text-xl font-semibold tabular-nums">{pctText(m.used_pct)}</span></div>
        <div class="mt-2 flex h-3 overflow-hidden rounded-full bg-line/60" role="img" aria-label="Memory: {pctText(m.used_pct)} in use"><div class="bg-accent" style="width: {bar.used * 100}%"></div><div style="width: {bar.reclaimable * 100}%; background: #3987e5"></div><div class="bg-success/70" style="width: {bar.free * 100}%"></div></div>
        <ul class="mt-2 space-y-0.5 text-xs">
          <li class="flex justify-between"><span class="text-fg-muted">In use</span><span class="tabular-nums">{memText(m.used)}</span></li>
          <li class="flex justify-between"><span class="text-fg-muted">Cache and buffers (reclaimable)</span><span class="tabular-nums">{memText(m.available - Math.min(m.free, m.available))}</span></li>
          <li class="flex justify-between"><span class="text-fg-muted">Free</span><span class="tabular-nums">{memText(Math.min(m.free, m.available))}</span></li>
          <li class="flex justify-between"><span class="text-fg-muted">Swap</span><span class="tabular-nums">{m.swap_total > 0 ? `${memText(m.swap_used)} / ${memText(m.swap_total)}` : "none"}</span></li>
        </ul>
      </section>
    {:else}
      <StatCard title="Memory" value={m ? pctText(m.used_pct) : UNAVAILABLE} sub={m ? `${memText(m.used)} / ${memText(m.total)}` : "Not reported by this host"} {frames} pick={(f) => f.mem} {now} {windowMs} slot={2} onopen={() => goto("memory")}>
        {#snippet icon()}<MemoryStick size={15} />{/snippet}
      </StatCard>
    {/if}
  {:else if item.type === "filesystems"}
    {#if item.view === "summary"}
      <StatCard title="Disk {fs ? `(${fs.mount})` : ''}" value={fs ? pctText(fs.used_pct) : UNAVAILABLE} sub={fs ? `${bytesText(fs.used)} / ${bytesText(fs.total)}` : "No filesystem to show"} {now} {windowMs} slot={3} onopen={() => goto("disks")}>
        {#snippet icon()}<HardDrive size={15} />{/snippet}
        {#if fs}<UsageBar pct={fs.used_pct} label="Use of {fs.mount}" />{/if}
      </StatCard>
    {:else if item.view === "table"}
      <section class="{box} h-full overflow-auto" aria-label="Filesystems">
        <h3 class="mb-1 text-sm font-semibold">Filesystems</h3>
        <table class="w-full text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-2 font-medium">Mount</th><th class="pr-2 font-medium">Type</th><th class="pr-2 text-right font-medium">Size</th><th class="pr-2 text-right font-medium">Used</th><th class="text-right font-medium">Use</th></tr></thead>
          <tbody>{#each s.filesystems as f (f.device + f.mount)}<tr class="border-t border-line/60 tabular-nums"><td class="py-1 pr-2 font-mono" title={f.device}>{f.mount}</td><td class="pr-2 font-sans">{f.fstype ?? UNAVAILABLE}</td><td class="pr-2 text-right">{bytesText(f.total)}</td><td class="pr-2 text-right">{bytesText(f.used)}</td><td class="text-right">{pctText(f.used_pct)}</td></tr>{/each}</tbody>
        </table>
      </section>
    {:else}
      <section class="{box} h-full overflow-auto" aria-label="Filesystems">
        <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Filesystems</h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("disks")}>Details</button></div>
        {#if s.filesystems.length === 0}
          <p class="py-6 text-center text-xs text-fg-muted">No mounted filesystems to show.</p>
        {:else}
          <ul class="mt-2 space-y-2 text-xs">
            {#each s.filesystems as f (f.device + f.mount)}
              <li><div class="flex justify-between gap-2"><span class="truncate font-mono" title="{f.device} · {f.fstype ?? ''}">{f.mount}</span><span class="tabular-nums text-fg-muted">{bytesText(f.used)} / {bytesText(f.total)} · {pctText(f.used_pct)}</span></div><UsageBar pct={f.used_pct} label="Use of {f.mount}" /></li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  {:else if item.type === "diskio"}
    {#if item.view === "chart"}
      <MetricChart title="Disk throughput" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "Read", slot: 1, pick: (f) => f.diskRead }, { label: "Write", slot: 2, pick: (f) => f.diskWrite }]} height={chartH} peaks />
    {:else}
      <section class="{box} h-full overflow-auto" aria-label="Physical disk I/O">
        <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Physical disks <span class="font-normal text-fg-muted">· activity</span></h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("disks")}>Details</button></div>
        {#if disks.length === 0}
          <p class="py-6 text-center text-xs text-fg-muted">This host does not report disk activity.</p>
        {:else}
          <table class="mt-1 w-full text-left text-xs">
            <thead class="text-fg-muted"><tr><th class="py-1 pr-2 font-medium">Device</th><th class="pr-2 text-right font-medium">Read</th><th class="pr-2 text-right font-medium">Write</th><th class="pr-2 text-right font-medium">Busy</th><th class="text-right font-medium">Latency</th></tr></thead>
            <tbody>{#each disks as d (d.name)}<tr class="border-t border-line/60 tabular-nums"><td class="py-1 pr-2 font-mono">{d.name}</td><td class="pr-2 text-right">{rateText(d.read_bps, rateUnit)}</td><td class="pr-2 text-right">{rateText(d.write_bps, rateUnit)}</td><td class="pr-2 text-right">{pctText(d.busy_pct)}</td><td class="text-right">{d.await_ms === null ? UNAVAILABLE : `${d.await_ms.toFixed(1)} ms`}</td></tr>{/each}</tbody>
          </table>
        {/if}
      </section>
    {/if}
  {:else if item.type === "network"}
    {#if item.view === "chart"}
      <MetricChart title="Network traffic" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "RX", slot: 1, pick: (f) => f.rx }, { label: "TX", slot: 2, pick: (f) => f.tx }]} height={chartH} peaks empty="Collecting readings… speeds appear from the second reading." />
    {:else if item.view === "interfaces"}
      <section class="{box} h-full overflow-auto" aria-label="Network interfaces">
        <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Interfaces</h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("network")}>Details</button></div>
        <table class="mt-1 w-full text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-2 font-medium">Name</th><th class="pr-2 text-right font-medium">Download</th><th class="text-right font-medium">Upload</th></tr></thead>
          <tbody>{#each s.network?.interfaces ?? [] as i (i.name)}<tr class="border-t border-line/60 tabular-nums"><td class="py-1 pr-2 font-mono">{i.name}{#if i.counted}<span class="ml-1 font-sans text-[10px] text-fg-muted">counted</span>{/if}</td><td class="pr-2 text-right">{rateText(i.rx_bps, rateUnit)}</td><td class="text-right">{rateText(i.tx_bps, rateUnit)}</td></tr>{/each}</tbody>
        </table>
      </section>
    {:else}
      <StatCard title="Network" value={total ? `↓ ${rateText(total.rx_bps, rateUnit)}` : UNAVAILABLE} sub={total ? `↑ ${rateText(total.tx_bps, rateUnit)}` : s.network ? "Speeds appear from the second reading" : "Not reported by this host"} {frames} pick={(f) => (f.rx === null && f.tx === null ? null : (f.rx ?? 0) + (f.tx ?? 0))} max={Math.max(1000, ...frames.map((f) => (f.rx ?? 0) + (f.tx ?? 0)))} {now} {windowMs} slot={4} onopen={() => goto("network")}>
        {#snippet icon()}<NetIcon size={15} />{/snippet}
      </StatCard>
    {/if}
  {:else if item.type === "gpu"}
    {#if item.view === "chart" && gpu}
      <MetricChart title="{gpu.name} · utilization" {frames} {now} {windowMs} unit="percent" series={[{ label: "GPU", slot: 1, pick: (f) => f.gpus[gpu.id]?.[0] ?? null }, { label: "VRAM", slot: 2, pick: (f) => f.gpus[gpu.id]?.[1] ?? null }]} height={chartH} />
    {:else if gpu}
      <StatCard title="GPU" value={pctText(gpu.util_pct)} sub="{gpu.name}{s.gpus.length > 1 ? ` + ${s.gpus.length - 1} more` : ''}{gpuVramPct(gpu) !== null ? ` · VRAM ${pctText(gpuVramPct(gpu))}` : ''}" {frames} pick={(f) => f.gpu} {now} {windowMs} slot={2} onopen={() => goto("gpu")}>
        {#snippet icon()}<Cpu size={15} />{/snippet}
      </StatCard>
    {/if}
  {:else if item.type === "temperature"}
    {#if item.view === "chart"}
      <MetricChart title="CPU temperature" {frames} {now} {windowMs} unit="percent" series={[{ label: "CPU", slot: 2, pick: (f) => f.temp }]} height={chartH} peaks empty="No CPU temperature to chart." />
    {:else if item.view === "list"}
      <section class="{box} h-full overflow-auto" aria-label="Temperature sensors">
        <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Temperature</h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("temps")}>All</button></div>
        <ul class="mt-1 space-y-1 text-xs">{#each s.temperatures as t, i (t.chip + t.label + i)}<li class="flex justify-between gap-2"><span class="truncate" title={t.chip}>{t.label}</span><span class="tabular-nums">{tempText(t.celsius)}</span></li>{/each}</ul>
      </section>
    {:else}
      <StatCard title="Temperature" value={tempText(cpuTemp)} sub={cpuTemp !== null ? "CPU" : "No CPU sensor on this host"} {frames} pick={(f) => f.temp} max={100} {now} {windowMs} slot={3} onopen={() => goto("temps")}>
        {#snippet icon()}<Thermometer size={15} />{/snippet}
      </StatCard>
    {/if}
  {:else if item.type === "uptime"}
    <section class="{box} h-full" aria-label="Uptime">
      <h3 class="text-sm font-semibold">Uptime</h3>
      <div class="mt-1 text-2xl font-semibold tabular-nums">{s.uptime_secs != null ? formatUptime(s.uptime_secs) : UNAVAILABLE}</div>
      <div class="text-xs text-fg-muted">{s.load ? `Load ${s.load.m1.toFixed(2)} · ${s.load.m5.toFixed(2)} · ${s.load.m15.toFixed(2)}` : ""}{s.processes?.processes != null ? ` · ${s.processes.processes} processes` : ""}</div>
    </section>
  {:else if item.type === "system"}
    <section class="{box} h-full overflow-auto" aria-label="System information">
      <h3 class="mb-2 text-sm font-semibold">System information</h3>
      <dl class="grid grid-cols-[6rem_1fr] gap-x-3 gap-y-1.5 text-xs">{#each sysRows as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="break-words {v ? '' : 'text-fg-muted'}">{v ?? UNAVAILABLE}</dd>{/each}</dl>
    </section>
  {:else if item.type === "processes"}
    <section class="{box} h-full overflow-hidden" aria-label="Top processes">
      <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Top processes</h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("processes")}>All</button></div>
      <table class="mt-1 w-full text-left text-xs">
        <thead class="text-fg-muted"><tr><th class="py-1 pr-2 text-right font-medium">PID</th><th class="pr-2 font-medium">User</th><th class="pr-2 text-right font-medium">CPU</th><th class="pr-2 text-right font-medium">Memory</th><th class="font-medium">Command</th></tr></thead>
        <tbody>{#each procs as p (p.pid)}<tr class="border-t border-line/60 tabular-nums"><td class="py-1 pr-2 text-right font-mono text-fg-muted">{p.pid}</td><td class="pr-2 font-sans">{p.user || "—"}</td><td class="pr-2 text-right">{p.cpu_pct === null ? "—" : `${p.cpu_pct.toFixed(1)}%`}</td><td class="pr-2 text-right">{memText(p.rss)}</td><td class="max-w-48 truncate font-sans" title={p.command || p.name}>{p.command || p.name}</td></tr>{:else}<tr><td colspan="5" class="py-4 text-center text-fg-muted">Reading…</td></tr>{/each}</tbody>
      </table>
    </section>
  {:else if item.type === "ports"}
    <section class="{box} h-full overflow-auto" aria-label="Listening ports">
      <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Listening ports <span class="font-normal text-fg-muted">({ports.length})</span></h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("ports")}>All</button></div>
      <table class="mt-1 w-full text-left text-xs">
        <thead class="text-fg-muted"><tr><th class="py-1 pr-2 font-medium">Proto</th><th class="pr-2 font-medium">Address</th><th class="pr-2 font-medium">Port</th><th class="font-medium">Process</th></tr></thead>
        <tbody>{#each ports as p (`${p.proto}|${p.address}|${p.port}`)}<tr class="border-t border-line/60"><td class="py-1 pr-2 uppercase">{p.proto}</td><td class="pr-2 font-mono">{p.address}</td><td class="pr-2 font-mono">{p.port}</td><td>{p.process ?? "—"}</td></tr>{:else}<tr><td colspan="4" class="py-4 text-center text-fg-muted">Reading…</td></tr>{/each}</tbody>
      </table>
    </section>
  {:else if item.type === "logins"}
    <section class="{box} h-full overflow-auto" aria-label="Recent logins">
      <div class="flex items-baseline justify-between"><h3 class="text-sm font-semibold">Recent logins</h3><button class="btn-ghost py-0 text-xs" onclick={() => goto("logins")}>All</button></div>
      <ul class="mt-1 space-y-1 text-xs">
        {#each logins?.recent.slice(0, 8) ?? [] as e, i (i)}<li class="flex justify-between gap-2"><span><span class="font-medium">{e.user}</span> <span class="font-mono text-fg-muted">{e.from ?? ""}</span></span><span class="text-fg-muted">{agoText(e.started_ms, logins?.taken_at_ms ?? 0) ?? e.started}</span></li>{:else}<li class="py-3 text-center text-fg-muted">{logins ? "No logins to show." : "Reading…"}</li>{/each}
      </ul>
    </section>
  {/if}
</div>
