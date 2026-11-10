<script lang="ts">
  import { Activity, AppWindow, Layers, MemoryStick, PanelLeft, PanelRight, PanelsTopLeft, Plus, RefreshCw, Server, SquareTerminal } from "lucide-svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import TmuxSettings from "./TmuxSettings.svelte";
  import { LIVE_CHOICES, bytes, serverKey } from "$lib/tmux/monitor";

  let { explorerDrawer = false, inspectorDrawer = false, onexplorer, oninspector }: { /** The list / the details have no room beside the preview, so these buttons open them over it. */ explorerDrawer?: boolean; inspectorDrawer?: boolean; onexplorer: () => void; oninspector: () => void } = $props();

  const sum = $derived(mon.summary);
  const anyLoading = $derived(Object.values(mon.loading).some(Boolean));
  const sel = $derived(mon.selectedRow);
  const target = $derived(sel ? mon.entries.find((e) => serverKey(e.hostId, e.socket) === sel.server) : mon.entries[0]);

  /** "Live · 4s" / "Paused": what the refresh is doing, from the preference (the interval is how often the session list is read). */
  const liveValue = $derived(mon.prefs.live ? String(mon.prefs.discoverMs / 1000) : "0");
  function setLive(v: string) {
    const n = Number(v);
    if (n === 0) mon.setLive(false);
    else {
      mon.setDiscoverSeconds(n);
      if (!mon.prefs.live) mon.setLive(true);
    }
  }
  const choices = $derived([...new Set([...LIVE_CHOICES.filter((n) => n > 0), mon.prefs.discoverMs / 1000])].sort((a, b) => a - b));
  const readAt = $derived(mon.lastReadAt ? new Date(mon.lastReadAt).toLocaleTimeString() : "not yet");
</script>

{#snippet metric(icon: typeof Server, value: string, label: string, title?: string)}
  {@const Icon = icon}
  <div class="flex min-w-[4.25rem] items-center gap-2 rounded-lg border border-line bg-base px-2.5 py-1.5" {title}>
    <Icon size={16} class="shrink-0 text-accent" />
    <div class="leading-tight">
      <div class="text-sm font-semibold tabular-nums">{value}</div>
      <div class="text-[10px] text-fg-muted">{label}</div>
    </div>
  </div>
{/snippet}

<header class="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-line bg-panel px-3 py-2.5">
  {#if explorerDrawer}<button class="grid h-8 w-8 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" aria-label="Show the session list" title="Show the session list" onclick={onexplorer}><PanelLeft size={15} /></button>{/if}
  <div class="flex min-w-0 items-center gap-3">
    <span class="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent/15 text-accent"><SquareTerminal size={20} /></span>
    <div class="min-w-0">
      <h1 class="text-[15px] font-semibold leading-tight">Tmux Monitor</h1>
      <p class="truncate text-xs text-fg-muted">Manage and monitor tmux sessions across all your hosts</p>
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-2" role="group" aria-label="Summary">
    {@render metric(Server, String(sum.hosts), sum.hosts === 1 ? "Host" : "Hosts")}
    {@render metric(Layers, String(sum.sessions), sum.sessions === 1 ? "Session" : "Sessions")}
    {@render metric(AppWindow, String(sum.windows), sum.windows === 1 ? "Window" : "Windows")}
    {@render metric(PanelsTopLeft, String(sum.panes), sum.panes === 1 ? "Pane" : "Panes")}
    {#if sum.cpuPct !== null}{@render metric(Activity, `${sum.cpuPct.toFixed(1)}%`, "CPU (sessions)", "CPU of the processes under tmux panes, as a share of one core, summed. An estimate from process trees.")}{/if}
    {#if sum.rss !== null}{@render metric(MemoryStick, bytes(sum.rss), "Memory (sessions)", "Resident memory of the processes under tmux panes, summed. An estimate; shared pages count in each process.")}{/if}
  </div>

  <div class="ml-auto flex items-center gap-2">
    <button class="grid h-8 w-8 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Refresh all hosts now" aria-label="Refresh all hosts" disabled={!mon.entries.length} onclick={() => mon.refreshAll()}><RefreshCw size={14} class={anyLoading ? "animate-spin" : ""} /></button>
    <label class="flex items-center gap-1.5 rounded-md border border-line bg-base px-2 py-1 text-xs" title="How often the session list is read. Last read: {readAt}">
      <span class="h-2 w-2 rounded-full {mon.prefs.live ? (sum.unreadable ? 'bg-warning' : 'bg-success') : 'bg-fg-muted/50'}"></span>
      <span class="sr-only">Refresh</span>
      <select class="bg-transparent py-0.5 text-xs text-fg focus:outline-none" aria-label="Live refresh" value={liveValue} onchange={(e) => setLive(e.currentTarget.value)}>
        {#each choices as n (n)}<option value={String(n)}>{`Live · ${n}s`}</option>{/each}
        <option value="0">Paused</option>
      </select>
    </label>
    <div class="rounded-md border border-line p-0.5"><TmuxSettings /></div>
    <button class="flex items-center gap-1.5 rounded-md bg-accent-solid px-3 py-1.5 text-xs font-medium text-white hover:opacity-90 disabled:opacity-50" disabled={!target} title="Create a tmux session on {target?.name ?? 'a host'}" onclick={() => target && mon.newSession(target.hostId, target.socket)}><Plus size={14} /> New Session</button>
    {#if inspectorDrawer}<button class="grid h-8 w-8 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" aria-label="Show the details panel" title="Show the details panel" onclick={oninspector}><PanelRight size={15} /></button>{/if}
  </div>
</header>
