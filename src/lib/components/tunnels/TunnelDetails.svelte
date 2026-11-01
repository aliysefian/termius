<script lang="ts">
  import { Ellipsis, Play, RotateCw, Square, X } from "lucide-svelte";
  import TunnelStatusBadge from "./TunnelStatusBadge.svelte";
  import TunnelActionsMenu from "./TunnelActionsMenu.svelte";
  import TunnelOverview from "./TunnelOverview.svelte";
  import TunnelConfiguration from "./TunnelConfiguration.svelte";
  import TunnelLogs from "./TunnelLogs.svelte";
  import { tunnelActions } from "./actions";
  import { tunnels, type DetailTab } from "$lib/stores/tunnels.svelte";
  import type { TunnelRow } from "$lib/tunnels";

  let { row, now, onclose }: { row: TunnelRow; now: number; /** Shown when the panel is a slide-over, to close it. */ onclose?: () => void } = $props();

  const a = $derived(tunnelActions(row));
  let menu = $state<{ x: number; y: number } | null>(null);
  const TABS: { id: DetailTab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "configuration", label: "Configuration" },
    { id: "logs", label: "Logs" },
  ];

  function tabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((t) => t.id === tunnels.tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    tunnels.tab = next.id;
    queueMicrotask(() => document.getElementById(`tunnel-tab-${next.id}`)?.focus());
  }
</script>

<section class="flex h-full min-h-0 flex-col" aria-label="Details of {row.rule.label}">
  <header class="space-y-3 border-b border-line px-4 pb-3 pt-4">
    <div class="flex items-start gap-2">
      <div class="min-w-0 flex-1">
        <h2 class="truncate text-[16px] font-semibold leading-6" title={row.rule.label}>{row.rule.label}</h2>
        <p class="truncate text-xs text-fg-muted">{row.rule.description || "No description"}</p>
      </div>
      {#if onclose}<button class="icon-btn h-8 w-8 shrink-0" title="Close details" aria-label="Close details" onclick={onclose}><X size={16} /></button>{/if}
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <TunnelStatusBadge status={row.status} />
      <div class="flex-1"></div>
      {#if row.status === "active" || row.status === "connecting"}
        <button class="btn-secondary h-8" disabled={!a.canStop} title={a.stopWhy} onclick={a.stop}><Square size={14} /> Stop</button>
      {:else}
        <button class="btn-primary h-8" disabled={!a.canStart} title={a.startWhy} onclick={a.start}><Play size={14} /> Start</button>
      {/if}
      <button class="btn-secondary h-8" disabled={!a.canRestart} title={a.restartWhy} onclick={a.restart}><RotateCw size={14} /> Restart</button>
      <button class="icon-btn h-8 w-8 border border-line" title="More actions" aria-label="More actions" aria-haspopup="menu" onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); menu = { x: r.right - 224, y: r.bottom + 4 }; }}><Ellipsis size={16} /></button>
    </div>
  </header>

  <div class="flex gap-1 border-b border-line px-3 pt-2" role="tablist" aria-label="Tunnel details">
    {#each TABS as t (t.id)}
      <button
        id="tunnel-tab-{t.id}"
        role="tab"
        aria-selected={tunnels.tab === t.id}
        aria-controls="tunnel-panel"
        tabindex={tunnels.tab === t.id ? 0 : -1}
        class="-mb-px rounded-t-md border-b-2 px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent {tunnels.tab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
        onclick={() => (tunnels.tab = t.id)}
        onkeydown={tabKey}>{t.label}</button
      >
    {/each}
  </div>

  <div id="tunnel-panel" role="tabpanel" aria-labelledby="tunnel-tab-{tunnels.tab}" class="min-h-0 flex-1 overflow-y-auto p-4">
    {#if tunnels.tab === "overview"}
      <TunnelOverview {row} {now} />
    {:else if tunnels.tab === "configuration"}
      <TunnelConfiguration {row} />
    {:else}
      <TunnelLogs {row} />
    {/if}
  </div>
</section>

{#if menu}
  <TunnelActionsMenu {row} x={menu.x} y={menu.y} onclose={() => (menu = null)} />
{/if}
