<script lang="ts">
  import { formatUptime } from "$lib/hostmetrics";
  import type { Frame } from "$lib/hostframes";
  import { pctText, type HostSample } from "$lib/hostsample";
  import { loadPerCpu } from "$lib/hostview";
  import MetricChart from "./MetricChart.svelte";
  import UsageBar from "./UsageBar.svelte";

  let { s, frames, now, windowMs }: { s: HostSample; frames: Frame[]; now: number; windowMs: number } = $props();
  const cpu = $derived(s.cpu);
  const info = $derived(s.info);
  let allCores = $state(false);
  const cores = $derived(cpu ? (allCores ? cpu.cores : cpu.cores.slice(0, 16)) : []);
  const mhz = (v: number | null) => (v === null ? "Unavailable" : v >= 1000 ? `${(v / 1000).toFixed(2)} GHz` : `${Math.round(v)} MHz`);
  const rows = $derived<[string, string | null][]>([
    ["Model", info.cpu_model],
    ["Sockets", info.sockets?.toString() ?? null],
    ["Physical cores", info.physical_cores?.toString() ?? null],
    ["Logical processors", (info.logical_cpus ?? cpu?.logical)?.toString() ?? null],
    ["Current frequency", cpu?.freq_mhz != null ? mhz(cpu.freq_mhz) : null],
    ["Base frequency", info.base_mhz != null ? mhz(info.base_mhz) : null],
    ["Maximum frequency", info.max_mhz != null ? mhz(info.max_mhz) : null],
    ["Virtualization", info.virtualization],
    ["Hypervisor", info.hypervisor],
    ...info.caches.map((c): [string, string] => [`${c.level} cache`, c.size]),
    ["Processes", s.processes?.processes?.toString() ?? null],
    ["Threads", s.processes?.threads?.toString() ?? null],
    ["Uptime", s.uptime_secs != null ? formatUptime(s.uptime_secs) : null],
  ]);
</script>

{#if !cpu}
  <p class="rounded-lg border border-line bg-panel p-4 text-sm text-fg-muted">This host does not report CPU counters (<code>/proc/stat</code>).</p>
{:else}
  <div class="grid gap-3 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
    <div class="space-y-3">
      <div class="rounded-lg border border-line bg-panel p-4">
        <div class="flex flex-wrap items-end gap-x-6 gap-y-2">
          <div>
            <div class="text-xs text-fg-muted">CPU usage</div>
            <div class="text-4xl font-semibold tabular-nums">{pctText(cpu.total_pct)}</div>
            <div class="text-xs text-fg-muted">{cpu.logical} logical{info.physical_cores ? ` · ${info.physical_cores} physical` : ""}</div>
          </div>
          <dl class="grid grid-cols-2 gap-x-5 text-xs">
            <dt class="text-fg-muted">I/O wait</dt><dd class="tabular-nums">{pctText(cpu.iowait_pct)}</dd>
            <dt class="text-fg-muted">Steal</dt><dd class="tabular-nums">{pctText(cpu.steal_pct)}</dd>
          </dl>
          {#if s.load}
            {@const per = loadPerCpu(s.load.m1, cpu.logical)}
            <dl class="ml-auto grid grid-cols-[auto_auto] gap-x-4 text-xs" aria-label="Load average">
              <dt class="text-fg-muted">Load 1 min</dt><dd class="tabular-nums">{s.load.m1.toFixed(2)}</dd>
              <dt class="text-fg-muted">5 min</dt><dd class="tabular-nums">{s.load.m5.toFixed(2)}</dd>
              <dt class="text-fg-muted">15 min</dt><dd class="tabular-nums">{s.load.m15.toFixed(2)}</dd>
              {#if per !== null}<dt class="text-fg-muted">Per CPU</dt><dd class="tabular-nums" title="Load per logical CPU: 1.00 means every CPU is busy all the time">{per.toFixed(2)}</dd>{/if}
            </dl>
          {/if}
        </div>
      </div>
      <MetricChart title="CPU usage" {frames} {now} {windowMs} unit="percent" series={[{ label: "CPU", slot: 1, pick: (f) => f.cpu }]} peaks empty="Collecting readings… the first one has no usage figure yet." />
      <div class="rounded-lg border border-line bg-panel p-3">
        <div class="mb-2 flex items-baseline justify-between"><h3 class="text-sm font-semibold">Per core</h3><span class="text-xs text-fg-muted">{cpu.cores.length} cores</span></div>
        <ul class="grid gap-x-6 gap-y-1.5 sm:grid-cols-2">
          {#each cores as c (c.index)}
            <li class="grid grid-cols-[3.5rem_1fr_3.5rem] items-center gap-2 text-xs">
              <span class="text-fg-muted">Core {c.index}</span>
              <UsageBar pct={c.pct} label="Core {c.index}" />
              <span class="text-right tabular-nums">{pctText(c.pct)}</span>
            </li>
          {/each}
        </ul>
        {#if cpu.cores.length > 16}
          <button class="btn-ghost mt-2 py-0.5 text-xs" onclick={() => (allCores = !allCores)}>{allCores ? "Show fewer" : `Show ${cpu.cores.length - 16} more`}</button>
        {/if}
      </div>
    </div>
    <div class="rounded-lg border border-line bg-panel p-3 self-start">
      <h3 class="mb-2 text-sm font-semibold">Processor</h3>
      <dl class="grid grid-cols-[8.5rem_1fr] gap-x-3 gap-y-1.5 text-xs">
        {#each rows as [k, v] (k)}
          <dt class="text-fg-muted">{k}</dt><dd class="break-words {v ? '' : 'text-fg-muted'}">{v ?? "Unavailable"}</dd>
        {/each}
      </dl>
      {#if !info.cpu_model && !info.logical_cpus}<p class="mt-2 text-[11px] text-fg-muted">Hardware facts are read once an hour; they appear with the first reading that carried them.</p>{/if}
    </div>
  </div>
{/if}
