<script lang="ts">
  import type { Frame } from "$lib/hostframes";
  import { cpuTemperature, tempText, UNAVAILABLE, type HostSample, type TempKind, type TempSensor } from "$lib/hostsample";
  import { usageTone } from "$lib/hostview";
  import MetricChart from "./MetricChart.svelte";

  let { s, frames, now, windowMs }: { s: HostSample; frames: Frame[]; now: number; windowMs: number } = $props();
  const GROUPS: [TempKind[], string][] = [
    [["cpu_package", "cpu_core"], "Processor"],
    [["gpu"], "Graphics"],
    [["storage"], "Storage"],
    [["system"], "System and motherboard"],
    [["network", "battery", "other"], "Other"],
  ];
  const cpu = $derived(cpuTemperature(s));
  const groups = $derived(GROUPS.map(([kinds, name]) => ({ name, items: s.temperatures.filter((t) => kinds.includes(t.kind)) })).filter((g) => g.items.length));
  /** How close to its own limit a sensor is: against what the hardware says is high or critical, else a general scale. */
  function pct(t: TempSensor): number | null {
    if (t.celsius === null) return null;
    const top = t.critical ?? t.high ?? 100;
    return Math.max(0, Math.min(100, (t.celsius * 100) / top));
  }
  const tone = (t: TempSensor) => {
    if (t.celsius === null) return "none";
    if (t.critical !== null && t.celsius >= t.critical - 5) return "bad";
    if (t.high !== null && t.celsius >= t.high - 5) return "warn";
    return t.high === null && t.critical === null ? usageTone(t.celsius, 70, 85) : "ok";
  };
</script>

<div class="space-y-3" data-testid="temps-panel">
  <div class="rounded-lg border border-line bg-panel p-4">
    <div class="text-xs text-fg-muted">CPU temperature</div>
    <div class="text-3xl font-semibold tabular-nums">{tempText(cpu)}</div>
    <p class="mt-1 text-xs text-fg-muted">{cpu === null ? "No sensor on this host is the processor's own, so no CPU figure is shown (the others below are not the CPU's)." : "From the processor's own sensor: its package reading, or the hottest core where there is none."}</p>
  </div>
  <MetricChart title="CPU temperature" {frames} {now} {windowMs} unit="percent" series={[{ label: "CPU", slot: 2, pick: (f) => f.temp }]} peaks height={130} empty="No CPU temperature to chart yet." />
  {#each groups as g (g.name)}
    <section class="rounded-lg border border-line bg-panel p-3" aria-label={g.name}>
      <h3 class="mb-1 text-sm font-semibold">{g.name}</h3>
      <ul class="space-y-1.5">
        {#each g.items as t, i (t.chip + t.label + i)}
          {@const p = pct(t)}
          {@const k = tone(t)}
          <li class="grid grid-cols-[minmax(0,1fr)_7rem_4.5rem] items-center gap-3 text-xs">
            <span class="truncate" title="{t.chip} · {t.source}"><span class="font-medium">{t.label}</span> <span class="text-fg-muted">{t.chip}</span></span>
            <span class="h-1.5 overflow-hidden rounded-full bg-line/60" role="progressbar" aria-label="{t.label} against its limit" aria-valuenow={p === null ? undefined : Math.round(p)} aria-valuemin="0" aria-valuemax="100">{#if p !== null}<span class="block h-full rounded-full {k === 'bad' ? 'bg-danger' : k === 'warn' ? 'bg-warning' : 'bg-success'}" style="width: {p}%"></span>{/if}</span>
            <span class="text-right tabular-nums {k === 'bad' ? 'text-danger' : k === 'warn' ? 'text-warning' : ''}" title={t.high !== null || t.critical !== null ? `High ${tempText(t.high)} · critical ${tempText(t.critical)}` : "The hardware gives no limit"}>{t.celsius === null ? UNAVAILABLE : tempText(t.celsius)}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  <p class="text-[11px] text-fg-muted">Read from <code>/sys/class/hwmon</code> and <code>/sys/class/thermal</code>. A sensor is called the CPU's only when its driver is a CPU's; a value of 0 or an impossible one (what an unconnected sensor reports) is shown as unavailable.</p>
</div>
