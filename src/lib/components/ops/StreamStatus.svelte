<script lang="ts">
  import { tick } from "svelte";
  import { AlertTriangle, ChevronDown, RefreshCw } from "lucide-svelte";
  import { opsLogs, type Phase } from "$lib/stores/opslogs.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { relativeTime, retryText } from "$lib/ops/logstatus";

  let open = $state(false);
  let button = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();
  let pos = $state<{ left: number; top: number; width: number } | null>(null);

  const c = $derived(opsLogs.counts);
  const phase = $derived<Phase>(opsLogs.phase);
  const paused = $derived(opsLogs.paused && opsLogs.running);
  const now = $derived(opsLogs.now);

  const badge = $derived.by((): { text: string; tone: "good" | "warn" | "bad" | "idle"; pulse: boolean } => {
    if (paused) return { text: "PAUSED", tone: "warn", pulse: false };
    switch (phase) {
      case "streaming": return { text: "LIVE", tone: "good", pulse: true };
      case "reconnecting": return { text: c.streaming > 0 ? "PARTIAL" : "RECONNECTING", tone: "warn", pulse: true };
      case "connecting": return { text: "CONNECTING", tone: "warn", pulse: true };
      case "stopping": return { text: "STOPPING", tone: "idle", pulse: false };
      case "failed": return { text: "DISCONNECTED", tone: "bad", pulse: false };
      case "stopped": return { text: "STOPPED", tone: "idle", pulse: false };
      default: return { text: "NOT FOLLOWING", tone: "idle", pulse: false };
    }
  });
  const tone = { good: "text-success", warn: "text-warning", bad: "text-danger", idle: "text-fg-muted" } as const;
  const dotTone = { good: "bg-success", warn: "bg-warning", bad: "bg-danger", idle: "bg-fg-muted/50" } as const;

  const detail = $derived.by(() => {
    if (!opsLogs.running || c.total === 0) return "";
    const parts = [`${c.streaming} of ${c.total} connected`];
    if (c.connecting) parts.push(`${c.connecting} connecting`);
    if (c.reconnecting) parts.push(`${c.reconnecting} reconnecting`);
    if (c.failed) parts.push(`${c.failed} failed`);
    return parts.join(" · ");
  });

  const perMinute = $derived(opsLogs.perMinute);
  const last = $derived(opsLogs.lastEventAt);
  const stateTone = { streaming: "good", connecting: "warn", reconnecting: "warn", failed: "bad", stopped: "idle" } as const;
  const hostRows = $derived(Object.entries(opsLogs.hostStates).map(([id, s]) => ({ id, label: opsLogs.label(id), ...s })));
  const problems = $derived(opsLogs.gaps.length + (opsLogs.droppedRemote > 0 ? 1 : 0) + (opsLogs.evicted > 0 ? 1 : 0));

  async function toggle() {
    open = !open;
    if (open) {
      await tick();
      place();
    }
  }
  function place() {
    if (!button) return;
    const r = button.getBoundingClientRect();
    const width = Math.min(460, window.innerWidth - 16);
    pos = { left: Math.max(8, Math.min(r.left, window.innerWidth - width - 8)), top: r.bottom + 6, width };
  }
  $effect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!panel?.contains(t) && !button?.contains(t)) open = false;
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        open = false;
        button?.focus();
      }
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", esc);
    };
  });
</script>

