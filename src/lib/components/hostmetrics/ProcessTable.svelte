<script lang="ts">
  import { onMount } from "svelte";
  import { AlertTriangle, ArrowDown, ArrowUp, ListTree, Loader2, OctagonX, RefreshCw, Skull } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import * as api from "$lib/api";
  import { defaultDir, processRows, signalInfo, signalQuestion, signalResult, type Dir, type ProcessSort } from "$lib/hostlists";
  import { baselineNote, memText, pctText, type ProcessInfo, type SignalName } from "$lib/hostsample";
  import { formatUptime } from "$lib/hostmetrics";
  import { hostDetails } from "$lib/stores/hostdetails.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { hostId }: { hostId: Uuid } = $props();
  const MAX_ROWS = 300;
  const label = $derived(vaultStore.hostById.get(hostId)?.data?.label ?? "host");
  const production = $derived(vaultStore.effectiveEnv(vaultStore.hostById.get(hostId)?.data) === "production");
  const list = $derived(hostDetails.processes[hostId] ?? null);
  const error = $derived(hostDetails.errors[`processes:${hostId}`] ?? null);
  const loading = $derived(!!hostDetails.loading[`processes:${hostId}`] && !list);

  let query = $state("");
  let tree = $state(false);
  let sort = $state<{ key: ProcessSort; dir: Dir }>({ key: "cpu", dir: "desc" });
  let busy = $state<Record<number, boolean>>({});
  let refreshing = $state(false);

  // Read while this table is on screen, and only then.
  onMount(() => hostDetails.watch("processes", hostId));

  const rows = $derived(list ? processRows(list.items, { query, sort: sort.key, dir: sort.dir, tree }) : []);
  const note = $derived(list ? baselineNote(list.baseline) : null);

  function sortBy(key: ProcessSort) {
    sort = sort.key === key ? { key, dir: sort.dir === "asc" ? "desc" : "asc" } : { key, dir: defaultDir(key) };
  }

  async function refresh() {
    refreshing = true;
    try {
      await hostDetails.refresh("processes", hostId, true);
    } finally {
      refreshing = false;
    }
  }

  /** Ask, then signal. The host checks the pid is still this program before sending; a timeout is never retried. */
  async function signal(p: ProcessInfo, sig: SignalName) {
    if (busy[p.pid]) return;
    const info = signalInfo(sig);
    const ok = await ask(signalQuestion(p, label, sig), { title: `${info.title} this process?`, confirm: info.title, danger: info.danger, requireText: production && info.danger ? label : undefined });
    if (!ok) return;
    busy[p.pid] = true;
    try {
      const r = signalResult(await api.hostSamples.signal(hostId, p.pid, p.start_ticks, p.name, sig), p, sig);
      ui.notify(r.kind, r.text);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      delete busy[p.pid];
      void hostDetails.refresh("processes", hostId, true);
    }
  }

  const HEADS: [ProcessSort | null, string, string][] = [["pid", "PID", "text-right"], ["user", "User", ""], ["cpu", "CPU %", "text-right"], ["mem", "Memory", "text-right"], ["age", "Age", "text-right"], ["name", "Command", ""]];
  const MORE: SignalName[] = ["HUP", "INT", "STOP", "CONT", "USR1", "USR2"];
</script>

