<script lang="ts">
  import { ArrowDown, ArrowUp } from "lucide-svelte";
  import { TOTAL, ifacePick, peak, pickInterface, type Frame } from "$lib/hostframes";
  import { bytesText, rateText, UNAVAILABLE, type HostSample, type RateUnit } from "$lib/hostsample";
  import { KIND_LABEL, countedNote, sortedInterfaces } from "$lib/hostview";
  import MetricChart from "./MetricChart.svelte";

  let {
    s,
    frames,
    now,
    windowMs,
    rateUnit,
    saved = null,
    onchoose,
  }: { s: HostSample; frames: Frame[]; now: number; windowMs: number; rateUnit: RateUnit; saved?: string | null; onchoose?: (name: string) => void } = $props();

  const net = $derived(s.network);
  const names = $derived((net?.interfaces ?? []).map((i) => i.name));
  const choice = $derived(pickInterface(saved, names, net?.default_iface ?? null, !!net?.total));
  const iface = $derived(choice === TOTAL ? null : (net?.interfaces.find((i) => i.name === choice) ?? null));
  const rx = $derived(iface ? iface.rx_bps : (net?.total?.rx_bps ?? null));
  const tx = $derived(iface ? iface.tx_bps : (net?.total?.tx_bps ?? null));
  const pickRx = $derived(iface ? ifacePick(iface.name, "rx") : (f: Frame) => f.rx);
  const pickTx = $derived(iface ? ifacePick(iface.name, "tx") : (f: Frame) => f.tx);
  const seen = $derived(frames.filter((f) => f.t >= now - windowMs));
  const sinceBoot = $derived(iface ? [iface.rx_bytes, iface.tx_bytes] : net ? [net.interfaces.filter((i) => i.counted).reduce((a, i) => a + i.rx_bytes, 0), net.interfaces.filter((i) => i.counted).reduce((a, i) => a + i.tx_bytes, 0)] : [null, null]);
  let mode = $state<"combined" | "separate">("combined");
  const label = $derived(iface ? iface.name : "All counted interfaces");
</script>

