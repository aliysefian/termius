<script lang="ts">
  import { Check, ChevronDown, ChevronUp, CircleDashed, CircleStop, Loader2, MinusCircle, RotateCw, Timer, X } from "lucide-svelte";
  import LogView from "./LogView.svelte";
  import { duration } from "$lib/runbook";
  import { elapsed, tally, timeline, type HostRun, type RunLive, type RunPhase } from "$lib/runbookrun";
  import { runbooks as rb, type ConsoleTab } from "$lib/stores/runbooks.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  /** The console of the run that is going (or just ended), or of a past run read from the history. */
  let { live, title, phase, past = false, now }: { live: RunLive; title: string; phase: RunPhase | "record"; past?: boolean; now: number } = $props();

  let selectedHost = $state<string | null>(null);
  let open = $state<Record<string, boolean>>({});
  const t = $derived(tally(live.hosts));
  const host = $derived(live.hosts.find((h) => h.hostId === selectedHost) ?? live.hosts[0] ?? null);
  const tab = $derived(rb.consoleTab);
  const TABS: { id: ConsoleTab; label: string }[] = [
    { id: "logs", label: "Live logs" },
    { id: "output", label: "Output" },
    { id: "summary", label: "Summary" },
  ];
  const running = $derived(!past && (phase === "running" || phase === "starting" || phase === "cancelling"));
  const ended = $derived(!past && (phase === "completed" || phase === "failed" || phase === "cancelled"));
  const failedAny = $derived(t.failed + t.skipped + t.cancelled > 0);
  const statusText: Record<string, string> = { running: "Running", starting: "Starting", cancelling: "Cancelling", completed: "Completed", failed: "Failed", cancelled: "Cancelled", record: "Finished", idle: "Idle", validating: "Validating" };

  const label: Record<HostRun["state"], string> = { pending: "Pending", connecting: "Connecting", running: "Running", success: "Success", failed: "Failed", skipped: "Skipped", cancelled: "Cancelled", timed_out: "Timed out" };
  const hostTone = (s: HostRun["state"]) => (s === "success" ? "text-success" : s === "failed" || s === "timed_out" ? "text-danger" : s === "running" || s === "connecting" ? "text-accent" : "text-fg-muted");
  const stepTone = (s: string) => (s === "ok" ? "text-success" : s === "failed" || s === "error" || s === "timed_out" ? "text-danger" : s === "running" ? "text-accent" : "text-fg-muted");
  const stepLabel: Record<string, string> = { ok: "Done", failed: "Failed", error: "Error", timed_out: "Timed out", skipped: "Skipped", running: "Running", pending: "Pending" };

  /** How long a host has been going, for a host still running. */
  const hostTime = (h: HostRun) => (h.durationMs !== null ? elapsed(h.durationMs) : h.startedAt && (h.state === "running" || h.state === "connecting") ? elapsed(now - h.startedAt) : "");

  const retries = $derived(ended ? { failed: rb.retryPlan("failed"), step: rb.retryPlan("from-failed-step"), all: rb.retryPlan("all") } : null);
</script>

