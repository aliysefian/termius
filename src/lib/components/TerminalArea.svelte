<script lang="ts">
  import { Circle, Columns2, Command, Keyboard, Plus, Rows2, SquareTerminal, Terminal, TextSelect, X, Zap } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import * as api from "$lib/api";
  import { MAX_PANES, layoutRects, type Divider } from "$lib/layout";
  import { settings } from "$lib/stores/settings.svelte";
  import { adhocLabel, ui, type Pane, type Tab } from "$lib/stores/ui.svelte";
  import { writeToPane } from "$lib/terminalio";
  import { envInfo } from "$lib/types";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";
  import SnippetPicker from "./SnippetPicker.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  const tab = $derived(ui.activeTab);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");
  let tabMenu = $state<{ id: string; x: number; y: number } | null>(null);
  let dragTab = $state<string | null>(null);
  let dropIndex = $state<number | null>(null);

  function paneLabel(p: Pane) {
    if (p.target.kind === "adhoc") return adhocLabel(p.target.adhoc);
    if (p.target.kind === "local") return "Local shell";
    const h = vaultStore.hostById.get(p.target.hostId)?.data;
    return h ? `${h.label} · ${h.hostname}` : "host removed";
  }

  function paneEnv(p: Pane) {
    return p.target.kind === "host" ? envInfo(vaultStore.hostById.get(p.target.hostId)?.data?.environment) : envInfo("");
  }

  const hasProd = (t: Tab) => t.panes.some((p) => paneEnv(p).value === "production");

  /** Synchronized input: type once, every pane in the tab receives it. */
  function toggleSync(t: Tab) {
    if (!t.syncInput && hasProd(t)) {
      const names = t.panes.filter((p) => paneEnv(p).value === "production").map((p) => paneLabel(p).split(" · ")[0]);
      if (!confirm(`This tab includes production hosts:\n\n${names.join("\n")}\n\nType into all panes at once anyway?`)) return;
    }
    t.syncInput = !t.syncInput;
  }

  // Ctrl+Shift+B from anywhere toggles sync for the active tab.
  let lastSync = ui.syncRequest;
  $effect(() => {
    const n = ui.syncRequest;
    if (n === lastSync) return;
    lastSync = n;
    const t = ui.activeTab;
    if (t && t.panes.length > 1) toggleSync(t);
  });

  function broadcastFor(t: Tab) {
    return (data: string | Uint8Array) => {
      for (const p of t.panes) if (ui.paneInfo[p.id]?.status === "connected") void writeToPane(p, data);
    };
  }

  function tabColor(t: Tab) {
    const p = t.panes[0];
    return p?.target.kind === "host" ? vaultStore.hostById.get(p.target.hostId)?.data?.color : undefined;
  }

  const dot: Record<string, string> = {
    connected: "bg-success",
    connecting: "bg-warning animate-pulse",
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

  // -- divider dragging ------------------------------------------------------

  function startResize(e: PointerEvent, t: Tab, d: Divider, container: HTMLElement) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const box = container.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      const ratio =
        d.dir === "row"
          ? ((ev.clientX - box.left) / box.width * 100 - d.area.x) / d.area.w
          : ((ev.clientY - box.top) / box.height * 100 - d.area.y) / d.area.h;
      ui.resizeSplit(t.id, d.splitId, ratio);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  // -- session logs -------------------------------------------------------------

  async function toggleRecording(p: Pane) {
    const info = ui.paneInfo[p.id];
    if (info?.recording) {
      await api.sessionLogs.stop(p.id);
      ui.paneInfo[p.id] = { ...info, recording: undefined };
      ui.notify("info", `Session log saved to ${info.recording}`);
      return;
    }
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const name = paneLabel(p).split(" · ")[0].replace(/[^\w.@-]+/g, "_");
    const path = await save({
      title: "Record session to…",
      defaultPath: `${name}-${stamp}.log`,
      filters: [{ name: "Log", extensions: ["log", "txt"] }],
    });
    if (!path) return;
    try {
      const raw = settings.prefs.logRaw;
      await api.sessionLogs.start(p.id, path, !raw, `# SSHVault session log: ${paneLabel(p)}, started ${new Date().toLocaleString()}`);
      ui.paneInfo[p.id] = { ...(ui.paneInfo[p.id] ?? { status: "connected" }), recording: path };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- tab reordering ---------------------------------------------------------

  function onTabDragOver(e: DragEvent, index: number) {
    if (!dragTab) return;
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    dropIndex = e.clientX < r.left + r.width / 2 ? index : index + 1;
  }

  function onTabDrop(e: DragEvent) {
    e.preventDefault();
    if (dragTab && dropIndex !== null) {
      const from = ui.tabs.findIndex((t) => t.id === dragTab);
      ui.moveTab(dragTab, dropIndex > from ? dropIndex - 1 : dropIndex);
    }
    dragTab = null;
    dropIndex = null;
  }
</script>

<section class="flex min-w-0 flex-1 flex-col bg-base">
  {#if ui.tabs.length > 0}
    <div class="flex h-10 items-end gap-0.5 overflow-x-auto border-b border-line bg-panel px-2" role="tablist" tabindex="-1" ondrop={onTabDrop} ondragover={(e) => dragTab && e.preventDefault()}>
      {#each ui.tabs as t, i (t.id)}
        {@const color = tabColor(t)}
        <div
          class="group relative flex h-9 max-w-56 shrink-0 items-center gap-2 rounded-t-md px-3 text-sm
            {t.id === ui.activeTabId ? 'bg-base text-fg' : 'text-fg-muted hover:bg-panel-hover'}
            {dragTab === t.id ? 'opacity-40' : ''}"
          role="tab"
          tabindex="0"
          aria-selected={t.id === ui.activeTabId}
          title={i < 9 ? `Ctrl+${i + 1}` : undefined}
          draggable={renaming !== t.id}
          ondragstart={(e) => {
            dragTab = t.id;
            e.dataTransfer?.setData("text/plain", t.id);
            if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
          }}
          ondragend={() => {
            dragTab = null;
            dropIndex = null;
          }}
          ondragover={(e) => onTabDragOver(e, i)}
          onclick={() => (ui.activeTabId = t.id)}
          ondblclick={() => startRename(t)}
          onkeydown={(e) => e.key === "Enter" && (ui.activeTabId = t.id)}
          onauxclick={(e) => e.button === 1 && ui.closeTab(t.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            tabMenu = { id: t.id, x: e.clientX, y: e.clientY };
          }}
        >
          {#if dropIndex === i && dragTab && dragTab !== t.id}
            <span class="absolute -left-0.5 top-1 bottom-1 w-0.5 rounded bg-accent"></span>
          {/if}
          {#if dropIndex === i + 1 && i === ui.tabs.length - 1 && dragTab && dragTab !== t.id}
            <span class="absolute -right-0.5 top-1 bottom-1 w-0.5 rounded bg-accent"></span>
          {/if}
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
          {#if hasProd(t)}
            <span class="shrink-0 rounded bg-danger/15 px-1 text-[9px] font-bold text-danger">PROD</span>
          {/if}
          {#if t.syncInput}
            <span class="shrink-0 rounded bg-warning/15 px-1 text-[9px] font-bold text-warning" title="Typing goes to every pane">SYNC</span>
          {/if}
          {#if t.panes.some((p) => ui.paneInfo[p.id]?.recording)}
            <Circle size={8} class="shrink-0 fill-danger text-danger" />
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
      <button class="icon-btn mb-1 h-7 w-7 shrink-0" title="Local terminal (Ctrl+Shift+`)" onclick={() => ui.openLocal()}>
        <SquareTerminal size={15} />
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

  <!-- Every tab stays mounted so background sessions keep streaming. Within a
       tab, panes are a flat keyed list placed by the layout's rectangles, so
       splitting never remounts (and disconnects) an existing terminal. -->
  {#each ui.tabs as t (t.id)}
    {@const rects = layoutRects(t.layout)}
    {@const multi = t.panes.length > 1}
    <div class="relative min-h-0 flex-1 overflow-hidden {t.id === ui.activeTabId ? 'block' : 'hidden'}" data-tab-body={t.id}>
      {#each t.panes as pane (pane.id)}
        {@const r = rects.panes.get(pane.id)}
        {@const info = ui.paneInfo[pane.id]}
        {@const env = paneEnv(pane)}
        {#if r}
          <div
            class="absolute flex flex-col overflow-hidden
              {t.syncInput && multi ? 'ring-1 ring-inset ring-warning/60' : pane.id === t.activePaneId && multi ? 'ring-1 ring-inset ring-accent/40' : ''}"
            style:left="{r.x}%"
            style:top="{r.y}%"
            style:width="{r.w}%"
            style:height="{r.h}%"
            role="presentation"
            onmousedown={() => (t.activePaneId = pane.id)}
          >
            <div
              class="flex h-7 shrink-0 items-center gap-2 border-b px-3 text-xs text-fg-muted
                {env.value === 'production' ? 'border-danger/40 bg-danger/10' : 'border-line bg-panel/40'}"
            >
              {#if env.value}
                <span class="shrink-0 rounded px-1 text-[9px] font-bold {'cls' in env ? env.cls : ''}">{"short" in env ? env.short : ""}</span>
              {/if}
              <span class="truncate">{paneLabel(pane)}</span>
              {#if info?.remoteTitle}
                <span class="truncate text-fg-muted/70">— {info.remoteTitle}</span>
              {/if}
              <div class="flex-1"></div>
              {#if info?.mouseTracked}
                <button
                  class="icon-btn h-6 w-6 {info.selectMode ? 'text-accent' : ''}"
                  title={info.selectMode ? "Give the mouse back to the program (tmux, vim…)" : "Select text with the mouse even though the program is using it"}
                  aria-pressed={!!info.selectMode}
                  onclick={() => (ui.paneInfo[pane.id] = { ...info, selectMode: !info.selectMode })}
                >
                  <TextSelect size={13} />
                </button>
              {/if}
              {#if multi}
                <button
                  class="icon-btn h-6 w-6 {t.syncInput ? 'text-warning' : ''}"
                  title={t.syncInput ? "Stop typing into all panes (Ctrl+Shift+B)" : "Type into all panes at once (Ctrl+Shift+B)"}
                  onclick={() => toggleSync(t)}
                >
                  <Keyboard size={13} />
                </button>
              {/if}
              <button
                class="icon-btn h-6 w-6 {info?.recording ? 'text-danger' : ''}"
                title={info?.recording ? `Stop recording (${info.recording})` : "Record session to a file"}
                disabled={info?.status !== "connected" && !info?.recording}
                onclick={() => toggleRecording(pane)}
              >
                <Circle size={11} class={info?.recording ? "fill-danger animate-pulse" : ""} />
              </button>
              {#if t.panes.length < MAX_PANES}
                <button class="icon-btn h-6 w-6" title="Split right (Ctrl+Shift+D)" onclick={() => { t.activePaneId = pane.id; ui.splitActive("vertical"); }}><Columns2 size={13} /></button>
                <button class="icon-btn h-6 w-6" title="Split down (Ctrl+Shift+E)" onclick={() => { t.activePaneId = pane.id; ui.splitActive("horizontal"); }}><Rows2 size={13} /></button>
              {/if}
              {#if multi}
                <button class="icon-btn h-6 w-6" title="Close pane" onclick={() => ui.closePane(t.id, pane.id)}><X size={13} /></button>
              {/if}
            </div>
            <TerminalPane
              {pane}
              active={pane.id === t.activePaneId && t.id === ui.activeTabId}
              broadcast={t.syncInput && multi ? broadcastFor(t) : null}
            />
          </div>
        {/if}
      {/each}

      {#each rects.dividers as d (d.splitId)}
        <div
          class="group absolute z-10 flex items-center justify-center {d.dir === 'row' ? 'cursor-col-resize' : 'cursor-row-resize'}"
          style:left={d.dir === "row" ? `calc(${d.at.x}% - 3px)` : `${d.at.x}%`}
          style:top={d.dir === "row" ? `${d.at.y}%` : `calc(${d.at.y}% - 3px)`}
          style:width={d.dir === "row" ? "6px" : `${d.at.w}%`}
          style:height={d.dir === "row" ? `${d.at.h}%` : "6px"}
          role="separator"
          aria-orientation={d.dir === "row" ? "vertical" : "horizontal"}
          onpointerdown={(e) => startResize(e, t, d, (e.currentTarget as HTMLElement).parentElement!)}
        >
          <div class="bg-line transition-colors group-hover:bg-accent {d.dir === 'row' ? 'h-full w-px' : 'h-px w-full'}"></div>
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
