<script lang="ts">
  import { PanelLeft, PanelRight, Plus, RefreshCw, Search, SquareTerminal, X } from "lucide-svelte";
  import { onMount } from "svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { serverKey } from "$lib/tmux/monitor";
  import Splitter from "./Splitter.svelte";
  import TmuxExplorer from "./TmuxExplorer.svelte";
  import TmuxInspector from "./TmuxInspector.svelte";
  import TmuxPreview from "./TmuxPreview.svelte";
  import TmuxSearch from "./TmuxSearch.svelte";

  let width = $state(1200);
  let searching = $state(false);
  let adding = $state("");
  let filter = $state<HTMLInputElement>();

  // The page is only "visible" while it is mounted: leaving it stops watching and capturing; coming back resumes.
  onMount(() => {
    void mon.start();
    const hide = () => (document.hidden ? mon.stop() : void mon.start());
    document.addEventListener("visibilitychange", hide);
    return () => {
      document.removeEventListener("visibilitychange", hide);
      mon.stop();
    };
  });

  // Narrow windows give the room to the preview: the inspector goes first, then the explorer.
  const showInspector = $derived(mon.prefs.inspectorOpen && width >= 900);
  const showExplorer = $derived(mon.prefs.explorerOpen && width >= 560);
  const explorerW = $derived(Math.min(mon.prefs.explorerWidth, Math.max(200, width * 0.4)));
  const inspectorW = $derived(Math.min(mon.prefs.inspectorWidth, Math.max(220, width * 0.35)));

  const selectedHost = $derived(mon.selectedRow ? mon.entries.find((e) => serverKey(e.hostId, e.socket) === mon.selectedRow!.server) : mon.entries[0]);
  const anyLoading = $derived(Object.values(mon.loading).some(Boolean));

  function keys(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    const typing = t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      filter?.focus();
    } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searching = true;
    } else if (!typing && e.key === "/") {
      e.preventDefault();
      filter?.focus();
    } else if (e.key === "Escape" && searching) {
      searching = false;
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" data-testid="tmux-monitor" bind:clientWidth={width}>
  <header class="flex shrink-0 flex-wrap items-center gap-2 border-b border-line bg-panel px-3 py-2">
    <h1 class="mr-1 flex items-center gap-2 text-[15px] font-semibold"><SquareTerminal size={17} class="text-accent" /> Tmux</h1>

    <select
      class="max-w-44 rounded-md border border-line bg-base px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none"
      aria-label="Add a host to the monitor"
      bind:value={adding}
      onchange={() => { if (adding) mon.addHost(adding); adding = ""; }}
    >
      <option value="">+ Add host…</option>
      {#each mon.addable as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
    </select>

    <div class="relative min-w-40 flex-1 sm:max-w-sm">
      <Search size={13} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input
        bind:this={filter}
        class="w-full rounded-md border border-line bg-base py-1 pl-7 pr-7 text-xs focus:border-accent focus:outline-none"
        placeholder="Search sessions, panes, commands…"
        aria-label="Search sessions, windows, panes, commands and tags"
        bind:value={mon.query}
      />
      {#if mon.query}<button class="absolute right-1 top-1/2 grid h-5 w-5 -translate-y-1/2 place-items-center rounded text-fg-muted hover:text-fg" aria-label="Clear search" onclick={() => (mon.query = "")}><X size={12} /></button>{/if}
    </div>

    <button class="flex items-center gap-1 rounded-md border px-2 py-1 text-xs {searching ? 'border-accent/60 bg-accent/10 text-accent' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}" aria-pressed={searching} title="Search the text inside panes (Ctrl+Shift+F)" onclick={() => (searching = !searching)}>
      <Search size={12} /> Output
    </button>

    <div class="ml-auto flex items-center gap-1.5">
      <span class="hidden text-[11px] text-fg-muted md:inline" aria-live="polite">{anyLoading ? "Reading…" : mon.entries.length ? `${mon.entries.length} ${mon.entries.length === 1 ? "server" : "servers"}` : ""}</span>
      <button class="grid h-7 w-7 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Refresh all hosts" aria-label="Refresh all hosts" disabled={!mon.entries.length} onclick={() => mon.refreshAll()}><RefreshCw size={13} class={anyLoading ? "animate-spin" : ""} /></button>
      <button class="flex items-center gap-1 rounded-md border border-accent/50 bg-accent/10 px-2.5 py-1 text-xs text-accent hover:bg-accent/20 disabled:opacity-50" disabled={!selectedHost} title="Create a tmux session on {selectedHost?.name ?? 'a host'}" onclick={() => selectedHost && mon.newSession(selectedHost.hostId, selectedHost.socket)}><Plus size={13} /> New session</button>
      <button class="grid h-7 w-7 place-items-center rounded-md border border-line {mon.prefs.explorerOpen ? 'text-accent' : 'text-fg-muted'} hover:bg-panel-hover" aria-pressed={mon.prefs.explorerOpen} title="Show or hide the session list" aria-label="Show or hide the session list" onclick={() => (mon.prefs.explorerOpen = !mon.prefs.explorerOpen)}><PanelLeft size={13} /></button>
      <button class="grid h-7 w-7 place-items-center rounded-md border border-line {mon.prefs.inspectorOpen ? 'text-accent' : 'text-fg-muted'} hover:bg-panel-hover" aria-pressed={mon.prefs.inspectorOpen} title="Show or hide the details panel" aria-label="Show or hide the details panel" onclick={() => (mon.prefs.inspectorOpen = !mon.prefs.inspectorOpen)}><PanelRight size={13} /></button>
    </div>
  </header>

  {#if mon.entries.length === 0}
    <div class="grid flex-1 place-items-center p-8 text-center">
      <div class="max-w-sm">
        <SquareTerminal size={30} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm font-medium">See the tmux sessions on your hosts</p>
        <p class="mt-1 text-xs text-fg-muted">Add a host from the bar above. Its sessions, windows and panes are read over the connection SSHVault already keeps; nothing is installed on the host, and nothing is attached until you choose.</p>
        {#if vaultStore.hosts.length === 0}<p class="mt-3 text-xs text-fg-muted">You have no hosts yet. Add one in Hosts first.</p>{/if}
      </div>
    </div>
  {:else}
    <div class="flex min-h-0 flex-1">
      {#if showExplorer && !searching}
        <nav class="flex min-h-0 shrink-0 flex-col bg-panel/30" style="width:{explorerW}px" aria-label="Session explorer">
          <TmuxExplorer />
        </nav>
        <Splitter width={mon.prefs.explorerWidth} min={200} max={560} side="left" label="Resize the session list" onchange={(w) => (mon.prefs.explorerWidth = w)} />
      {/if}
      {#if searching}
        <TmuxSearch onclose={() => (searching = false)} />
      {:else}
        <TmuxPreview />
      {/if}
      {#if showInspector && !searching}
        <Splitter width={mon.prefs.inspectorWidth} min={220} max={520} side="right" label="Resize the details panel" onchange={(w) => (mon.prefs.inspectorWidth = w)} />
        <div class="min-h-0 shrink-0" style="width:{inspectorW}px"><TmuxInspector /></div>
      {/if}
    </div>
  {/if}
</div>
