<script lang="ts">
  import { Columns2, Command, Plus, Rows2, Terminal, X, Zap } from "lucide-svelte";
  import { adhocLabel, ui, type Pane, type Tab } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import SnippetPicker from "./SnippetPicker.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  const tab = $derived(ui.activeTab);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");
  let tabMenu = $state<{ id: string; x: number; y: number } | null>(null);

  function paneLabel(p: Pane) {
    if (p.target.kind === "adhoc") return adhocLabel(p.target.adhoc);
    const h = vaultStore.hostById.get(p.target.hostId)?.data;
    return h ? `${h.label} · ${h.hostname}` : "host removed";
  }

  function tabColor(t: Tab) {
    const p = t.panes[0];
    return p?.target.kind === "host" ? vaultStore.hostById.get(p.target.hostId)?.data?.color : undefined;
  }

  const dot: Record<string, string> = {
    connected: "bg-success",
    connecting: "bg-yellow-400 animate-pulse",
    error: "bg-danger",
    disconnected: "bg-fg-muted/50",
  };

  function startRename(t: Tab) {
    renaming = t.id;
    renameValue = t.customTitle ?? t.title;
  }

  function commitRename() {
    if (renaming) ui.renameTab(renaming, renameValue);
    renaming = null;
  }
</script>

