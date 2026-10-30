<script lang="ts">
  import { Activity, Code, FolderSync, FolderTree, History, Info, X } from "lucide-svelte";
  import ProcessesPanel from "./ProcessesPanel.svelte";
  import Badge from "./Badge.svelte";
  import FilesBrowser from "./FilesBrowser.svelte";
  import SnippetsPanel from "./SnippetsPanel.svelte";
  import { RIGHT_DEFAULT, RIGHT_MIN, rightMax, rightWidth } from "$lib/docklayout";
  import { sessionFacts } from "$lib/sessioninfo";
  import { settings } from "$lib/stores/settings.svelte";
  import { STATUS_DOT, paneLabel, ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo } from "$lib/types";

  let { windowWidth, leftUsed }: { windowWidth: number; leftUsed: number } = $props();
  /** The gap between this panel and the terminal, inside the dock so it closes with it. */
  const GUTTER = 6;

  const open = $derived(settings.prefs.rightPanelOpen);
  const width = $derived(rightWidth(settings.prefs.rightPanelWidth, windowWidth, leftUsed));
  const tabs = [
    { id: "files", label: "Files", icon: FolderTree },
    { id: "processes", label: "Processes", icon: Activity },
    { id: "session", label: "Session", icon: Info },
    { id: "snippets", label: "Snippets", icon: Code },
  ] as const;

  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;
  let frame = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startWidth = width;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    // The handle is on the left edge: dragging left makes the dock wider.
    const raw = startWidth - (e.clientX - startX);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => dragging && (settings.prefs.rightPanelWidth = rightWidth(raw, windowWidth, leftUsed)));
  }
  function stop() {
    cancelAnimationFrame(frame);
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      settings.prefs.rightPanelWidth = rightWidth(width + (e.key === "ArrowLeft" ? step : -step), windowWidth, leftUsed);
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.rightPanelWidth = RIGHT_DEFAULT;
    }
  }

  // -- Session tab: what is known about the focused pane -----------------------------------------
  const tab = $derived(ui.activeTab);
  const pane = $derived(tab?.panes.find((p) => p.id === tab.activePaneId));
  const info = $derived(pane ? ui.paneInfo[pane.id] : undefined);
  const hostId = $derived(pane?.target.kind === "host" ? pane.target.hostId : undefined);
  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const facts = $derived(pane ? sessionFacts(pane.target, host, hostId ? vaultStore.health[hostId] : undefined) : undefined);
  const env = $derived(envInfo(host?.environment));
  const rows = $derived.by(() => {
    if (!pane || !facts) return [] as [string, string][];
    const r: [string, string][] = [["Protocol", facts.protocol]];
    if (host?.hostname) r.push(["Address", `${host.hostname}${host.port && host.port !== 22 ? ":" + host.port : ""}`]);
    if (facts.latencyMs !== null) r.push(["Latency", `${facts.latencyMs} ms`]);
    if (facts.term) r.push(["TERM", facts.term]);
    if (facts.encoding) r.push(["Encoding", facts.encoding]);
    if (info?.cols && info.rows) r.push(["Size", `${info.cols}×${info.rows}`]);
    if (info?.cwd) r.push(["Folder", info.cwd]);
    return r;
  });
</script>

<aside
  class="relative shrink-0 overflow-hidden {dragging ? '' : 'transition-[width] duration-150 ease-out'}"
  style:width="{open ? width + GUTTER : 0}px"
  aria-label="Right panel"
  aria-hidden={!open}
  inert={!open}
>
  <!-- Content keeps its width and is clipped while the dock opens or closes, so it slides instead of squashing. -->
  <div class="@container ml-1.5 flex h-full flex-col overflow-hidden rounded-lg border border-line bg-panel" style:width="{width}px">
    <div class="flex h-10 shrink-0 items-center gap-1 border-b border-line px-2" role="tablist" aria-label="Right panel">
      {#each tabs as t (t.id)}
        <button
          class="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs {settings.prefs.rightPanelTab === t.id ? 'bg-accent/15 font-medium text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          role="tab"
          aria-selected={settings.prefs.rightPanelTab === t.id}
          title={t.label}
          onclick={() => (settings.prefs.rightPanelTab = t.id)}><t.icon size={13} /><span class="@max-[20rem]:sr-only">{t.label}</span></button
        >
      {/each}
      <div class="flex-1"></div>
      <button class="icon-btn h-6 w-6" title="Hide right panel (Ctrl+Alt+B)" aria-label="Hide right panel" onclick={() => ui.toggleRightPanel()}><X size={14} /></button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if settings.prefs.rightPanelTab === "files"}
        <FilesBrowser active={open && settings.prefs.rightPanelTab === "files"} />
      {:else if settings.prefs.rightPanelTab === "processes"}
        <ProcessesPanel active={open && settings.prefs.rightPanelTab === "processes"} />
      {:else if settings.prefs.rightPanelTab === "snippets"}
        <div class="flex h-full"><SnippetsPanel /></div>
      {:else if !pane || !facts}
        <p class="p-4 text-sm text-fg-muted">Open a terminal to see its session here.</p>
      {:else}
        <div class="space-y-4 p-4 text-sm">
          <div>
            <div class="flex items-center gap-2">
              <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[info?.status ?? 'disconnected']}"></span>
              <span class="min-w-0 flex-1 truncate font-medium">{tab?.customTitle ?? paneLabel(pane.target)}</span>
              {#if env.value}<Badge tone={"tone" in env ? env.tone : undefined}>{"short" in env ? env.short : ""}</Badge>{/if}
            </div>
            {#if info?.remoteTitle}<p class="mt-1 truncate text-xs text-fg-muted">{info.remoteTitle}</p>{/if}
          </div>
          <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs">
            {#each rows as [k, v] (k)}
              <dt class="text-fg-muted">{k}</dt>
              <dd class="min-w-0 break-words font-mono" title={v}>{v}</dd>
            {/each}
          </dl>
          {#if hostId}
            {@const hid = hostId}
            <div class="flex flex-wrap gap-2">
              <button class="btn-secondary" onclick={() => ui.openSftpAt(hid, info?.cwd ?? "")}><FolderSync size={14} /> Browse files</button>
              <button class="btn-secondary" onclick={() => (ui.configHistory = { hostId: hid })}><History size={14} /> Config history</button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if open}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute left-0.5 top-0 z-20 h-full w-2 cursor-col-resize outline-none
        after:absolute after:left-0 after:top-0 after:h-full after:w-0.5 after:transition-colors
        {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the right panel"
      aria-valuenow={width}
      aria-valuemin={RIGHT_MIN}
      aria-valuemax={rightMax(windowWidth)}
      tabindex="0"
      title="Drag to resize · double-click to reset"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={stop}
      onpointercancel={stop}
      onlostpointercapture={() => dragging && stop()}
      ondblclick={() => (settings.prefs.rightPanelWidth = RIGHT_DEFAULT)}
      onkeydown={key}
    ></div>
  {/if}
</aside>
