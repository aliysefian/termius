<script lang="ts">
  import type { Frame } from "$lib/hostframes";
  import { bytesText, gpuVramPct, pctText, tempText, UNAVAILABLE, type HostSample } from "$lib/hostsample";
  import { usageTone } from "$lib/hostview";
  import MetricChart from "./MetricChart.svelte";
  import UsageBar from "./UsageBar.svelte";

  let { s, frames, now, windowMs }: { s: HostSample; frames: Frame[]; now: number; windowMs: number } = $props();
  const watts = (w: number | null) => (w === null ? UNAVAILABLE : `${w.toFixed(w < 100 ? 1 : 0)} W`);
</script>

<div class="space-y-3" data-testid="gpu-panel">
  {#each s.notes as n (n)}<p class="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning" role="status">{n}</p>{/each}
  {#each s.gpus as g (g.id)}
    {@const vram = gpuVramPct(g)}
    <section class="rounded-lg border border-line bg-panel p-4" aria-label={g.name}>
      <div class="flex flex-wrap items-baseline gap-x-3">
        <h3 class="text-sm font-semibold">{g.name}</h3>
        <span class="text-xs text-fg-muted">{g.vendor === "nvidia" ? "NVIDIA" : "AMD"}{g.driver ? ` · driver ${g.driver}` : ""} · read with {g.vendor === "nvidia" ? "nvidia-smi" : "the amdgpu driver's files"}</span>
      </div>
      <div class="mt-3 grid gap-x-8 gap-y-3 md:grid-cols-2">
        <div>
          <div class="flex items-baseline justify-between"><span class="text-xs text-fg-muted">Utilization</span><span class="text-2xl font-semibold tabular-nums">{pctText(g.util_pct)}</span></div>
          <UsageBar pct={g.util_pct} label="{g.name} utilization" tone={false} />
          {#if g.mem_util_pct !== null}<div class="mt-1 text-xs text-fg-muted">Memory controller busy {pctText(g.mem_util_pct)}</div>{/if}
        </div>
        <div>
          <div class="flex items-baseline justify-between"><span class="text-xs text-fg-muted">VRAM</span><span class="text-2xl font-semibold tabular-nums">{pctText(vram)}</span></div>
          <UsageBar pct={vram} label="{g.name} video memory" />
          <div class="mt-1 text-xs text-fg-muted">{g.vram_used !== null && g.vram_total !== null ? `${bytesText(g.vram_used)} of ${bytesText(g.vram_total)}` : UNAVAILABLE}</div>
        </div>
      </div>
      <dl class="mt-3 flex flex-wrap gap-x-8 gap-y-1 border-t border-line/60 pt-2 text-xs">
        <div class="flex gap-1.5"><dt class="text-fg-muted">Temperature</dt><dd class="tabular-nums {usageTone(g.celsius, 80, 90) === 'bad' ? 'text-danger' : usageTone(g.celsius, 80, 90) === 'warn' ? 'text-warning' : ''}">{tempText(g.celsius)}</dd></div>
        <div class="flex gap-1.5"><dt class="text-fg-muted">Power</dt><dd class="tabular-nums">{watts(g.power_watts)}{g.power_limit_watts !== null ? ` / ${watts(g.power_limit_watts)} limit` : ""}</dd></div>
        <div class="flex gap-1.5"><dt class="text-fg-muted">Fan</dt><dd class="tabular-nums">{pctText(g.fan_pct)}</dd></div>
      </dl>
    </section>

    <div class="grid gap-3 lg:grid-cols-2">
      <MetricChart title="{g.name} · utilization" {frames} {now} {windowMs} unit="percent" series={[{ label: "GPU", slot: 1, pick: (f) => f.gpus[g.id]?.[0] ?? null }]} peaks height={130} empty="Collecting readings… the card is asked about every 5 seconds." />
      <MetricChart title="{g.name} · VRAM" {frames} {now} {windowMs} unit="percent" series={[{ label: "VRAM", slot: 2, pick: (f) => f.gpus[g.id]?.[1] ?? null }]} peaks height={130} />
    </div>

    {#if g.vendor === "nvidia"}
      <section class="rounded-lg border border-line bg-panel p-3">
        <h3 class="text-sm font-semibold">Programs using {g.name} <span class="font-normal text-fg-muted">· compute work only</span></h3>
        {#if g.processes.length}
          <table class="mt-2 w-full text-left text-xs">
            <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">PID</th><th class="pr-3 font-medium">Program</th><th class="text-right font-medium">VRAM</th></tr></thead>
            <tbody>{#each g.processes as p (p.pid)}<tr class="border-t border-line/60 tabular-nums"><td class="py-1.5 pr-3 font-mono">{p.pid}</td><td class="pr-3 font-sans">{p.name}</td><td class="text-right">{bytesText(p.vram)}</td></tr>{/each}</tbody>
          </table>
        {:else}
          <p class="py-2 text-xs text-fg-muted">No compute programs are using it. (Programs that only draw graphics are not listed by this query.)</p>
        {/if}
      </section>
    {/if}
  {/each}
</div>