<div class="flex h-full min-h-0 flex-col gap-2" data-testid="process-table">
  <div class="flex flex-wrap items-center gap-2">
    <input class="input h-8 w-64 text-xs" placeholder="Search name, command, user or pid…" aria-label="Search processes" bind:value={query} />
    <button class="btn-secondary py-1 text-xs {tree ? 'border-accent text-accent' : ''}" aria-pressed={tree} onclick={() => (tree = !tree)} title="Show each process under its parent"><ListTree size={13} /> Tree</button>
    <span class="text-xs text-fg-muted" aria-live="polite">{#if list}{rows.length === list.items.length ? `${list.items.length} processes` : `${rows.length} of ${list.items.length}`}{list.truncated ? ` (the busiest ${list.items.length} of ${list.total})` : ""}{/if}</span>
    <button class="icon-btn ml-auto h-7 w-7" title="Refresh now" aria-label="Refresh processes" onclick={refresh} disabled={refreshing}><RefreshCw size={14} class={refreshing ? "animate-spin" : ""} /></button>
  </div>

  {#if error}
    <p class="flex items-start gap-2 rounded-md border {list ? 'border-warning/40 bg-warning/10 text-warning' : 'border-danger/30 bg-danger/10 text-danger'} px-3 py-2 text-xs" role="alert"><AlertTriangle size={14} class="mt-0.5 shrink-0" /><span>{list ? `The last reading failed (${error}); showing the one before it.` : error}</span></p>
  {/if}
  {#if note}<p class="text-xs text-fg-muted">{note}</p>{/if}

  {#if loading}
    <div class="flex flex-1 items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading the processes…</div>
  {:else if list}
    <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-line bg-panel">
      <table class="w-full min-w-[46rem] text-left text-xs">
        <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
          <tr>
            {#each HEADS as [k, name, cls] (name)}
              <th class="px-2 py-1.5 font-medium {cls}" aria-sort={k && sort.key === k ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
                {#if k}<button class="inline-flex items-center gap-1 hover:text-fg" onclick={() => sortBy(k)}>{name}{#if sort.key === k}{#if sort.dir === "asc"}<ArrowUp size={11} />{:else}<ArrowDown size={11} />{/if}{/if}</button>{:else}{name}{/if}
              </th>
            {/each}
            <th class="px-2 py-1.5 text-right font-medium">Signal</th>
          </tr>
        </thead>
        <tbody>
          {#each rows.slice(0, MAX_ROWS) as { p, depth } (p.pid)}
            <tr class="border-t border-line/60 hover:bg-panel-hover/50">
              <td class="px-2 py-1 text-right font-mono text-fg-muted">{p.pid}</td>
              <td class="px-2 py-1">{p.user || "—"}</td>
              <td class="px-2 py-1 text-right tabular-nums" title={p.cpu_pct === null ? "Needs a second reading" : ""}>{p.cpu_pct === null ? "—" : p.cpu_pct.toFixed(1)}</td>
              <td class="px-2 py-1 text-right tabular-nums" title={p.mem_pct !== null ? `${pctText(p.mem_pct)} of the host's memory` : ""}>{memText(p.rss)}</td>
              <td class="px-2 py-1 text-right tabular-nums text-fg-muted">{p.age_secs === null ? "—" : formatUptime(p.age_secs)}</td>
              <td class="max-w-[28rem] truncate px-2 py-1" style="padding-left: {0.5 + depth * 1.1}rem" title={p.command || p.name}>
                {#if p.state === "Z"}<span class="mr-1 rounded bg-warning/20 px-1 text-[10px] text-warning" title="Zombie: it has ended and is waiting for its parent">zombie</span>{/if}{p.command || p.name}
              </td>
              <td class="px-2 py-1">
                <div class="flex justify-end gap-0.5">
                  <button class="icon-btn h-6 w-6" title="Terminate (SIGTERM): ask it to stop" aria-label="Terminate {p.name} {p.pid}" disabled={!!busy[p.pid] || p.pid <= 1} onclick={() => signal(p, "TERM")}><OctagonX size={13} /></button>
                  <button class="icon-btn h-6 w-6 hover:text-danger" title="Force kill (SIGKILL): stop it at once" aria-label="Force kill {p.name} {p.pid}" disabled={!!busy[p.pid] || p.pid <= 1} onclick={() => signal(p, "KILL")}><Skull size={13} /></button>
                  <select class="input h-6 w-[4.75rem] py-0 pl-1 pr-0 text-[11px]" aria-label="Other signals for {p.name} {p.pid}" disabled={!!busy[p.pid] || p.pid <= 1} value="" onchange={(e) => { const v = e.currentTarget.value as SignalName; e.currentTarget.value = ""; if (v) void signal(p, v); }}>
                    <option value="">More</option>
                    {#each MORE as s (s)}<option value={s}>{s}</option>{/each}
                  </select>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if rows.length === 0}<p class="py-8 text-center text-xs text-fg-muted">{list.items.length ? "No process matches." : "No processes."}</p>{/if}
      {#if rows.length > MAX_ROWS}<p class="py-2 text-center text-xs text-fg-muted">Showing the first {MAX_ROWS} of {rows.length}. Search or sort to find the rest.</p>{/if}
    </div>
    <p class="text-[11px] text-fg-muted">CPU is a share of one core over the time between two readings, so a busy multi-threaded program can pass 100. {list.commands ? "Command lines can hold what was typed, so they are shown here and never logged. " : "This host has no `ps`, so only program names are shown. "}Signals use your own rights on the host (nothing is elevated), and the host checks the program is still the same one before sending. Nothing is stopped automatically.</p>
  {/if}
</div>
