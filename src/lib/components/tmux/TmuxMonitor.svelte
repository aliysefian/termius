<script lang="ts">
  import { SquareTerminal, X } from "lucide-svelte";
  import { onMount, untrack } from "svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { layoutFor } from "$lib/tmux/monitor";
  import Splitter from "./Splitter.svelte";
  import TmuxBottom from "./TmuxBottom.svelte";
  import TmuxExplorer from "./TmuxExplorer.svelte";
  import TmuxHeader from "./TmuxHeader.svelte";
  import TmuxInspector from "./TmuxInspector.svelte";
  import TmuxSettings from "./TmuxSettings.svelte";
  import TmuxWork from "./TmuxWork.svelte";

  let width = $state(1200);
  let adding = $state("");
  let explorer = $state<TmuxExplorer>();
  /** On a narrow window the explorer and the inspector are drawers over the preview. */
  let drawer = $state<"explorer" | "inspector" | null>(null);

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

  // The preview keeps its room: the inspector goes first, then the explorer, and each is then a drawer instead.
  const room = $derived(layoutFor(width, mon.prefs));
  const explorerW = $derived(Math.min(mon.prefs.explorerWidth, Math.max(200, width * 0.4)));
  const inspectorW = $derived(Math.min(mon.prefs.inspectorWidth, Math.max(220, width * 0.35)));

  // Figures follow the selected server (and the store's timer keeps them current while the page is on screen). The effect depends on
  // which server is selected, not on the readings that change under it.
  const resourceServer = $derived(mon.selectedRow && mon.focusTarget ? mon.selectedRow.server : null);
  $effect(() => {
    const server = resourceServer;
    if (server) untrack(() => void mon.loadResources(server));
  });
  // Choosing something in a drawer closes it, so the preview is what you see next.
  $effect(() => {
    void mon.selected;
    untrack(() => (drawer = null));
  });

  function keys(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    const typing = t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement;
    const focusFilter = () => {
      if (!room.explorer) {
        drawer = "explorer";
        queueMicrotask(() => explorer?.focusFilter());
      } else explorer?.focusFilter();
    };
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      mon.prefs.bottom.tab = "search";
      mon.prefs.bottom.open = true;
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      focusFilter();
    } else if (!typing && e.key === "/") {
      e.preventDefault();
      focusFilter();
    } else if (e.key === "Escape" && drawer) {
      drawer = null;
    }
  }
</script>

<svelte:window onkeydown={keys} />

{#snippet pickHost()}
  <select
    class="rounded-lg border border-line bg-base px-3 py-2 text-sm text-fg focus:border-accent focus:outline-none"
    aria-label="Add a host to the monitor"
    bind:value={adding}
    onchange={() => { if (adding) mon.addHost(adding as never); adding = ""; }}
  >
    <option value="">+ Add a host…</option>
    {#each mon.addable as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
  </select>
{/snippet}

<div class="relative flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" data-testid="tmux-monitor" bind:clientWidth={width}>
  <TmuxHeader explorerDrawer={!room.explorer} inspectorDrawer={!room.inspector} onexplorer={() => (drawer = "explorer")} oninspector={() => (drawer = "inspector")} />

  {#if mon.entries.length === 0}
    <div class="grid flex-1 place-items-center p-8 text-center">
      <div class="max-w-sm">
        <span class="mx-auto mb-4 grid h-14 w-14 place-items-center rounded-2xl bg-accent/15 text-accent"><SquareTerminal size={26} /></span>
        <p class="text-sm font-semibold">See the tmux sessions on your hosts</p>
        <p class="mt-1 text-xs text-fg-muted">Add a host. Its sessions, windows and panes are read over the connection SSHVault already keeps; nothing is installed on the host, and nothing is attached until you choose.</p>
        <div class="mt-4">{@render pickHost()}</div>
        {#if vaultStore.hosts.length === 0}<p class="mt-3 text-xs text-fg-muted">You have no hosts yet. Add one in Hosts first.</p>{/if}
      </div>
    </div>
  {:else}
    <div class="flex min-h-0 flex-1">
      {#if room.explorer}
        <nav class="flex min-h-0 shrink-0 flex-col bg-panel/30" style="width:{explorerW}px" aria-label="Session explorer">
          <TmuxExplorer bind:this={explorer} />
        </nav>
        <Splitter width={mon.prefs.explorerWidth} min={240} max={560} side="left" label="Resize the session list" onchange={(w) => (mon.prefs.explorerWidth = w)} />
      {/if}

      <div class="flex min-h-0 min-w-0 flex-1 flex-col">
        <TmuxWork />
        {#if mon.prefs.bottom.open}
          <Splitter vertical width={mon.prefs.bottom.height} min={120} max={600} side="right" label="Resize the output panel" onchange={(h) => (mon.prefs.bottom.height = h)} />
        {/if}
        <TmuxBottom />
      </div>

      {#if room.inspector}
        <Splitter width={mon.prefs.inspectorWidth} min={260} max={520} side="right" label="Resize the details panel" onchange={(w) => (mon.prefs.inspectorWidth = w)} />
        <div class="min-h-0 shrink-0" style="width:{inspectorW}px"><TmuxInspector /></div>
      {/if}
    </div>

    <!-- Drawers: the same panels, over the preview, when there is no room beside it. -->
    {#if drawer}
      <button class="absolute inset-0 z-20 cursor-default bg-black/40" aria-label="Close the panel" tabindex="-1" onclick={() => (drawer = null)}></button>
      <div class="absolute inset-y-0 z-30 flex w-[min(22rem,90%)] flex-col border-line bg-panel shadow-2xl {drawer === 'explorer' ? 'left-0 border-r' : 'right-0 border-l'}" role="dialog" aria-label={drawer === "explorer" ? "Session explorer" : "Details"}>
        <div class="flex shrink-0 items-center justify-between border-b border-line px-3 py-2 text-xs font-semibold">
          {drawer === "explorer" ? "Sessions" : "Details"}
          <button class="grid h-6 w-6 place-items-center rounded text-fg-muted hover:bg-panel-hover hover:text-fg" aria-label="Close" onclick={() => (drawer = null)}><X size={14} /></button>
        </div>
        <div class="flex min-h-0 flex-1 flex-col">
          {#if drawer === "explorer"}<TmuxExplorer bind:this={explorer} />{:else}<TmuxInspector />{/if}
        </div>
      </div>
    {/if}
  {/if}
</div>
