<script lang="ts">
  import { Copy, Ellipsis, Pause, Play, Square, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import ContainerOverview from "./ContainerOverview.svelte";
  import ContainerLogsPanel from "./ContainerLogsPanel.svelte";
  import ContainerStatsPanel from "./ContainerStatsPanel.svelte";
  import ContainerInspectPanel from "./ContainerInspectPanel.svelte";
  import ContainerTerminalPanel from "./ContainerTerminalPanel.svelte";
  import ContainerMenu from "./ContainerMenu.svelte";
  import StateBadge from "./StateBadge.svelte";
  import { runMenu, type DrawerTab } from "./actions";
  import { rowPrimary } from "$lib/containerview";
  import * as api from "$lib/api";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerInfo } from "$lib/types";

  let { sourceKey, c, tab = $bindable(), onclose }: { sourceKey: string; c: ContainerInfo; tab: DrawerTab; /** Shown as a close button (and Escape) when the drawer is a slide-over or docked. */ onclose: () => void } = $props();

  const TABS: { id: DrawerTab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "logs", label: "Logs" },
    { id: "stats", label: "Stats" },
    { id: "inspect", label: "Inspect" },
    { id: "terminal", label: "Terminal" },
  ];
  const src = $derived(containers.sources[sourceKey]);
  const busy = $derived(!!containers.busy[c.id]);
  const primary = $derived(rowPrimary(c));
  let menu = $state<{ x: number; y: number } | null>(null);
  // The shell and logs are kept once opened, so switching tabs doesn't end them; the whole drawer is rebuilt for another container.
  let opened = $state<Record<string, boolean>>({});
  $effect(() => {
    opened[tab] = true;
  });

  function tabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((t) => t.id === tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    tab = next.id;
    queueMicrotask(() => document.getElementById(`container-tab-${next.id}`)?.focus());
  }

  function primaryAction() {
    runMenu(sourceKey, c, primary === "stop" ? "stop" : primary === "resume" ? "resume" : "start", (t) => (tab = t));
  }
  async function copyId() {
    try {
      await writeText(c.id);
      ui.notify("info", "Copied the container ID.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

<section class="flex h-full min-h-0 flex-col" aria-label="Details of {c.name}">
  <header class="space-y-3 border-b border-line px-4 pb-3 pt-4">
    <div class="flex items-start gap-2">
      <div class="min-w-0 flex-1">
        <h2 class="truncate text-[16px] font-semibold leading-6" title={c.name}>{c.name}</h2>
        <p class="truncate font-mono text-xs text-fg-muted" title={c.image}>{c.image}</p>
        <p class="mt-0.5 flex items-center gap-1 font-mono text-[11px] text-fg-muted">
          {c.id.slice(0, 12)}
          <button class="inline-flex hover:text-fg" title="Copy the full container ID" aria-label="Copy the container ID" onclick={() => void copyId()}><Copy size={11} /></button>
        </p>
      </div>
      <button class="icon-btn h-8 w-8 shrink-0" title="Close details (Esc)" aria-label="Close details" onclick={onclose}><X size={16} /></button>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <StateBadge {c} />
      <div class="flex-1"></div>
      <button class="{primary === 'stop' ? 'btn-secondary' : 'btn-primary'} h-8" disabled={busy} onclick={primaryAction}>
        {#if primary === "stop"}<Square size={14} /> Stop{:else if primary === "resume"}<Play size={14} /> Resume{:else}<Play size={14} /> Start{/if}
      </button>
      <button class="icon-btn h-8 w-8 border border-line" title="More actions" aria-label="More actions" aria-haspopup="menu" onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); menu = { x: r.right - 224, y: r.bottom + 4 }; }}><Ellipsis size={16} /></button>
    </div>
  </header>

  <div class="flex gap-1 overflow-x-auto border-b border-line px-3 pt-2" role="tablist" aria-label="Container details">
    {#each TABS as t (t.id)}
      <button
        id="container-tab-{t.id}"
        role="tab"
        aria-selected={tab === t.id}
        aria-controls="container-panel-{t.id}"
        tabindex={tab === t.id ? 0 : -1}
        class="-mb-px shrink-0 rounded-t-md border-b-2 px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent {tab === t.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
        onclick={() => (tab = t.id)}
        onkeydown={tabKey}>{t.label}</button
      >
    {/each}
  </div>

  <div class="relative min-h-0 flex-1 overflow-hidden">
    <div id="container-panel-overview" role="tabpanel" aria-labelledby="container-tab-overview" class="h-full overflow-y-auto p-4 {tab === 'overview' ? '' : 'hidden'}">
      <ContainerOverview {sourceKey} {c} />
    </div>
    {#if opened.logs}
      <div id="container-panel-logs" role="tabpanel" aria-labelledby="container-tab-logs" class="h-full p-3 {tab === 'logs' ? '' : 'hidden'}">
        <ContainerLogsPanel {sourceKey} id={c.id} name={c.name} />
      </div>
    {/if}
    <div id="container-panel-stats" role="tabpanel" aria-labelledby="container-tab-stats" class="h-full overflow-y-auto p-4 {tab === 'stats' ? '' : 'hidden'}">
      {#if tab === "stats"}<ContainerStatsPanel {sourceKey} {c} />{/if}
    </div>
    {#if opened.inspect}
      <div id="container-panel-inspect" role="tabpanel" aria-labelledby="container-tab-inspect" class="h-full p-3 {tab === 'inspect' ? '' : 'hidden'}">
        <ContainerInspectPanel secrets load={() => (src ? api.containers.inspect(src.sessionId, src.runtime, c.id) : Promise.reject(new Error("This host was closed.")))} />
      </div>
    {/if}
    <div id="container-panel-terminal" role="tabpanel" aria-labelledby="container-tab-terminal" class="h-full p-3 {tab === 'terminal' ? '' : 'hidden'}">
      <ContainerTerminalPanel {sourceKey} {c} visible={tab === "terminal"} />
    </div>
  </div>
</section>

{#if menu}
  <ContainerMenu {c} {busy} x={menu.x} y={menu.y} onpick={(id) => runMenu(sourceKey, c, id, (t) => (tab = t))} onclose={() => (menu = null)} />
{/if}
