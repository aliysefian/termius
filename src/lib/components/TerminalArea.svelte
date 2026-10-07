<script lang="ts">
  import Illustration from "./Illustration.svelte";
  import { t as tr } from "$lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { keepInView } from "$lib/actions";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { sshCommand } from "$lib/sshcmd";
  import { ACTIONS, comboFor } from "$lib/shortcuts";
  import { ArrowLeftRight, BellRing, Check, ChevronDown, Circle, Columns2, Command, Copy, Ellipsis, FolderSync, Keyboard, List, Loader2, Maximize2, Minimize2, Plus, RefreshCw, Rows2, Search, SquareTerminal, Terminal, TextSelect, X, Zap } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import Kbd from "./Kbd.svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import * as api from "$lib/api";
  import { MAX_PANES, MIN_RATIO, layoutRects, type Divider } from "$lib/layout";
  import { settings } from "$lib/stores/settings.svelte";
  import { localShells } from "$lib/stores/localshells.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { paneLabel, STATUS_DOT, ui, type Pane, type Tab } from "$lib/stores/ui.svelte";
  import { writeToPane } from "$lib/terminalio";
  import { sftp } from "$lib/sftp";
  import { PAGE_VIEWS } from "$lib/stores/ui.svelte";
  import { envInfo } from "$lib/types";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";
  import SnippetPicker from "./SnippetPicker.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import RemotePane from "./RemotePane.svelte";

  const tab = $derived(ui.activeTab);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");
  let tabMenu = $state<{ id: string; x: number; y: number } | null>(null);
  let dragTab = $state<string | null>(null);
  let dropIndex = $state<number | null>(null);
  let tabStrip = $state<HTMLDivElement>();
  let overflowing = $state(false);
  let atStart = $state(true);
  let atEnd = $state(true);
  let tabListOpen = $state<{ x: number; y: number } | null>(null);
  let tabListQuery = $state("");
  let paneMenu = $state<{ tabId: string; paneId: string; x: number; y: number } | null>(null);


  function paneEnv(p: Pane) {
    return p.target.kind === "host" ? envInfo(vaultStore.hostById.get(p.target.hostId)?.data?.environment) : envInfo("");
  }

  const hasProd = (t: Tab) => t.panes.some((p) => paneEnv(p).value === "production");

  /** Synchronized input: type once, every pane in the tab receives it. */
  /** Single-quote a path for POSIX shells. */
  // Dropping OS files onto a terminal uploads them to the active pane's
  // host, at its known directory (OSC 7), then types the remote path at
  // the cursor so it's ready to use. Scoped to the active pane only: the
  // drop event carries a screen position, not which split pane it landed
  // on, and guessing from coordinates risked uploading to the wrong host.
  let shellMenu = $state<{ x: number; y: number } | null>(null);
  function openShellMenu(el: HTMLElement) {
    const r = el.getBoundingClientRect();
    shellMenu = { x: Math.max(4, Math.min(r.left, window.innerWidth - 330)), y: r.bottom + 4 };
    void localShells.ensure();
  }

  onMount(() => {
    let off: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
      off = await getCurrentWebview().onDragDropEvent((ev) => {
        if (PAGE_VIEWS.includes(ui.view) || ui.view === "sftp") return;
        const p = ev.payload;
        if (p.type !== "drop" || !p.paths.length) return;
        const tab = ui.activeTab;
        const pane = tab?.panes.find((x) => x.id === tab.activePaneId);
        if (pane?.target.kind !== "host") return;
        void uploadDropped(pane, pane.target.hostId, p.paths);
      });
    });
    return () => off?.();
  });

  async function uploadDropped(pane: Pane, hostId: string, localPaths: string[]) {
    const host = vaultStore.hostById.get(hostId)?.data;
    if (!host || !vaultStore.effectiveIdentity(host)) {
      ui.notify("error", "This host has no saved credentials for SFTP. Open it in the SFTP view to upload there.");
      return;
    }
    const info = ui.paneInfo[pane.id];
    const sessionId = `drop-${crypto.randomUUID()}`;
    const names = localPaths.map((p) => p.split(/[\\/]/).pop() ?? p);
    ui.notify("info", `Uploading ${names.length === 1 ? names[0] : `${names.length} files`} to ${host.label}…`);
    try {
      const opened = await sftp.open(sessionId, hostId, null);
      const destDir = info?.cwd || opened.home;
      const transferId = `drop-${crypto.randomUUID()}`;
      await sftp.transfer(sessionId, transferId, "upload", localPaths, destDir, () => {});
      await sftp.close(sessionId);
      const remotePaths = names.map((n) => `${destDir.replace(/\/$/, "")}/${n}`);
      ui.notify("info", `Uploaded to ${destDir}.`);
      void writeToPane(pane, remotePaths.map(shellQuote).join(" "));
    } catch (e) {
      await sftp.close(sessionId).catch(() => {});
      ui.notify("error", `Upload failed: ${errorMessage(e)}`);
    }
  }

  function shellQuote(s: string) {
    return `'${s.replace(/'/g, "'\\''")}'`;
  }

  async function toggleSync(t: Tab) {
    if (!t.syncInput && hasProd(t)) {
      const names = t.panes.filter((p) => paneEnv(p).value === "production").map((p) => paneLabel(p.target).split(" · ")[0]);
      if (!await ask(`This tab includes production hosts:\n\n${names.join("\n")}\n\nType into all panes at once anyway?`)) return;
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

  function startRename(t: Tab) {
    renaming = t.id;
    renameValue = t.customTitle ?? t.title;
  }

  function commitRename() {
    if (renaming) ui.renameTab(renaming, renameValue);
    renaming = null;
  }

  // -- divider dragging ------------------------------------------------------

  /** The divider's own current ratio, worked back out of where it's drawn. */
  function dividerRatio(d: Divider): number {
    return d.dir === "row" ? (d.at.x - d.area.x) / d.area.w : (d.at.y - d.area.y) / d.area.h;
  }

  /** Escape mid-drag puts the split back where it started. */
  let dividerCancel: (() => void) | null = null;

  function startResize(e: PointerEvent, t: Tab, d: Divider, container: HTMLElement) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    el.focus();
    const box = container.getBoundingClientRect();
    const startRatio = dividerRatio(d);
    const move = (ev: PointerEvent) => {
      const ratio =
        d.dir === "row"
          ? ((ev.clientX - box.left) / box.width * 100 - d.area.x) / d.area.w
          : ((ev.clientY - box.top) / box.height * 100 - d.area.y) / d.area.h;
      ui.resizeSplit(t.id, d.splitId, ratio);
    };
    const up = () => {
      dividerCancel = null;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    dividerCancel = () => {
      ui.resizeSplit(t.id, d.splitId, startRatio);
      up();
    };
  }

  /** Arrow keys nudge the split; Home/Enter centres it; double-click centres it too. */
  function dividerKeydown(e: KeyboardEvent, t: Tab, d: Divider) {
    const horizontal = d.dir === "row";
    const isForward = horizontal ? e.key === "ArrowRight" : e.key === "ArrowDown";
    const isBack = horizontal ? e.key === "ArrowLeft" : e.key === "ArrowUp";
    if (isForward || isBack) {
      e.preventDefault();
      const step = e.shiftKey ? 0.1 : 0.02;
      ui.resizeSplit(t.id, d.splitId, dividerRatio(d) + (isForward ? step : -step));
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      ui.resizeSplit(t.id, d.splitId, 0.5);
    }
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
    const name = paneLabel(p.target).split(" · ")[0].replace(/[^\w.@-]+/g, "_");
    const path = await save({
      title: "Record session to…",
      defaultPath: `${name}-${stamp}.log`,
      filters: [{ name: "Log", extensions: ["log", "txt"] }],
    });
    if (!path) return;
    try {
      const raw = settings.prefs.logRaw;
      await api.sessionLogs.start(p.id, path, !raw, `# SSHVault session log: ${paneLabel(p.target)}, started ${new Date().toLocaleString()}`);
      ui.paneInfo[p.id] = { ...(ui.paneInfo[p.id] ?? { status: "connected" }), recording: path };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- pane overflow menu -------------------------------------------------------

  function openPaneMenu(e: MouseEvent, tabId: string, paneId: string) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    paneMenu = { tabId, paneId, x: r.right, y: r.bottom + 4 };
  }

  async function copyPaneSsh(p: Pane) {
    if (p.target.kind !== "host") return;
    const host = vaultStore.hostById.get(p.target.hostId)?.data;
    if (!host) return;
    const cmd = sshCommand({
      host,
      hostId: p.target.hostId,
      hostById: vaultStore.hostById,
      identityById: vaultStore.identityById,
      identityFor: (h) => vaultStore.effectiveIdentity(h),
      jumpFor: (h, id) => vaultStore.effectiveJump(h, id),
      proxyFor: (h) => vaultStore.proxyById.get(vaultStore.effectiveProxy(h) ?? "")?.data?.spec,
    });
    try {
      await writeText(cmd);
      ui.notify("info", `Copied: ${cmd}`);
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

  // -- overflow: a fade on either scrolled edge, and a searchable list of
  // every tab once there are too many to see at once -------------------------

  function checkScroll() {
    if (!tabStrip) return;
    overflowing = tabStrip.scrollWidth > tabStrip.clientWidth + 1;
    atStart = tabStrip.scrollLeft <= 1;
    atEnd = tabStrip.scrollLeft + tabStrip.clientWidth >= tabStrip.scrollWidth - 1;
  }

  onMount(() => {
    if (!tabStrip) return;
    const ro = new ResizeObserver(checkScroll);
    ro.observe(tabStrip);
    tabStrip.addEventListener("scroll", checkScroll, { passive: true });
    checkScroll();
    return () => {
      ro.disconnect();
      tabStrip?.removeEventListener("scroll", checkScroll);
    };
  });

  // The strip's content width changes whenever a tab opens, closes or is
  // renamed; recheck once the DOM has caught up.
  $effect(() => {
    ui.tabs.length;
    queueMicrotask(checkScroll);
  });

  const splitRight = ACTIONS.find((a) => a.id === "split-right");
  const splitDown = ACTIONS.find((a) => a.id === "split-down");

  const filteredTabs = $derived(
    tabListQuery.trim() ? ui.tabs.filter((t) => (t.customTitle ?? t.title).toLowerCase().includes(tabListQuery.trim().toLowerCase())) : ui.tabs,
  );

  function openTabList(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    tabListQuery = "";
    tabListOpen = { x: r.left, y: r.bottom + 4 };
  }
</script>

{#if shellMenu}
  <div class="fixed inset-0 z-40" role="presentation" onclick={() => (shellMenu = null)} oncontextmenu={(e) => { e.preventDefault(); shellMenu = null; }}></div>
  <div
    class="fixed z-50 min-w-48 max-w-80 overflow-auto rounded-lg border border-line bg-panel py-1 text-xs shadow-xl"
    style="left:{shellMenu.x}px;top:{shellMenu.y}px;max-height:60vh"
    role="menu"
    aria-label="Local shells"
  >
    {#each localShells.list as sh (sh.id)}
      <button
        class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-hover"
        role="menuitem"
        title={sh.argv.join(" ")}
        onclick={() => { shellMenu = null; ui.openLocal(undefined, sh.kind === "wsl" ? sh.label.replace("WSL: ", "") : sh.label, sh.id); }}
      >
        <span class="truncate">{sh.label}</span>
        {#if sh.id === (localShells.defaultId ?? localShells.list.find((s) => s.is_default)?.id)}<span class="ml-auto text-fg-muted">default</span>{/if}
      </button>
    {:else}
      <p class="px-3 py-2 text-fg-muted">{localShells.loaded ? "No shells found." : "Looking…"}</p>
    {/each}
    <button class="flex w-full items-center gap-2 border-t border-line px-3 py-1.5 text-left text-fg-muted hover:bg-hover" role="menuitem" onclick={() => { shellMenu = null; void localShells.refresh(); }}>
      Look again
    </button>
  </div>
{/if}

<svelte:window
  onkeydown={(e) => {
    if (e.key !== "Escape") return;
    if (dividerCancel) dividerCancel();
    else if (tabMenu) tabMenu = null;
    else if (tabListOpen) tabListOpen = null;
    else if (paneMenu) paneMenu = null;
  }}
  onresize={() => { tabMenu = null; tabListOpen = null; paneMenu = null; checkScroll(); }}
/>

<section class="flex min-w-0 flex-1 flex-col bg-base">
  {#if ui.tabs.length > 0 && !settings.prefs.focusMode}
    <div class="flex h-10 items-stretch border-b border-line bg-panel">
    <div class="relative min-w-0 flex-1">
    <div bind:this={tabStrip} class="flex h-10 items-end gap-0.5 overflow-x-auto px-2" role="presentation" ondrop={onTabDrop} ondragover={(e) => dragTab && e.preventDefault()}>
      <!-- Only the tabs are in the tab list; the buttons after them are not tabs. -->
      <div class="flex items-end gap-0.5" role="tablist" aria-label="Open terminals">
      {#each ui.tabs as t, i (t.id)}
        {@const color = tabColor(t)}
        <div
          class="group relative flex h-9 max-w-56 shrink-0 items-center gap-2 rounded-t-md px-3 text-sm
            {t.id === ui.activeTabId ? 'bg-base text-fg' : 'text-fg-muted hover:bg-panel-hover'}
            {dragTab === t.id ? 'opacity-40' : ''}"
          role="presentation"
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
          onauxclick={(e) => e.button === 1 && void ui.requestCloseTab(t.id)}
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
          <div
            class="flex h-full min-w-0 flex-1 items-center gap-2 rounded outline-none focus-visible:ring-1 focus-visible:ring-accent"
            role="tab"
            tabindex="0"
            aria-selected={t.id === ui.activeTabId}
            onkeydown={(e) => e.key === "Enter" && (ui.activeTabId = t.id)}
          >
          <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[ui.tabStatus(t)]}"></span>
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
            <Badge tone="danger">PROD</Badge>
          {/if}
          {#if t.syncInput}
            <Badge tone="warning" title="Typing goes to every pane">SYNC</Badge>
          {/if}
          {#if t.panes.some((p) => ui.paneInfo[p.id]?.recording)}
            <Circle size={8} class="shrink-0 fill-danger text-danger" />
          {/if}
          {#if t.id !== ui.activeTabId && t.panes.some((p) => ui.paneInfo[p.id]?.running)}
            <Loader2 size={11} class="shrink-0 animate-spin text-fg-muted" aria-label="A command is running" />
          {/if}
          {#if t.id !== ui.activeTabId && t.panes.some((p) => ui.paneInfo[p.id]?.bell)}
            <BellRing size={11} class="shrink-0 text-warning" aria-label="Bell" />
          {:else if t.id !== ui.activeTabId && t.panes.some((p) => ui.paneInfo[p.id]?.unread)}
            <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" aria-label="New output" title="New output"></span>
          {/if}
          </div>
          <button
            class="reveal rounded p-0.5 hover:bg-panel-hover {t.id === ui.activeTabId ? 'opacity-60' : ''}"
            onclick={(e) => { e.stopPropagation(); void ui.requestCloseTab(t.id); }}
            aria-hidden="true"
            tabindex="-1"
            title="Close (Ctrl+Shift+W)"
          >
            <X size={12} />
          </button>
        </div>
      {/each}
      </div>
      <button class="icon-btn mb-1 h-7 w-7 shrink-0" title="New connection (Ctrl+Shift+T)" onclick={() => (ui.modal = { kind: "quick-connect" })}>
        <Plus size={15} />
      </button>
      <button class="icon-btn mb-1 h-7 w-7 shrink-0" title="Local terminal (Ctrl+Shift+`)" onclick={() => ui.openLocal()}>
        <SquareTerminal size={15} />
      </button>
      <button
        class="icon-btn mb-1 -ml-1 h-7 w-4 shrink-0"
        title="Choose a shell"
        aria-label="Choose a shell for a new local terminal"
        aria-haspopup="menu"
        aria-expanded={shellMenu !== null}
        onclick={(e) => openShellMenu(e.currentTarget)}
      >
        <ChevronDown size={12} />
      </button>
    </div>
    {#if !atStart}
      <div class="pointer-events-none absolute inset-y-0 left-0 w-6 bg-gradient-to-r from-panel to-transparent"></div>
    {/if}
    {#if !atEnd}
      <div class="pointer-events-none absolute inset-y-0 right-0 w-6 bg-gradient-to-l from-panel to-transparent"></div>
    {/if}
    </div>
    {#if overflowing}
      <button class="icon-btn mb-1 h-7 w-7 shrink-0 self-end" title="List all tabs" aria-label="List all tabs" onclick={openTabList}>
        <List size={15} />
      </button>
    {/if}
    <div class="flex h-10 shrink-0 items-center gap-0.5 px-1">
      <button class="icon-btn h-8 w-8" title="Command palette (Ctrl+Shift+P)" onclick={() => (ui.paletteOpen = true)}><Command size={15} /></button>
      <SnippetPicker />
    </div>
    </div>
  {/if}

  {#if tabListOpen}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (tabListOpen = null)}></button>
    <div class="fixed z-50 w-64 overflow-hidden rounded-md border border-line bg-panel shadow-2xl" use:keepInView={tabListOpen} role="menu">
      <div class="relative border-b border-line p-1.5">
        <Search size={12} class="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-fg-muted" />
        <!-- svelte-ignore a11y_autofocus -->
        <input
          autofocus
          class="input py-1 pl-7 text-xs"
          placeholder="Filter tabs…"
          bind:value={tabListQuery}
          onkeydown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              tabListOpen = null;
            } else if (e.key === "Enter" && filteredTabs[0]) {
              ui.activeTabId = filteredTabs[0].id;
              tabListOpen = null;
            }
          }}
        />
      </div>
      <div class="max-h-72 overflow-y-auto py-1">
        {#each filteredTabs as t (t.id)}
          <button
            class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-panel-hover {t.id === ui.activeTabId ? 'text-accent' : ''}"
            role="menuitem"
            onclick={() => {
              ui.activeTabId = t.id;
              tabListOpen = null;
            }}
          >
            <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[ui.tabStatus(t)]}"></span>
            <span class="min-w-0 flex-1 truncate">{t.customTitle ?? t.title}</span>
            {#if t.panes.some((p) => ui.paneInfo[p.id]?.unread)}<span class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent"></span>{/if}
          </button>
        {:else}
          <p class="px-3 py-4 text-center text-xs text-fg-muted">No matches.</p>
        {/each}
      </div>
    </div>
  {/if}

  {#if paneMenu}
    {@const pm = paneMenu}
    {@const menuTab = ui.tabs.find((x) => x.id === pm.tabId)}
    {@const menuPane = menuTab?.panes.find((x) => x.id === pm.paneId)}
    {@const menuInfo = ui.paneInfo[pm.paneId]}
    {@const menuMulti = (menuTab?.panes.length ?? 0) > 1}
    {@const menuFull = menuTab?.zoomedPaneId === pm.paneId}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (paneMenu = null)} oncontextmenu={(e) => { e.preventDefault(); paneMenu = null; }}></button>
    <div class="fixed z-50 w-56 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" use:keepInView={pm} role="menu">
      {#if menuTab && menuPane}
        {#if menuInfo?.status === "error" || menuInfo?.status === "disconnected"}
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.requestReconnect(menuPane.id); paneMenu = null; }}>
            <RefreshCw size={13} /> Reconnect
          </button>
        {/if}
        <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.duplicatePane(menuTab.id, menuPane.id); paneMenu = null; }}>
          <Copy size={13} /> Duplicate pane
        </button>
        {#if menuPane.target.kind === "host"}
          {@const hid = menuPane.target.hostId}
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.openSftpAt(hid, menuInfo?.cwd ?? ""); paneMenu = null; }}>
            <FolderSync size={13} /> Open SFTP here
          </button>
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void copyPaneSsh(menuPane); paneMenu = null; }}>
            <SquareTerminal size={13} /> Copy ssh command
          </button>
        {/if}
        <div class="my-1 border-t border-line"></div>
        {#if menuInfo?.mouseTracked}
          <button class="flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left hover:bg-panel-hover {menuInfo.selectMode ? 'text-accent' : ''}" role="menuitem" onclick={() => { ui.paneInfo[menuPane.id] = { ...menuInfo, selectMode: !menuInfo.selectMode }; paneMenu = null; }}>
            <span class="flex items-center gap-2"><TextSelect size={13} /> Select text with the mouse</span>
            {#if menuInfo.selectMode}<Check size={13} />{/if}
          </button>
        {/if}
        {#if menuMulti}
          <button class="flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left hover:bg-panel-hover {menuTab.syncInput ? 'text-warning' : ''}" role="menuitem" onclick={() => { toggleSync(menuTab); paneMenu = null; }}>
            <span class="flex items-center gap-2"><Keyboard size={13} /> Type into all panes</span>
            {#if menuTab.syncInput}<Check size={13} />{/if}
          </button>
        {/if}
        <button
          class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 disabled:hover:bg-transparent {menuInfo?.recording ? 'text-danger' : ''}"
          role="menuitem"
          disabled={menuInfo?.status !== "connected" && !menuInfo?.recording}
          onclick={() => { toggleRecording(menuPane); paneMenu = null; }}
        >
          <Circle size={13} class={menuInfo?.recording ? "fill-danger" : ""} /> {menuInfo?.recording ? "Stop recording" : "Record session to a file"}
        </button>
        {#if menuMulti}
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.toggleZoom(menuTab.id, menuPane.id); paneMenu = null; }}>
            {#if menuFull}<Minimize2 size={13} /> Restore the split{:else}<Maximize2 size={13} /> Maximize this pane{/if}
          </button>
        {/if}
        {#if menuTab.panes.length === 2}
          {@const other = menuTab.panes.find((p) => p.id !== menuPane.id)}
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { if (other) ui.swapPanes(menuTab.id, menuPane.id, other.id); paneMenu = null; }}>
            <ArrowLeftRight size={13} /> Swap panes
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  {#if tabMenu}
    {@const menuTab = tabMenu.id}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (tabMenu = null)} oncontextmenu={(e) => { e.preventDefault(); tabMenu = null; }}></button>
    <div class="fixed z-50 w-48 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" use:keepInView={tabMenu} role="menu">
      <!-- Each action gets the tab id as an argument: `menuTab` is derived from
           `tabMenu`, so reading it after the menu closes would throw. -->
      {#each [
        { label: "Rename", run: (id: string) => { const t = ui.tabs.find((x) => x.id === id); if (t) startRename(t); } },
        { label: "Duplicate", run: (id: string) => ui.duplicateTab(id) },
        { label: "Close", run: (id: string): void => void ui.requestCloseTab(id) },
        { label: "Close other tabs", run: (id: string): void => void ui.requestCloseOtherTabs(id), disabled: ui.tabs.length < 2 },
      ] as item (item.label)}
        <button
          class="w-full px-3 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 disabled:hover:bg-transparent"
          role="menuitem"
          disabled={item.disabled}
          onclick={() => {
            const id = menuTab;
            tabMenu = null;
            item.run(id);
          }}
        >
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
    {@const zoomed = multi ? t.zoomedPaneId : undefined}
    <div class="relative min-h-0 flex-1 overflow-hidden {t.id === ui.activeTabId ? 'block' : 'hidden'}" data-tab-body={t.id}>
      {#each t.panes as pane (pane.id)}
        {@const r = rects.panes.get(pane.id)}
        {@const info = ui.paneInfo[pane.id]}
        {@const env = paneEnv(pane)}
        {#if r}
          {@const full = zoomed === pane.id}
          <div
            class="absolute flex flex-col overflow-hidden
              {zoomed && !full ? 'invisible' : ''}
              {t.syncInput && multi ? 'ring-1 ring-inset ring-warning/60' : pane.id === t.activePaneId && multi && !full ? 'ring-1 ring-inset ring-accent/40' : ''}"
            style:left="{full ? 0 : r.x}%"
            style:top="{full ? 0 : r.y}%"
            style:width="{full ? 100 : r.w}%"
            style:height="{full ? 100 : r.h}%"
            role="presentation"
            onmousedown={() => (t.activePaneId = pane.id)}
          >
            <div
              class="{settings.prefs.focusMode ? 'hidden' : 'flex'} h-7 shrink-0 items-center gap-2 border-b px-3 text-xs text-fg-muted
                {env.value === 'production' ? 'border-danger/40 bg-danger/10' : 'border-line bg-panel/40'}"
              role="presentation"
              ondblclick={() => multi && ui.toggleZoom(t.id, pane.id)}
              title={multi ? "Double-click to maximize or restore (Ctrl+Shift+Enter)" : undefined}
            >
              {#if env.value}
                <Badge tone={"tone" in env ? env.tone : undefined}>{"short" in env ? env.short : ""}</Badge>
              {/if}
              <span class="truncate">{paneLabel(pane.target)}</span>
              {#if pane.target.kind === "telnet" || (pane.target.kind === "host" && vaultStore.hostById.get(pane.target.hostId)?.data?.protocol === "telnet")}
                <Badge tone="warning" title="Telnet sends everything, including passwords, unencrypted">UNENCRYPTED</Badge>
              {/if}
              {#if info?.remoteTitle}
                <span class="truncate text-fg-muted">— {info.remoteTitle}</span>
              {/if}
              {#if info?.cwd && pane.target.kind === "host"}
                {@const hid = pane.target.hostId}
                {@const cwd = info.cwd}
                <span class="truncate font-mono text-fg-muted" title={cwd}>{cwd.replace(/^\/home\/[^/]+/, "~")}</span>
                <button class="icon-btn h-6 w-6" title="Browse {cwd} in SFTP" onclick={() => ui.openSftpAt(hid, cwd)}><FolderSync size={12} /></button>
                <button class="icon-btn h-6 w-6" title="New tab in {cwd}" onclick={() => ui.openTerminal(hid, paneLabel(pane.target), `cd ${shellQuote(cwd)}`)}><SquareTerminal size={12} /></button>
              {/if}
              <div class="flex-1"></div>
              {#if info?.recording}
                <span title="Recording to {info.recording}"><Circle size={10} class="shrink-0 fill-danger text-danger animate-pulse" aria-label="Recording" /></span>
              {/if}
              <button class="icon-btn h-6 w-6" title="More pane actions" aria-label="More pane actions" onclick={(e) => openPaneMenu(e, t.id, pane.id)}>
                <Ellipsis size={14} />
              </button>
              {#if t.panes.length < MAX_PANES}
                <button class="icon-btn h-6 w-6" title="Split right (Ctrl+Shift+D)" onclick={() => { t.activePaneId = pane.id; ui.splitActive("vertical"); }}><Columns2 size={13} /></button>
                <button class="icon-btn h-6 w-6" title="Split down (Ctrl+Shift+E)" onclick={() => { t.activePaneId = pane.id; ui.splitActive("horizontal"); }}><Rows2 size={13} /></button>
              {/if}
              {#if multi}
                <button class="icon-btn h-6 w-6" title="Close pane" onclick={() => void ui.requestClosePane(t.id, pane.id)}><X size={13} /></button>
              {/if}
            </div>
            {#if pane.target.kind === "host" && vaultStore.hostById.get(pane.target.hostId)?.data?.protocol === "rdp"}
              <RemotePane {pane} active={pane.id === t.activePaneId && t.id === ui.activeTabId} />
            {:else}
              <TerminalPane
                {pane}
                active={pane.id === t.activePaneId && t.id === ui.activeTabId}
                broadcast={t.syncInput && multi ? broadcastFor(t) : null}
              />
            {/if}
          </div>
        {/if}
      {/each}

      {#each zoomed ? [] : rects.dividers as d (d.splitId)}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="group absolute z-10 flex items-center justify-center outline-none {d.dir === 'row' ? 'cursor-col-resize' : 'cursor-row-resize'}"
          style:left={d.dir === "row" ? `calc(${d.at.x}% - 3px)` : `${d.at.x}%`}
          style:top={d.dir === "row" ? `${d.at.y}%` : `calc(${d.at.y}% - 3px)`}
          style:width={d.dir === "row" ? "6px" : `${d.at.w}%`}
          style:height={d.dir === "row" ? `${d.at.h}%` : "6px"}
          role="separator"
          aria-orientation={d.dir === "row" ? "vertical" : "horizontal"}
          aria-label="Resize split"
          aria-valuenow={Math.round(dividerRatio(d) * 100)}
          aria-valuemin={Math.round(MIN_RATIO * 100)}
          aria-valuemax={Math.round((1 - MIN_RATIO) * 100)}
          tabindex="0"
          title="Drag to resize · double-click to centre"
          onpointerdown={(e) => startResize(e, t, d, (e.currentTarget as HTMLElement).parentElement!)}
          ondblclick={() => ui.resizeSplit(t.id, d.splitId, 0.5)}
          onkeydown={(e) => dividerKeydown(e, t, d)}
        >
          <div class="bg-line transition-colors group-hover:bg-accent focus-visible:bg-accent {d.dir === 'row' ? 'h-full w-px' : 'h-px w-full'}"></div>
        </div>
      {/each}
    </div>
  {/each}

  {#if !tab}
    <div class="flex flex-1 flex-col items-center justify-center p-6 text-center">
      <div class="anim-rise mb-2"><Illustration scene="terminal" size={150} /></div>
      <h2 class="anim-rise text-[16px] leading-6 font-semibold" style="--i:1">{tr("welcome.title")}</h2>
      <p class="mt-1 max-w-sm text-sm text-fg-muted">{tr("welcome.text")}</p>
      <div class="mt-5 flex gap-2">
        <button class="btn-primary" onclick={() => (ui.modal = { kind: "quick-connect" })}><Zap size={14} /> {tr("welcome.quick")}</button>
        <button class="btn-secondary" onclick={() => (ui.paletteOpen = true)}><Command size={14} /> {tr("welcome.palette")}</button>
      </div>
      <div class="mt-6 grid grid-cols-2 gap-x-6 gap-y-1.5 text-left text-xs text-fg-muted">
        {#each [
          ["palette", "Command palette"],
          ["quick-connect", "Quick connect"],
          ["next-tab", "Next tab"],
          ["close-tab", "Close tab"],
          ["find", "Find in terminal"],
        ] as [id, label] (id)}
          {@const a = ACTIONS.find((x) => x.id === id)}
          {#if a && comboFor(a)}
            <Kbd keys={comboFor(a)} />
            <span class="self-center">{label}</span>
          {/if}
        {/each}
        {#if splitRight && splitDown && (comboFor(splitRight) || comboFor(splitDown))}
          <Kbd keys="{comboFor(splitRight) || "—"} / {comboFor(splitDown) || "—"}" />
          <span class="self-center">Split right / down</span>
        {/if}
        <!-- Not remappable actions, so always accurate without reading settings. -->
        <Kbd keys="Ctrl+Shift+C / V" />
        <span class="self-center">Copy / paste</span>
        <Kbd keys="Ctrl+= / Ctrl+-" />
        <span class="self-center">Zoom in / out</span>
      </div>
    </div>
  {/if}
</section>
