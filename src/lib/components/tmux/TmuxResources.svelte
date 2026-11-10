<script lang="ts">
  // CPU, memory and processes of the selected session, from the host's /proc (see the engine's resources module). An estimate from
  // process trees, never cgroup accounting; with nothing measured it says "Unavailable", not zero.
  import { Cpu, MemoryStick } from "lucide-svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { bytes, type Session, type Usage } from "$lib/tmux/monitor";
  import Spark from "./Spark.svelte";

  let { server, session, compact = false, onall }: { server: string; session: Session; compact?: boolean; onall?: () => void } = $props();

  const res = $derived(mon.resources[server]);
  const err = $derived(mon.resourceError[server]);
  const su = $derived(res?.sessions.find((s) => s.session_id === session.id));
  const usage = $derived<Usage | undefined>(su?.usage ?? undefined);
  const hist = $derived(mon.history[`${server}|${session.id}`]);
  const cpu = $derived(usage?.cpu_pct == null ? null : `${usage.cpu_pct.toFixed(1)}%`);
  const unavailable = $derived(err ? `Unavailable: ${err}` : res ? "Unavailable: none of this session's processes could be seen from this user." : "Measuring…");
  const panes = $derived(session.windows.flatMap((w) => w.panes.map((p) => ({ w, p, u: res?.panes[p.id] }))));
</script>

{#if usage}
  <div class="grid gap-2 {compact ? 'grid-cols-2' : 'grid-cols-1 sm:grid-cols-3'}">
    <div class="rounded-lg border border-line bg-base p-2.5">
      <div class="flex items-center gap-1.5 text-[11px] text-fg-muted"><Cpu size={13} /> CPU</div>
      <div class="mt-0.5 text-[1rem] font-semibold tabular-nums">{cpu ?? "…"}</div>
      <Spark series={hist?.cpu ?? []} label="CPU" />
      {#if cpu === null}<div class="text-[10px] text-fg-muted">{res?.cpu_note ?? "Needs two readings."}</div>{/if}
    </div>
    <div class="rounded-lg border border-line bg-base p-2.5">
      <div class="flex items-center gap-1.5 text-[11px] text-fg-muted"><MemoryStick size={13} /> Memory</div>
      <div class="mt-0.5 text-[1rem] font-semibold tabular-nums">{bytes(usage.rss)}</div>
      <Spark series={hist?.mem ?? []} tone="success" label="Memory" />
    </div>
    {#if !compact}
      <div class="rounded-lg border border-line bg-base p-2.5">
        <div class="text-[11px] text-fg-muted">Processes</div>
        <div class="mt-0.5 text-[1rem] font-semibold tabular-nums">{usage.processes}</div>
        <div class="truncate font-mono text-[11px] text-fg-muted" title={usage.top.map(([n, c]) => `${n} ×${c}`).join(", ")}>{usage.top.map(([n, c]) => (c > 1 ? `${n} ×${c}` : n)).join(", ")}</div>
      </div>
    {/if}
  </div>
  {#if compact}
    <div class="mt-2 flex items-center justify-between text-xs">
      <span class="text-fg-muted">Processes <span class="ml-2 font-medium tabular-nums text-fg">{usage.processes}</span></span>
      {#if onall}<button class="rounded-md border border-line px-2 py-1 hover:bg-panel-hover" onclick={onall}>View Processes</button>{/if}
    </div>
  {/if}
  {#if su && su.panes_seen < su.panes_total}<p class="mt-1.5 text-[11px] text-warning">{su.panes_total - su.panes_seen} of {su.panes_total} panes could not be measured.</p>{/if}
  {#if !compact}
    <table class="mt-4 w-full text-left text-xs">
      <thead class="text-[11px] text-fg-muted"><tr><th class="py-1 font-medium">Pane</th><th class="font-medium">Window</th><th class="text-right font-medium">Processes</th><th class="text-right font-medium">CPU</th><th class="text-right font-medium">Memory</th><th class="pl-3 font-medium">Running</th></tr></thead>
      <tbody>
        {#each panes as r (r.p.id)}
          <tr class="border-t border-line/60">
            <td class="py-1 font-mono">{r.p.id}</td><td class="truncate">{r.w.index}: {r.w.name}</td>
            {#if r.u}
              <td class="text-right tabular-nums">{r.u.processes}</td><td class="text-right tabular-nums">{r.u.cpu_pct === null ? "…" : `${r.u.cpu_pct.toFixed(1)}%`}</td><td class="text-right tabular-nums">{bytes(r.u.rss)}</td>
              <td class="max-w-48 truncate pl-3 font-mono text-fg-muted">{r.u.top.map(([n, c]) => (c > 1 ? `${n} ×${c}` : n)).join(", ")}</td>
            {:else}
              <td class="text-right text-fg-muted" colspan="3">Unavailable</td><td></td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <p class="mt-2 text-[11px] text-fg-muted">An estimate from the processes under the pane shells: resident memory summed per process, so shared pages count in each. Not container or cgroup accounting.</p>
{:else}
  <p class="rounded-lg border border-dashed border-line p-3 text-xs text-fg-muted" role="status">{unavailable}</p>
{/if}