<section class="flex min-w-0 flex-1 flex-col bg-base">
  {#if ui.tabs.length > 0}
    <div class="flex h-10 items-end gap-0.5 overflow-x-auto border-b border-line bg-panel px-2">
      {#each ui.tabs as t, i (t.id)}
        {@const color = tabColor(t)}
        <div
          class="group relative flex h-9 max-w-56 shrink-0 items-center gap-2 rounded-t-md px-3 text-sm
            {t.id === ui.activeTabId ? 'bg-base text-fg' : 'text-fg-muted hover:bg-panel-hover'}"
          role="tab"
          tabindex="0"
          aria-selected={t.id === ui.activeTabId}
          title={i < 9 ? `Ctrl+${i + 1}` : undefined}
          onclick={() => (ui.activeTabId = t.id)}
          ondblclick={() => startRename(t)}
          onkeydown={(e) => e.key === "Enter" && (ui.activeTabId = t.id)}
          onauxclick={(e) => e.button === 1 && ui.closeTab(t.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            tabMenu = { id: t.id, x: e.clientX, y: e.clientY };
          }}
        >
          {#if t.id === ui.activeTabId}
            <span class="absolute inset-x-2 top-0 h-0.5 rounded-b" style:background={color ?? "var(--color-accent)"}></span>
          {/if}
          <span class="h-2 w-2 shrink-0 rounded-full {dot[ui.tabStatus(t)]}"></span>
          {#if renaming === t.id}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="w-32 rounded bg-panel px-1 text-sm outline-none ring-1 ring-accent"
              bind:value={renameValue}
              autofocus
              onblur={commitRename}
              onkeydown={(e) => {
                if (e.key === "Enter") commitRename();
                else if (e.key === "Escape") renaming = null;
                e.stopPropagation();
              }}
            />
          {:else}
            <span class="truncate">{t.customTitle ?? t.title}</span>
          {/if}
          <button
            class="rounded p-0.5 opacity-0 hover:bg-panel-hover group-hover:opacity-100 {t.id === ui.activeTabId ? 'opacity-60' : ''}"
            onclick={(e) => { e.stopPropagation(); ui.closeTab(t.id); }}
            aria-label="Close tab"
            title="Close (Ctrl+Shift+W)"
          >
            <X size={12} />
          </button>
        </div>
      {/each}
      <button class="icon-btn mb-1 h-7 w-7 shrink-0" title="New connection (Ctrl+Shift+T)" onclick={() => (ui.modal = { kind: "quick-connect" })}>
        <Plus size={15} />
      </button>
      <div class="flex-1"></div>
      <div class="flex h-10 items-center gap-0.5">
        <button class="icon-btn h-8 w-8" title="Command palette (Ctrl+Shift+P)" onclick={() => (ui.paletteOpen = true)}><Command size={15} /></button>
        <SnippetPicker />
      </div>
    </div>
  {/if}

  {#if tabMenu}
    {@const id = tabMenu.id}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (tabMenu = null)}></button>
    <div class="fixed z-50 w-48 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" style:left="{tabMenu.x}px" style:top="{tabMenu.y}px" role="menu">
      {#each [
        { label: "Rename", run: () => { const t = ui.tabs.find((x) => x.id === id); if (t) startRename(t); } },
        { label: "Duplicate", run: () => ui.duplicateTab(id) },
        { label: "Close", run: () => ui.closeTab(id) },
        { label: "Close other tabs", run: () => ui.closeOtherTabs(id) },
      ] as item (item.label)}
        <button class="w-full px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { tabMenu = null; item.run(); }}>
          {item.label}
        </button>
      {/each}
    </div>
  {/if}

  <!-- Every tab stays mounted so background sessions keep streaming. -->
  {#each ui.tabs as t (t.id)}
    <div class="min-h-0 flex-1 overflow-hidden {t.id === ui.activeTabId ? 'flex' : 'hidden'} {t.split === 'horizontal' ? 'flex-col' : 'flex-row'}">
      {#each t.panes as pane (pane.id)}
        {@const info = ui.paneInfo[pane.id]}
        <div
          class="relative flex min-h-0 min-w-0 flex-1 flex-col border-line
            {t.panes.length > 1 && t.split === 'vertical' ? 'first:border-r' : ''}
            {t.panes.length > 1 && t.split === 'horizontal' ? 'first:border-b' : ''}
            {pane.id === t.activePaneId && t.panes.length > 1 ? 'ring-1 ring-inset ring-accent/40' : ''}"
          role="presentation"
          onmousedown={() => (t.activePaneId = pane.id)}
        >
          <div class="flex h-7 items-center gap-2 border-b border-line bg-panel/40 px-3 text-xs text-fg-muted">
            <span class="truncate">{paneLabel(pane)}</span>
            {#if info?.remoteTitle}
              <span class="truncate text-fg-muted/70">— {info.remoteTitle}</span>
            {/if}
            <div class="flex-1"></div>
            {#if t.panes.length === 1}
              <button class="icon-btn h-6 w-6" title="Split right (Ctrl+Shift+D)" onclick={() => ui.splitActive("vertical")}><Columns2 size={13} /></button>
              <button class="icon-btn h-6 w-6" title="Split down (Ctrl+Shift+E)" onclick={() => ui.splitActive("horizontal")}><Rows2 size={13} /></button>
            {:else}
              <button class="icon-btn h-6 w-6" title="Close pane" onclick={() => ui.closePane(t.id, pane.id)}><X size={13} /></button>
            {/if}
          </div>
          <TerminalPane {pane} active={pane.id === t.activePaneId && t.id === ui.activeTabId} />
        </div>
      {/each}
    </div>
  {/each}

  {#if !tab}
    <div class="flex flex-1 flex-col items-center justify-center p-6 text-center">
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-panel text-accent">
        <Terminal size={26} />
      </div>
      <h2 class="text-base font-semibold">No open terminals</h2>
      <p class="mt-1 max-w-sm text-sm text-fg-muted">
        Double-click a host in the sidebar, or connect to any server without saving it first.
      </p>
      <div class="mt-5 flex gap-2">
        <button class="btn-primary" onclick={() => (ui.modal = { kind: "quick-connect" })}><Zap size={14} /> Quick connect</button>
        <button class="btn-ghost border border-line" onclick={() => (ui.paletteOpen = true)}><Command size={14} /> Command palette</button>
      </div>
      <div class="mt-6 grid grid-cols-2 gap-x-6 gap-y-1.5 text-left text-xs text-fg-muted">
        {#each [
          ["Ctrl+Shift+P", "Command palette"],
          ["Ctrl+Shift+T", "Quick connect"],
          ["Ctrl+Tab", "Next tab"],
          ["Ctrl+Shift+W", "Close tab"],
          ["Ctrl+Shift+D / E", "Split right / down"],
          ["Ctrl+Shift+F", "Find in terminal"],
          ["Ctrl+Shift+C / V", "Copy / paste"],
          ["Ctrl+= / Ctrl+-", "Zoom in / out"],
        ] as [keys, what] (keys)}
          <kbd class="rounded border border-line bg-panel px-1.5 py-0.5 font-mono text-[11px] text-fg">{keys}</kbd>
          <span class="self-center">{what}</span>
        {/each}
      </div>
    </div>
  {/if}
</section>