{#snippet hostIcon(s: HostRun["state"])}
  {#if s === "running" || s === "connecting"}<Loader2 size={13} class="animate-spin text-accent motion-reduce:animate-none" />
  {:else if s === "success"}<Check size={13} class="text-success" />
  {:else if s === "failed" || s === "timed_out"}<X size={13} class="text-danger" />
  {:else if s === "skipped" || s === "cancelled"}<MinusCircle size={13} class="text-fg-muted" />
  {:else}<CircleDashed size={13} class="text-fg-muted" />{/if}
{/snippet}

<section class="flex h-full min-h-0 flex-col bg-panel" aria-label="Execution output" data-testid="run-console">
  <header class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line px-3 py-1.5">
    <h3 class="min-w-0 truncate text-sm font-semibold">{past ? "Run" : "Execution"}: {title}</h3>
    <span class="rounded-full border px-2 py-0.5 text-[11px] font-medium {phase === 'completed' || phase === 'record' && !failedAny ? 'border-success/40 text-success' : phase === 'failed' || phase === 'cancelled' || (phase === 'record' && failedAny) ? 'border-danger/40 text-danger' : 'border-accent/40 text-accent'}" role="status" data-testid="run-status">{statusText[phase] ?? phase}</span>
    <span class="text-xs text-fg-muted" data-testid="run-tally">{t.done} of {t.total} hosts done{t.success ? ` · ${t.success} succeeded` : ""}{t.failed ? ` · ${t.failed} failed` : ""}{t.skipped ? ` · ${t.skipped} skipped` : ""}{t.cancelled ? ` · ${t.cancelled} cancelled` : ""}{t.running ? ` · ${t.running} running` : ""}{t.pending ? ` · ${t.pending} pending` : ""}</span>
    <div class="ml-auto flex items-center gap-1.5">
      {#if running}
        <button class="btn-danger h-7 border border-danger/40 px-2 text-xs" disabled={phase === "cancelling"} onclick={() => rb.cancel()}><CircleStop size={12} /> {phase === "cancelling" ? "Cancelling…" : "Cancel"}</button>
      {/if}
      {#if retries}
        <div class="flex items-center gap-1" role="group" aria-label="Run again">
          <button class="btn-secondary h-7 px-2 text-xs" disabled={!retries.failed} title={retries.failed ? "Run the whole runbook again on the hosts that did not succeed" : "Every host succeeded"} onclick={() => rb.retry("failed")} data-testid="retry-failed"><RotateCw size={12} /> Retry failed hosts</button>
          <button class="btn-secondary h-7 px-2 text-xs" disabled={!retries.step} title={retries.step ? `Start the failed hosts again at step ${(retries.step.fromStep ?? 0) + 1}. Earlier steps are skipped.` : "No step failed"} onclick={() => rb.retry("from-failed-step")}>From the failed step</button>
          <button class="btn-ghost h-7 px-2 text-xs" disabled={!retries.all} onclick={() => rb.retry("all")}>Rerun all</button>
        </div>
      {/if}
      {#if past}<span class="text-[11px] text-fg-muted">From the history. Kept on this computer only.</span>{/if}
      <button class="icon-btn h-7 w-7" title="Hide the console" aria-label="Hide the console" onclick={() => (settings.prefs.rbOutputOpen = false)}><ChevronDown size={14} /></button>
    </div>
  </header>

  <div class="flex min-h-0 flex-1">
    <ul class="w-60 shrink-0 overflow-y-auto border-r border-line py-1" aria-label="Hosts" data-testid="console-hosts">
      {#each live.hosts as h (h.hostId)}
        <li>
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none {host?.hostId === h.hostId ? 'bg-accent/10' : ''}" aria-current={host?.hostId === h.hostId} onclick={() => { selectedHost = h.hostId; }}>
            {@render hostIcon(h.state)}
            <span class="min-w-0 flex-1">
              <span class="block truncate font-medium">{h.label}</span>
              <span class="block truncate text-[11px] {hostTone(h.state)}">{label[h.state]}{#if h.current !== null && live.stepNames[h.current]} · {live.stepNames[h.current]}{/if}</span>
            </span>
            <span class="shrink-0 font-mono text-[11px] text-fg-muted">{hostTime(h)}</span>
          </button>
        </li>
      {/each}
    </ul>

    <div class="flex min-w-0 flex-1 flex-col">
      <div class="flex gap-1 border-b border-line px-2" role="tablist" aria-label="Console views">
        {#each TABS as x (x.id)}
          <button role="tab" aria-selected={tab === x.id} class="-mb-px border-b-2 px-3 py-1.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent {tab === x.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}" onclick={() => (rb.consoleTab = x.id)}>{x.label}</button>
        {/each}
      </div>
      <div class="min-h-0 flex-1" role="tabpanel">
        {#if tab === "logs"}
          <LogView {live} runName={title} />
        {:else if tab === "output"}
          <div class="h-full overflow-y-auto p-3">
            {#if !host}
              <p class="text-xs text-fg-muted">No host.</p>
            {:else}
              <h4 class="mb-2 text-xs font-semibold">{host.label} · {label[host.state]}</h4>
              {#if host.error}<p class="mb-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger" role="alert">{host.error}</p>{/if}
              <ol class="space-y-1.5" aria-label="Steps of {host.label}">
                {#each timeline(host, live.stepNames, live.rollbackFrom) as s (s.index)}
                  {@const result = host.steps.find((r) => r.index === s.index)}
                  <li class="rounded-md border border-line">
                    <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs" aria-expanded={!!open[`${host.hostId}:${s.index}`]} disabled={!result} onclick={() => (open[`${host.hostId}:${s.index}`] = !open[`${host.hostId}:${s.index}`])}>
                      {#if s.status === "running"}<Loader2 size={12} class="animate-spin text-accent" />{:else if s.status === "ok"}<Check size={12} class="text-success" />{:else if s.status === "pending"}<CircleDashed size={12} class="text-fg-muted" />{:else if s.status === "skipped"}<MinusCircle size={12} class="text-fg-muted" />{:else}<X size={12} class="text-danger" />{/if}
                      <span class="min-w-0 flex-1 truncate font-medium">{s.index + 1}. {s.name}</span>
                      <span class={stepTone(s.status)}>{stepLabel[s.status]}</span>
                      {#if s.note}<span class="max-w-60 truncate text-fg-muted" title={s.note}>{s.note}</span>{/if}
                      {#if s.durationMs}<span class="inline-flex items-center gap-1 font-mono text-fg-muted"><Timer size={10} />{duration(s.durationMs)}</span>{/if}
                      {#if result && (result.output.stdout || result.output.stderr)}{#if open[`${host.hostId}:${s.index}`]}<ChevronUp size={12} />{:else}<ChevronDown size={12} />{/if}{/if}
                    </button>
                    {#if result && open[`${host.hostId}:${s.index}`]}
                      <div class="border-t border-line p-2">
                        {#if result.output.stdout}<pre class="max-h-48 overflow-auto whitespace-pre-wrap rounded bg-base p-2 font-mono text-[11px]" aria-label="Standard output">{result.output.stdout}</pre>{/if}
                        {#if result.output.stderr}<pre class="mt-1 max-h-32 overflow-auto whitespace-pre-wrap rounded bg-base p-2 font-mono text-[11px] text-warning" aria-label="Standard error">{result.output.stderr}</pre>{/if}
                        {#if result.output.truncated}<p class="mt-1 text-[11px] text-fg-muted">Output was cut at the first part.</p>{/if}
                        {#if !result.output.stdout && !result.output.stderr}<p class="text-[11px] text-fg-muted">No output.</p>{/if}
                      </div>
                    {/if}
                  </li>
                {/each}
              </ol>
            {/if}
          </div>
        {:else}
          <div class="h-full overflow-y-auto p-3 text-xs" data-testid="run-summary">
            <dl class="grid max-w-md grid-cols-[8rem_1fr] gap-x-3 gap-y-1.5">
              <dt class="text-fg-muted">Hosts</dt><dd>{t.total}</dd>
              <dt class="text-fg-muted">Succeeded</dt><dd class="text-success">{t.success}</dd>
              <dt class="text-fg-muted">Failed</dt><dd class={t.failed ? "text-danger" : ""}>{t.failed}</dd>
              <dt class="text-fg-muted">Skipped</dt><dd>{t.skipped}</dd>
              <dt class="text-fg-muted">Cancelled</dt><dd>{t.cancelled}</dd>
              {#if t.pending || t.running}<dt class="text-fg-muted">Not finished</dt><dd>{t.pending + t.running}</dd>{/if}
            </dl>
            <h4 class="mb-1.5 mt-4 font-semibold">Hosts and steps</h4>
            <div class="overflow-x-auto">
              <table class="text-left text-[11px]" aria-label="Result of each step on each host">
                <thead><tr><th class="pr-3 font-medium text-fg-muted">Host</th>{#each live.stepNames as n, i (i)}<th class="px-1.5 font-medium text-fg-muted" title={n}>{i + 1}</th>{/each}<th class="pl-3 font-medium text-fg-muted">Result</th></tr></thead>
                <tbody>
                  {#each live.hosts as h (h.hostId)}
                    {@const line = timeline(h, live.stepNames, live.rollbackFrom)}
                    <tr class="border-t border-line/60">
                      <td class="max-w-40 truncate py-1 pr-3 font-medium">{h.label}</td>
                      {#each line as s (s.index)}
                        <td class="px-1.5 text-center" title="{s.name}: {stepLabel[s.status]}{s.note ? ` (${s.note})` : ''}"><span class="{stepTone(s.status)}" aria-label={stepLabel[s.status]}>{s.status === "ok" ? "✓" : s.status === "skipped" ? "–" : s.status === "pending" ? "·" : s.status === "running" ? "…" : "✗"}</span></td>
                      {/each}
                      <td class="pl-3 {hostTone(h.state)}">{label[h.state]}{#if h.error} <span class="text-fg-muted">· {h.error}</span>{/if}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
</section>
