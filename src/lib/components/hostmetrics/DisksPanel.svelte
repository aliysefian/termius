<script lang="ts">
  import type { Frame } from "$lib/hostframes";
  import { bytesText, pctText, rateText, UNAVAILABLE, type HostSample, type RateUnit } from "$lib/hostsample";
  import MetricChart from "./MetricChart.svelte";
  import UsageBar from "./UsageBar.svelte";

  let { s, frames, now, windowMs, rateUnit }: { s: HostSample; frames: Frame[]; now: number; windowMs: number; rateUnit: RateUnit } = $props();
  let layers = $state(false);
  const disks = $derived(s.disks.filter((d) => layers || !d.virtual_device));
  const hidden = $derived(s.disks.length - s.disks.filter((d) => !d.virtual_device).length);
  const ms = (v: number | null) => (v === null ? UNAVAILABLE : `${v < 10 ? v.toFixed(2) : v.toFixed(1)} ms`);
  const iops = (v: number | null) => (v === null ? UNAVAILABLE : `${Math.round(v)}/s`);
</script>

<div class="space-y-3">
  <section class="rounded-lg border border-line bg-panel p-3" aria-labelledby="fs-h">
    <h3 id="fs-h" class="text-sm font-semibold">Filesystems <span class="font-normal text-fg-muted">· how full</span></h3>
    {#if s.filesystems.length === 0}
      <p class="py-4 text-center text-xs text-fg-muted">{s.capabilities.filesystems ? "No mounted filesystems to show." : "This host's disk usage could not be read (df)."}</p>
    {:else}
      <div class="overflow-x-auto">
        <table class="mt-2 w-full min-w-[40rem] text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">Filesystem</th><th class="pr-3 font-medium">Mounted on</th><th class="pr-3 font-medium">Type</th><th class="pr-3 text-right font-medium">Size</th><th class="pr-3 text-right font-medium">Used</th><th class="pr-3 text-right font-medium">Available</th><th class="w-44 font-medium">Use</th></tr></thead>
          <tbody>
            {#each s.filesystems as f (f.device + f.mount)}
              <tr class="border-t border-line/60">
                <td class="py-1.5 pr-3 font-mono">{f.device}</td>
                <td class="pr-3 font-mono">{f.mount}</td>
                <td class="pr-3">{f.fstype ?? UNAVAILABLE}</td>
                <td class="pr-3 text-right tabular-nums">{bytesText(f.total)}</td>
                <td class="pr-3 text-right tabular-nums">{bytesText(f.used)}</td>
                <td class="pr-3 text-right tabular-nums">{bytesText(f.available)}</td>
                <td><div class="flex items-center gap-2"><div class="flex-1"><UsageBar pct={f.used_pct} label="Use of {f.mount}" /></div><span class="w-12 text-right tabular-nums">{pctText(f.used_pct)}</span></div></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="mt-2 text-[11px] text-fg-muted">Use is shown the way <code>df</code> does: used out of used plus available, so space reserved for root is not counted as free.</p>
    {/if}
  </section>

  <section class="rounded-lg border border-line bg-panel p-3" aria-labelledby="io-h">
    <div class="flex flex-wrap items-baseline justify-between gap-2">
      <h3 id="io-h" class="text-sm font-semibold">Physical disks <span class="font-normal text-fg-muted">· activity</span></h3>
      {#if hidden > 0 || layers}
        <label class="flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={layers} /> Show RAID / device-mapper layers ({hidden})</label>
      {/if}
    </div>
    {#if disks.length === 0}
      <p class="py-4 text-center text-xs text-fg-muted">{s.capabilities.diskstats && s.capabilities.sys_block ? "No physical disks to show." : "This host does not report disk activity."}</p>
    {:else}
      <div class="overflow-x-auto">
        <table class="mt-2 w-full min-w-[44rem] text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">Device</th><th class="pr-3 font-medium">Model</th><th class="pr-3 text-right font-medium">Size</th><th class="pr-3 text-right font-medium">Read</th><th class="pr-3 text-right font-medium">Write</th><th class="pr-3 text-right font-medium">Read ops</th><th class="pr-3 text-right font-medium">Write ops</th><th class="pr-3 text-right font-medium" title="Share of the time the device had I/O in flight. Not how full it is, and not its capacity.">Busy</th><th class="text-right font-medium">Latency</th></tr></thead>
          <tbody>
            {#each disks as d (d.name)}
              <tr class="border-t border-line/60 tabular-nums {d.virtual_device ? 'text-fg-muted' : ''}">
                <td class="py-1.5 pr-3 font-mono">{d.name}{#if d.virtual_device}<span class="ml-1 font-sans text-[10px]">(layer)</span>{/if}</td>
                <td class="max-w-48 truncate pr-3" title={d.model ?? ""}>{d.model ?? UNAVAILABLE}{#if d.rotational !== null}<span class="ml-1 text-fg-muted">{d.rotational ? "HDD" : "SSD"}</span>{/if}</td>
                <td class="pr-3 text-right">{bytesText(d.size)}</td>
                <td class="pr-3 text-right">{rateText(d.read_bps, rateUnit)}</td>
                <td class="pr-3 text-right">{rateText(d.write_bps, rateUnit)}</td>
                <td class="pr-3 text-right">{iops(d.read_iops)}</td>
                <td class="pr-3 text-right">{iops(d.write_iops)}</td>
                <td class="pr-3 text-right">{pctText(d.busy_pct)}</td>
                <td class="text-right" title={d.await_ms === null && d.busy_pct !== null ? "No reads or writes in this interval" : ""}>{ms(d.await_ms)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="mt-2 text-[11px] text-fg-muted">Busy is how much of the time the disk had work in flight, not how full it is. Partitions are not listed (their traffic is the disk's), and RAID or device-mapper layers are left out of the totals below.</p>
    {/if}
  </section>

  <MetricChart title="Disk throughput (all physical disks)" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "Read", slot: 1, pick: (f) => f.diskRead }, { label: "Write", slot: 2, pick: (f) => f.diskWrite }]} peaks empty="Collecting readings… rates appear from the second reading." />
</div>
