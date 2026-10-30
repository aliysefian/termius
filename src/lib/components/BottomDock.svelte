<script lang="ts">
  import { Trash2, X } from "lucide-svelte";
  import { BOTTOM_DEFAULT, BOTTOM_MIN, bottomHeight, bottomMax } from "$lib/docklayout";
  import { connectionLog } from "$lib/stores/connectionlog.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { STATUS_DOT, paneLabel, ui } from "$lib/stores/ui.svelte";

  let { windowHeight }: { windowHeight: number } = $props();

  const open = $derived(settings.prefs.bottomPanelOpen);
  const height = $derived(bottomHeight(settings.prefs.bottomPanelHeight, windowHeight));
  const tabs = [
    { id: "sessions", label: "Sessions" },
    { id: "connections", label: "Connections" },
  ] as const;

  let dragging = $state(false);
  let startY = 0;
  let startHeight = 0;
  let frame = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startY = e.clientY;
    startHeight = height;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    // The handle is on the top edge: dragging up makes the dock taller.
    const raw = startHeight - (e.clientY - startY);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => dragging && (settings.prefs.bottomPanelHeight = bottomHeight(raw, windowHeight)));
  }
  function stop() {
    cancelAnimationFrame(frame);
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      settings.prefs.bottomPanelHeight = bottomHeight(height + (e.key === "ArrowUp" ? step : -step), windowHeight);
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.bottomPanelHeight = BOTTOM_DEFAULT;
    }
  }

  const time = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  function took(e: { startedAt: number; endedAt: number | null }): string {
    if (!e.endedAt) return "open";
    const s = Math.round((e.endedAt - e.startedAt) / 1000);
    return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
  }

  /** Every pane in every tab, so a session in a background tab is one click away. */
  const sessions = $derived(ui.tabs.flatMap((t) => t.panes.map((p) => ({ tab: t, pane: p }))));
</script>

<section
  class="relative shrink-0 overflow-hidden border-t border-line bg-panel {dragging ? '' : 'transition-[height] duration-150 ease-out'}"
  style:height="{open ? height : 0}px"
  style:border-top-width={open ? "1px" : "0"}
  aria-label="Bottom panel"
  aria-hidden={!open}
  inert={!open}
>
  <div class="flex flex-col" style:height="{height}px">
    <div class="flex h-8 shrink-0 items-center gap-1 border-b border-line px-2" role="tablist" aria-label="Bottom panel">
      {#each tabs as t (t.id)}
        <button
          class="h-6 rounded-md px-2.5 text-xs {settings.prefs.bottomPanelTab === t.id ? 'bg-accent/15 font-medium text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          role="tab"
          aria-selected={settings.prefs.bottomPanelTab === t.id}
          onclick={() => (settings.prefs.bottomPanelTab = t.id)}>{t.label}</button
        >
      {/each}
      <div class="flex-1"></div>
      {#if settings.prefs.bottomPanelTab === "connections" && connectionLog.entries.length}
        <button class="icon-btn h-6 w-6" title="Clear the connection log" aria-label="Clear the connection log" onclick={() => connectionLog.clear()}><Trash2 size={13} /></button>
      {/if}
      <button class="icon-btn h-6 w-6" title="Hide bottom panel (Ctrl+Alt+J)" aria-label="Hide bottom panel" onclick={() => ui.toggleBottomPanel()}><X size={14} /></button>
    </div>

    <div class="min-h-0 flex-1 overflow-auto text-xs">
      {#if settings.prefs.bottomPanelTab === "connections"}
        {#each connectionLog.entries.slice(0, 200) as e (e.id)}
          <div class="flex items-center gap-3 border-b border-line/50 px-3 py-1.5">
            <span class="w-[4.5rem] shrink-0 whitespace-nowrap tabular-nums text-fg-muted">{time(e.startedAt)}</span>
            <span class="min-w-0 flex-1 truncate">{e.label}</span>
            <span class="shrink-0 text-fg-muted">{e.kind}</span>
            <span class="w-14 shrink-0 text-right tabular-nums text-fg-muted">{took(e)}</span>
            <span class="w-16 shrink-0 text-right {e.reason === 'failed' || e.reason === 'dropped' ? 'text-danger' : 'text-fg-muted'}">{e.reason ?? ""}</span>
          </div>
        {:else}
          <p class="p-4 text-fg-muted">No connections yet.</p>
        {/each}
      {:else}
        {#each sessions as { tab: t, pane: p } (p.id)}
          {@const status = ui.paneInfo[p.id]?.status ?? "disconnected"}
          <button
            class="flex w-full items-center gap-3 border-b border-line/50 px-3 py-1.5 text-left hover:bg-panel-hover {t.id === ui.activeTabId && p.id === t.activePaneId ? 'bg-accent/10' : ''}"
            onclick={() => {
              ui.activeTabId = t.id;
              t.activePaneId = p.id;
            }}
          >
            <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[status]}"></span>
            <span class="min-w-0 flex-1 truncate">{t.customTitle ?? paneLabel(p.target)}</span>
            <span class="min-w-0 max-w-[40%] truncate font-mono text-fg-muted">{ui.paneInfo[p.id]?.cwd ?? ""}</span>
            <span class="shrink-0 text-fg-muted">{status}</span>
          </button>
        {:else}
          <p class="p-4 text-fg-muted">No open sessions.</p>
        {/each}
      {/if}
    </div>
  </div>

  {#if open}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute left-0 top-0 z-20 h-2 w-full cursor-row-resize outline-none
        after:absolute after:left-0 after:top-0 after:h-0.5 after:w-full after:transition-colors
        {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize the bottom panel"
      aria-valuenow={height}
      aria-valuemin={BOTTOM_MIN}
      aria-valuemax={bottomMax(windowHeight)}
      tabindex="0"
      title="Drag to resize · double-click to reset"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={stop}
      onpointercancel={stop}
      onlostpointercapture={() => dragging && stop()}
      ondblclick={() => (settings.prefs.bottomPanelHeight = BOTTOM_DEFAULT)}
      onkeydown={key}
    ></div>
  {/if}
</section>
