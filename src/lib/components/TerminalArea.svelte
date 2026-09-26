<script lang="ts">
  import { Columns2, Rows2, Terminal, X } from "lucide-svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import SnippetPicker from "./SnippetPicker.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const tab = $derived(ui.activeTab);
  const t_is_active = (id: string) => id === ui.activeTabId;
</script>

<section class="flex min-w-0 flex-1 flex-col bg-base">
  {#if ui.tabs.length > 0}
    <div class="flex h-10 items-end gap-0.5 overflow-x-auto border-b border-line bg-panel px-2">
      {#each ui.tabs as t (t.id)}
        <div
          class="group flex h-9 max-w-52 items-center gap-2 rounded-t-md px-3 text-sm
            {t.id === ui.activeTabId ? 'bg-base text-fg' : 'text-fg-muted hover:bg-panel-hover'}"
          role="tab"
          tabindex="0"
          aria-selected={t.id === ui.activeTabId}
          onclick={() => (ui.activeTabId = t.id)}
          onkeydown={(e) => e.key === "Enter" && (ui.activeTabId = t.id)}
          onauxclick={(e) => e.button === 1 && ui.closeTab(t.id)}
        >
          <Terminal size={14} class="shrink-0" />
          <span class="truncate">{t.title}</span>
          <button
            class="rounded p-0.5 opacity-0 hover:bg-panel-hover group-hover:opacity-100"
            onclick={(e) => { e.stopPropagation(); ui.closeTab(t.id); }}
            aria-label="Close tab"
          >
            <X size={12} />
          </button>
        </div>
      {/each}
      <div class="flex-1"></div>
      <div class="flex h-10 items-center"><SnippetPicker /></div>
    </div>
  {/if}

  {#if tab}
    <div class="flex flex-1 overflow-hidden {tab.split === 'horizontal' ? 'flex-col' : 'flex-row'}">
      {#each tab.panes as pane (pane.id)}
        {@const host = vaultStore.hostById.get(pane.hostId)?.data}
        <div
          class="relative flex min-h-0 min-w-0 flex-1 flex-col border-line
            {tab.panes.length > 1 && tab.split === 'vertical' ? 'first:border-r' : ''}
            {tab.panes.length > 1 && tab.split === 'horizontal' ? 'first:border-b' : ''}
            {pane.id === tab.activePaneId && tab.panes.length > 1 ? 'ring-1 ring-inset ring-accent/40' : ''}"
          role="presentation"
          onmousedown={() => (tab.activePaneId = pane.id)}
        >
          <div class="flex h-7 items-center gap-2 border-b border-line px-3 text-xs text-fg-muted">
            <span class="truncate">{host ? `${host.label} · ${host.hostname}` : "host removed"}</span>
            <div class="flex-1"></div>
            {#if tab.panes.length === 1}
              <button class="icon-btn h-6 w-6" title="Split right" onclick={() => ui.splitActive("vertical", pane.hostId)}><Columns2 size={13} /></button>
              <button class="icon-btn h-6 w-6" title="Split down" onclick={() => ui.splitActive("horizontal", pane.hostId)}><Rows2 size={13} /></button>
            {:else}
              <button class="icon-btn h-6 w-6" title="Close pane" onclick={() => ui.closePane(tab.id, pane.id)}><X size={13} /></button>
            {/if}
          </div>
          {#key pane.id}
            <TerminalPane paneId={pane.id} hostId={pane.hostId} active={pane.id === tab.activePaneId && t_is_active(tab.id)} />
          {/key}
        </div>
      {/each}
    </div>
  {:else}
    <div class="flex flex-1 flex-col items-center justify-center text-center">
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-panel text-accent">
        <Terminal size={26} />
      </div>
      <h2 class="text-base font-semibold">No open terminals</h2>
      <p class="mt-1 max-w-xs text-sm text-fg-muted">
        Double-click a host in the sidebar to open it in a new tab.
      </p>
    </div>
  {/if}
</section>
