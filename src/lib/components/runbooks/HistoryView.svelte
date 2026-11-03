<script lang="ts">
  import { History as HistoryIcon, RotateCw, Search, Trash2 } from "lucide-svelte";
  import RunConsole from "./RunConsole.svelte";
  import { duration } from "$lib/runbook";
  import { strategyLabel } from "$lib/runbookrun";
  import { ask } from "$lib/dialogs.svelte";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let { now }: { now: number } = $props();
  let query = $state("");
  let result = $state<"all" | "failed" | "ok">("all");

  $effect(() => {
    void rb.loadHistory();
  });

  const list = $derived(
    rb.history.filter((r) => {
      const q = query.trim().toLowerCase();
      if (q && !`${r.runbook} ${(r.failed_hosts ?? []).join(" ")}`.toLowerCase().includes(q)) return false;
      if (result === "failed") return r.failed > 0 || r.cancelled;
      if (result === "ok") return r.failed === 0 && !r.cancelled && !r.running;
      return true;
    }),
  );
  const stamp = (t: number) => new Date(t * 1000).toLocaleString();
  const trigger = (t: string | null | undefined) => (t === "schedule" ? "Scheduled" : t === "retry" ? "Retry" : "Manual");
  const rec = $derived(rb.viewing);

  async function remove(id: string) {
    if (await ask("Delete this run from the history?", { title: "Delete record", confirm: "Delete", danger: true })) await rb.deleteRecord(id);
  }

  const failedIds = $derived(rec?.hosts.filter((h) => h.ok === false).map((h) => h.host_id) ?? []);

  async function again(onlyFailed: boolean) {
    if (!rec) return;
    await rb.runAgainFromRecord(onlyFailed ? { ...rec, hosts: rec.hosts.filter((h) => h.ok === false) } : rec);
  }
</script>

<div class="flex h-full min-h-0" data-testid="history-view">
  <aside class="flex w-80 shrink-0 flex-col border-r border-line bg-panel" aria-label="Past runs">
    <div class="space-y-2 border-b border-line p-3">
      <div class="relative">
        <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input h-8 w-full pl-8 text-xs" type="search" placeholder="Search runs" aria-label="Search runs" bind:value={query} />
      </div>
      <div class="flex items-center gap-1" role="group" aria-label="Filter by result">
        {#each [["all", "All"], ["failed", "Failed"], ["ok", "Succeeded"]] as [id, label] (id)}
          <button class="rounded-md border px-2 py-0.5 text-[11px] {result === id ? 'border-accent/50 bg-accent/15 text-accent' : 'border-line text-fg-muted hover:bg-panel-hover'}" aria-pressed={result === id} onclick={() => (result = id as typeof result)}>{label}</button>
        {/each}
        <span class="ml-auto text-[11px] text-fg-muted">Kept on this computer</span>
        {#if rb.history.length}<button class="text-[11px] text-danger hover:underline" onclick={() => rb.clearHistory()}>Clear</button>{/if}
      </div>
    </div>
    <ul class="min-h-0 flex-1 overflow-y-auto" data-testid="history-list">
      {#each list as r (r.id)}
        <li class="group flex items-center {rec?.id === r.id ? 'bg-accent/15' : 'hover:bg-panel-hover'}">
          <button class="min-w-0 flex-1 px-3 py-2 text-left" aria-current={rec?.id === r.id} onclick={() => rb.openRecord(r.id)}>
            <span class="flex items-center gap-2"><span class="h-2 w-2 shrink-0 rounded-full {r.running ? 'bg-accent' : r.cancelled ? 'bg-warning' : r.failed ? 'bg-danger' : 'bg-success'}" aria-hidden="true"></span><span class="truncate text-sm font-medium">{r.runbook}</span></span>
            <span class="block truncate text-[11px] text-fg-muted">{stamp(r.started_at)} · {trigger(r.trigger)}{r.finished_at ? ` · ${duration((r.finished_at - r.started_at) * 1000)}` : ""}</span>
            <span class="block truncate text-[11px] {r.failed ? 'text-danger' : 'text-success'}">{r.running ? "running" : r.cancelled ? "cancelled · " : ""}{r.ok} of {r.hosts} succeeded{r.failed_hosts?.length ? ` · failed: ${r.failed_hosts.slice(0, 3).join(", ")}${r.failed_hosts.length > 3 ? "…" : ""}` : ""}</span>
          </button>
          <button class="icon-btn mr-1 h-6 w-6 opacity-0 focus:opacity-100 group-hover:opacity-100" aria-label="Delete this record" onclick={() => remove(r.id)}><Trash2 size={12} /></button>
        </li>
      {:else}
        <li class="px-4 py-8 text-center text-xs text-fg-muted">{rb.history.length ? "No run matches." : "Nothing has run yet. Runs started here and by schedules are listed."}</li>
      {/each}
    </ul>
  </aside>

  <section class="flex min-w-0 flex-1 flex-col" aria-label="Run details">
    {#if rec && rb.viewLive}
      <header class="space-y-2 border-b border-line px-4 py-3">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="text-[15px] font-semibold">{rec.runbook}</h2>
          <span class="rounded border border-line px-1.5 text-[11px] text-fg-muted">{trigger(rec.trigger)}</span>
          <div class="flex-1"></div>
          <button class="btn-secondary h-7 px-2 text-xs" onclick={() => again(false)} title="Open this runbook with the same hosts"><RotateCw size={12} /> Run again</button>
          <button class="btn-secondary h-7 px-2 text-xs" disabled={failedIds.length === 0} onclick={() => again(true)} title="Open this runbook with only the hosts that failed">Retry failed hosts</button>
        </div>
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1 text-xs sm:grid-cols-[6.5rem_1fr_6.5rem_1fr]">
          <dt class="text-fg-muted">Started</dt><dd>{stamp(rec.started_at)}</dd>
          <dt class="text-fg-muted">Duration</dt><dd>{rec.finished_at ? duration((rec.finished_at - rec.started_at) * 1000) : "still running"}{rec.cancelled ? " · cancelled" : ""}</dd>
          <dt class="text-fg-muted">Strategy</dt><dd>{rec.strategy ? strategyLabel(rec.strategy) : "Not recorded"}</dd>
          <dt class="text-fg-muted">Rollback</dt><dd>{rec.rollback ? "on" : "off"}</dd>
          {#if rec.from_step !== undefined}<dt class="text-fg-muted">Started at</dt><dd>step {rec.from_step + 1}</dd>{/if}
          {#if rec.retry_of}<dt class="text-fg-muted">Retry of</dt><dd class="truncate font-mono text-[11px]">{rec.retry_of}</dd>{/if}
          {#if Object.keys(rec.params).length}<dt class="text-fg-muted">Values</dt><dd class="sm:col-span-3 font-mono text-[11px]">{Object.entries(rec.params).map(([k, v]) => `${k} = ${v}`).join(", ")}</dd>{/if}
        </dl>
      </header>
      <div class="min-h-0 flex-1">
        <RunConsole live={rb.viewLive} title={rec.runbook} phase="record" past={true} {now} />
      </div>
    {:else}
      <div class="flex flex-1 items-center justify-center p-8 text-center">
        <div class="max-w-sm"><HistoryIcon size={26} class="mx-auto text-fg-muted" /><p class="mt-3 text-sm text-fg-muted">Choose a run to see what each host did, step by step. Output can contain secrets, so this record never leaves this computer, and secret values are hidden in it.</p></div>
      </div>
    {/if}
  </section>
</div>
