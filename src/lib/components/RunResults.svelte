<script lang="ts">
  import { Check, ChevronDown, ChevronRight, CircleDashed, Loader2, MinusCircle, X } from "lucide-svelte";
  import { duration, statusLabel, type HostRun, type StepResult } from "$lib/runbook";

  /** The hosts of a run, live or from the record. */
  let { hosts, steps }: { hosts: HostRun[]; steps: { name: string }[] } = $props();

  let open = $state<Set<string>>(new Set());
  const toggle = (id: string) => {
    const next = new Set(open);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    open = next;
  };

  const mark = (s: StepResult["status"]) => (s === "ok" ? "text-success" : s === "skipped" ? "text-fg-muted" : "text-danger");
  const summary = (h: HostRun) => {
    const done = h.steps.filter((s) => s.status !== "skipped").length;
    return h.state === "waiting" ? "waiting" : h.state === "running" ? `step ${(h.current ?? done) + 1} of ${steps.length}` : h.error ? h.error : `${done} of ${steps.length} steps`;
  };
</script>

<ul class="space-y-1" data-testid="run-results">
  {#each hosts as h (h.hostId)}
    <li class="rounded-md border border-line">
      <button class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm" aria-expanded={open.has(h.hostId)} onclick={() => toggle(h.hostId)} data-host={h.label}>
        {#if open.has(h.hostId)}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
        {#if h.state === "running"}<Loader2 size={14} class="animate-spin text-accent" />
        {:else if h.state === "ok"}<Check size={14} class="text-success" />
        {:else if h.state === "failed"}<X size={14} class="text-danger" />
        {:else}<CircleDashed size={14} class="text-fg-muted" />{/if}
        <span class="min-w-0 flex-1 truncate font-medium">{h.label}</span>
        <span class="truncate text-xs {h.state === 'failed' ? 'text-danger' : 'text-fg-muted'}" data-testid="host-state">{h.state === "ok" ? "done" : h.state === "failed" ? "failed" : ""} {summary(h)}</span>
      </button>
      {#if open.has(h.hostId)}
        <ol class="space-y-1 border-t border-line px-3 py-2">
          {#each h.steps as s (s.index)}
            <li>
              <div class="flex items-baseline gap-2 text-xs">
                {#if s.status === "ok"}<Check size={12} class="shrink-0 {mark(s.status)}" />{:else if s.status === "skipped"}<MinusCircle size={12} class="shrink-0 {mark(s.status)}" />{:else}<X size={12} class="shrink-0 {mark(s.status)}" />{/if}
                <span class="font-medium">{s.name}</span>
                <span class={mark(s.status)}>{statusLabel(s.status)}</span>
                {#if s.note}<span class="text-fg-muted">{s.note}</span>{/if}
                {#if s.output.duration_ms}<span class="ml-auto text-fg-muted">{duration(s.output.duration_ms)}</span>{/if}
              </div>
              {#if s.output.stdout || s.output.stderr}
                <pre class="mt-1 max-h-48 overflow-auto rounded border border-line bg-base p-2 font-mono text-[11px] leading-4 whitespace-pre-wrap">{s.output.stdout}{#if s.output.stderr}<span class="text-warning">{s.output.stderr}</span>{/if}{#if s.output.truncated}<span class="text-fg-muted">… (cut)</span>{/if}</pre>
              {/if}
            </li>
          {/each}
          {#if h.current !== null}<li class="flex items-center gap-2 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> {steps[h.current]?.name ?? `Step ${h.current + 1}`}…</li>{/if}
          {#if h.error}<li class="text-xs text-danger">{h.error}</li>{/if}
        </ol>
      {/if}
    </li>
  {/each}
</ul>