<section class="flex min-h-11 shrink-0 flex-wrap items-center gap-x-4 gap-y-1 py-1 border-b border-line bg-base px-4 text-xs" aria-label="Stream status" data-testid="stream-status">
  <span class="inline-flex items-center gap-1.5 font-semibold tracking-wide {tone[badge.tone]}" role="status" data-testid="stream-badge">
    <span class="h-2 w-2 rounded-full {dotTone[badge.tone]} {badge.pulse ? 'animate-pulse' : ''}" aria-hidden="true"></span>
    {badge.text}
  </span>

  <button bind:this={button} class="inline-flex items-center gap-1 rounded px-1 py-0.5 text-fg hover:bg-panel-hover focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent" aria-expanded={open} aria-haspopup="dialog" onclick={() => void toggle()} data-testid="hosts-status">
    {opsLogs.hosts.length} Host{opsLogs.hosts.length === 1 ? "" : "s"}
    {#if detail}<span class="text-fg-muted">· {detail}</span>{/if}
    <ChevronDown size={12} class="text-fg-muted" />
  </button>

  {#if opsLogs.running}
    <span class="text-fg-muted" data-testid="rate">
      {#if perMinute !== null}<span class="text-fg">{perMinute.toLocaleString()}</span> events/min{:else}measuring…{/if}
    </span>
    <span class="text-fg-muted" data-testid="last-event">Last event: <span class="text-fg">{last ? relativeTime(last, now) : "none yet"}</span></span>
  {:else if opsLogs.received > 0}
    <span class="text-fg-muted">{opsLogs.received.toLocaleString()} events kept in this view</span>
  {/if}

  {#if paused}
    <span class="text-warning" title="The view is held still. Collecting from the hosts goes on.">Collecting continues{#if opsLogs.arrivedWhilePaused} · {opsLogs.arrivedWhilePaused.toLocaleString()} new{/if}</span>
  {/if}

  {#each hostRows.filter((h) => h.state === "reconnecting" || h.state === "failed").slice(0, 2) as h (h.id)}
    <span class="inline-flex items-center gap-1 {h.state === 'failed' ? 'text-danger' : 'text-warning'}" data-testid="host-trouble">
      <span class="max-w-32 truncate">{h.label}:</span> {retryText(h, now)}
      <button class="rounded p-0.5 text-fg-muted hover:text-fg" title="Try this host again now" aria-label="Try {h.label} again now" onclick={() => void opsLogs.retryHost(h.id)}><RefreshCw size={11} /></button>
    </span>
  {/each}

  {#if problems > 0}
    <button class="inline-flex items-center gap-1 text-warning hover:underline" onclick={() => void toggle()} title="Events may be missing: see the details">
      <AlertTriangle size={12} />
      {#if opsLogs.gaps.length}{opsLogs.gaps.length} possible gap{opsLogs.gaps.length === 1 ? "" : "s"}{/if}
      {#if opsLogs.droppedRemote}{opsLogs.gaps.length ? " · " : ""}{opsLogs.droppedRemote.toLocaleString()} dropped{/if}
      {#if opsLogs.evicted}{opsLogs.gaps.length || opsLogs.droppedRemote ? " · " : ""}{opsLogs.evicted.toLocaleString()} oldest removed{/if}
    </button>
  {/if}

  <label class="ml-auto inline-flex cursor-pointer items-center gap-1.5 text-fg-muted" title="When a connection drops, try again with growing waits, and carry on from the last entry seen where the host's journal allows it.">
    Auto-reconnect
    <input type="checkbox" class="accent-input" checked={settings.prefs.opsLogAutoReconnect} onchange={(e) => void opsLogs.setAutoReconnect(e.currentTarget.checked)} aria-label="Auto-reconnect" />
    <span class="w-6 font-medium {settings.prefs.opsLogAutoReconnect ? 'text-success' : 'text-fg-muted'}">{settings.prefs.opsLogAutoReconnect ? "ON" : "OFF"}</span>
  </label>
</section>

{#if open && pos}
  <div bind:this={panel} class="fixed z-50 max-h-[70vh] overflow-y-auto rounded-lg border border-line bg-panel p-3 text-xs shadow-2xl" style="left: {pos.left}px; top: {pos.top}px; width: {pos.width}px" role="dialog" aria-label="Host streams" data-testid="hosts-popover">
    <h3 class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Each host's stream</h3>
    {#if hostRows.length === 0}
      <p class="text-fg-muted">Nothing is being followed.</p>
    {/if}
    <ul class="space-y-2">
      {#each hostRows as h (h.id)}
        <li class="rounded-md border border-line bg-base p-2">
          <div class="flex items-center gap-2">
            <span class="h-2 w-2 shrink-0 rounded-full {dotTone[stateTone[h.state]]}" aria-hidden="true"></span>
            <span class="min-w-0 flex-1 truncate font-medium">{h.label}</span>
            <span class="{tone[stateTone[h.state]]}">{retryText(h, now)}</span>
            {#if h.state === "failed" || h.state === "reconnecting"}<button class="btn-ghost h-6 px-2 text-[11px]" onclick={() => void opsLogs.retryHost(h.id)}>Retry now</button>{/if}
          </div>
          <div class="mt-1 text-fg-muted">{h.count.toLocaleString()} event{h.count === 1 ? "" : "s"}{h.lastAt ? ` · last ${relativeTime(h.lastAt, now)}` : ""}</div>
          {#if h.message && h.state !== "streaming"}<div class="mt-1 break-words text-warning">{h.message}</div>{/if}
          {#if h.notice}<div class="mt-1 break-words text-fg-muted" title="Printed by the command on the host, not a log line">Host says: {h.notice}</div>{/if}
        </li>
      {/each}
    </ul>
    {#if opsLogs.gaps.length}
      <h3 class="mb-1 mt-3 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Possible gaps</h3>
      <ul class="space-y-1">
        {#each opsLogs.gaps.slice(-8).reverse() as g, i (i)}<li class="text-warning"><span class="font-medium">{g.host}</span> · {g.reason}</li>{/each}
      </ul>
      <p class="mt-1 text-fg-muted">After a lost connection there is no way to know what a host logged meanwhile unless its journal can be read from the last entry seen.</p>
    {/if}
    {#if opsLogs.droppedRemote || opsLogs.evicted}
      <h3 class="mb-1 mt-3 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Not all of it is here</h3>
      <ul class="space-y-1 text-fg-muted">
        {#if opsLogs.droppedRemote}<li>{opsLogs.droppedRemote.toLocaleString()} entries were left out because a host wrote faster than they can be shown ({3000}/s per host).</li>{/if}
        {#if opsLogs.evicted}<li>{opsLogs.evicted.toLocaleString()} oldest entries were removed to stay within the {settings.prefs.opsLogMax.toLocaleString()} kept. This can be changed in the toolbar's view menu.</li>{/if}
      </ul>
    {/if}
  </div>
{/if}