{#if !net}
  <p class="rounded-lg border border-line bg-panel p-4 text-sm text-fg-muted">This host does not report network counters (<code>/proc/net/dev</code>).</p>
{:else}
  <div class="space-y-3">
    <div class="rounded-lg border border-line bg-panel p-4">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="text-sm font-semibold">Network <span class="font-normal text-fg-muted">· {label}</span></h3>
        <label class="flex items-center gap-2 text-xs text-fg-muted">
          Interface
          <select class="input h-7 w-44 py-0 text-xs" value={choice} onchange={(e) => onchoose?.(e.currentTarget.value)} aria-label="Network interface">
            {#if net.total}<option value={TOTAL}>All (total)</option>{/if}
            {#each sortedInterfaces(net.interfaces) as i (i.name)}<option value={i.name}>{i.name}{i.name === net.default_iface ? " (default route)" : ""} · {KIND_LABEL[i.kind]}</option>{/each}
          </select>
        </label>
      </div>
      <div class="mt-3 grid gap-4 sm:grid-cols-2">
        <div>
          <div class="flex items-center gap-1.5 text-xs text-fg-muted"><ArrowDown size={13} /> Download (RX)</div>
          <div class="text-3xl font-semibold tabular-nums">{rateText(rx, rateUnit)}</div>
          <div class="text-xs text-fg-muted">Peak <span class="tabular-nums">{rateText(peak(seen, pickRx), rateUnit)}</span> · Received since boot <span class="tabular-nums">{bytesText(sinceBoot[0])}</span></div>
        </div>
        <div>
          <div class="flex items-center gap-1.5 text-xs text-fg-muted"><ArrowUp size={13} /> Upload (TX)</div>
          <div class="text-3xl font-semibold tabular-nums">{rateText(tx, rateUnit)}</div>
          <div class="text-xs text-fg-muted">Peak <span class="tabular-nums">{rateText(peak(seen, pickTx), rateUnit)}</span> · Transmitted since boot <span class="tabular-nums">{bytesText(sinceBoot[1])}</span></div>
        </div>
      </div>
      {#if iface}
        <dl class="mt-3 flex flex-wrap gap-x-6 gap-y-1 border-t border-line/60 pt-2 text-xs">
          <div class="flex gap-1.5"><dt class="text-fg-muted">State</dt><dd>{iface.state}</dd></div>
          <div class="flex gap-1.5"><dt class="text-fg-muted">Type</dt><dd>{KIND_LABEL[iface.kind]}</dd></div>
          <div class="flex gap-1.5"><dt class="text-fg-muted">Link speed</dt><dd>{iface.link_mbps ? `${iface.link_mbps >= 1000 ? `${iface.link_mbps / 1000} Gbps` : `${iface.link_mbps} Mbps`}` : "Not reported"}</dd></div>
          <div class="flex gap-1.5"><dt class="text-fg-muted">MTU</dt><dd>{iface.mtu ?? UNAVAILABLE}</dd></div>
          <div class="flex gap-1.5"><dt class="text-fg-muted">Errors in / out</dt><dd class="tabular-nums">{iface.rx_errors} / {iface.tx_errors}</dd></div>
          <div class="flex gap-1.5"><dt class="text-fg-muted">Dropped in / out</dt><dd class="tabular-nums">{iface.rx_drops} / {iface.tx_drops}</dd></div>
          <div class="w-full text-fg-muted">{countedNote(iface)}</div>
        </dl>
      {:else if net.total}
        <p class="mt-3 border-t border-line/60 pt-2 text-xs text-fg-muted">Added from {net.total.from.join(", ")}. Bridges, tunnels, containers and the members of a bond are left out, because their traffic also crosses a real interface and would be counted twice.</p>
      {:else}
        <p class="mt-3 border-t border-line/60 pt-2 text-xs text-fg-muted">No total yet: it needs a speed for every real interface, and that comes from the second reading. Pick an interface to see its own figures.</p>
      {/if}
    </div>

    <div class="flex items-center justify-end gap-1 text-xs" role="group" aria-label="Chart layout">
      {#each [["combined", "Combined"], ["separate", "RX and TX apart"]] as [v, t] (v)}
        <button class="rounded-md border px-2 py-0.5 {mode === v ? 'border-accent bg-accent/15 text-accent' : 'border-line text-fg-muted hover:text-fg'}" aria-pressed={mode === v} onclick={() => (mode = v as typeof mode)}>{t}</button>
      {/each}
    </div>
    {#if mode === "combined"}
      <MetricChart title="Traffic · {label}" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "RX (download)", slot: 1, pick: pickRx }, { label: "TX (upload)", slot: 2, pick: pickTx }]} peaks height={180} empty="Collecting readings… speeds appear from the second reading." />
    {:else}
      <div class="grid gap-3 lg:grid-cols-2">
        <MetricChart title="Download (RX) · {label}" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "RX", slot: 1, pick: pickRx }]} peaks empty="Collecting readings…" />
        <MetricChart title="Upload (TX) · {label}" {frames} {now} {windowMs} {rateUnit} unit="rate" series={[{ label: "TX", slot: 2, pick: pickTx }]} peaks empty="Collecting readings…" />
      </div>
    {/if}

    <section class="rounded-lg border border-line bg-panel p-3" aria-labelledby="ifs-h">
      <h3 id="ifs-h" class="text-sm font-semibold">Interfaces</h3>
      <div class="overflow-x-auto">
        <table class="mt-2 w-full min-w-[40rem] text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">Name</th><th class="pr-3 font-medium">Type</th><th class="pr-3 font-medium">State</th><th class="pr-3 text-right font-medium">Download</th><th class="pr-3 text-right font-medium">Upload</th><th class="pr-3 text-right font-medium">Received</th><th class="pr-3 text-right font-medium">Sent</th><th class="text-right font-medium">Errors / drops</th></tr></thead>
          <tbody>
            {#each sortedInterfaces(net.interfaces) as i (i.name)}
              <tr class="border-t border-line/60 tabular-nums hover:bg-panel-hover/50 {i.name === choice ? 'bg-accent/5' : ''}">
                <td class="py-1.5 pr-3 font-mono"><button class="hover:text-accent" onclick={() => onchoose?.(i.name)} title={countedNote(i)}>{i.name}</button>{#if i.counted}<span class="ml-1 font-sans text-[10px] text-fg-muted">counted</span>{/if}</td>
                <td class="pr-3 font-sans">{KIND_LABEL[i.kind]}{i.master ? ` · in ${i.master}` : ""}</td>
                <td class="pr-3 font-sans">{i.state}</td>
                <td class="pr-3 text-right">{rateText(i.rx_bps, rateUnit)}</td>
                <td class="pr-3 text-right">{rateText(i.tx_bps, rateUnit)}</td>
                <td class="pr-3 text-right">{bytesText(i.rx_bytes)}</td>
                <td class="pr-3 text-right">{bytesText(i.tx_bytes)}</td>
                <td class="text-right">{i.rx_errors + i.tx_errors} / {i.rx_drops + i.tx_drops}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  </div>
{/if}
