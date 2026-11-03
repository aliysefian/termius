<script lang="ts">
  import { onMount } from "svelte";
  import ConnectionContext from "./databases/ConnectionContext.svelte";
  import DocTabs from "./databases/DocTabs.svelte";
  import ExplorerPanel from "./databases/ExplorerPanel.svelte";
  import Inspector from "./databases/Inspector.svelte";
  import PanelResizer from "./databases/PanelResizer.svelte";
  import QueryDoc from "./databases/QueryDoc.svelte";
  import TableDoc from "./databases/TableDoc.svelte";
  import Welcome from "./databases/Welcome.svelte";
  import { EXPLORER_DEFAULT, EXPLORER_MAX, EXPLORER_MIN, INSPECTOR_DEFAULT, INSPECTOR_MAX, INSPECTOR_MIN, layoutFor } from "$lib/dblayout";
  import { databases } from "$lib/stores/databases.svelte";
  import { dbWorkspace } from "$lib/stores/dbworkspace.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import type { Uuid } from "$lib/types";

  let width = $state(1400);
  let now = $state(Date.now());
  let queryDoc = $state<{ focus(): void }>();

  // Keeps elapsed times and ages moving.
  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(t);
  });

  const prefs = $derived(settings.prefs);
  const layout = $derived(layoutFor({ width, explorerOpen: prefs.dbExplorerOpen, inspectorOpen: prefs.dbInspectorOpen, explorerWidth: prefs.dbExplorerWidth, inspectorWidth: prefs.dbInspectorWidth }));
  const tab = $derived(databases.active);

  // A panel that has no room beside the editor opens over it, on request, and goes away when it is used.
  function showExplorer() {
    prefs.dbExplorerOpen = true;
    if (!layoutFor({ width, explorerOpen: true, inspectorOpen: prefs.dbInspectorOpen, explorerWidth: prefs.dbExplorerWidth, inspectorWidth: prefs.dbInspectorWidth }).explorerDocked) dbWorkspace.explorerDrawerOpen = true;
  }
  function hideExplorer() {
    prefs.dbExplorerOpen = false;
    dbWorkspace.explorerDrawerOpen = false;
  }
  function showInspector() {
    prefs.dbInspectorOpen = true;
    if (!layoutFor({ width, explorerOpen: prefs.dbExplorerOpen, inspectorOpen: true, explorerWidth: prefs.dbExplorerWidth, inspectorWidth: prefs.dbInspectorWidth }).inspectorDocked) dbWorkspace.inspectorDrawerOpen = true;
  }
  function hideInspector() {
    prefs.dbInspectorOpen = false;
    dbWorkspace.inspectorDrawerOpen = false;
  }

  function newQuery(connId: Uuid, sql = "") {
    const t = databases.newTab(connId, sql);
    dbWorkspace.selectDoc(t.id);
    void databases.connect(connId);
    dbWorkspace.explorerDrawerOpen = false;
  }

  function newQueryAnywhere() {
    const conn = tab?.connId ?? (dbWorkspace.focus && "connId" in dbWorkspace.focus ? dbWorkspace.focus.connId : Object.keys(databases.sessions)[0]);
    if (conn) newQuery(conn);
    else ui.modal = { kind: "db-connection", id: null };
  }

  function focusSearch() {
    if (!layout.explorerDocked) showExplorer();
    queueMicrotask(() => document.querySelector<HTMLInputElement>('[aria-label="Search database objects"]')?.focus());
  }

  function showConnectionDetails() {
    if (!prefs.dbInspectorOpen) showInspector();
  }

  function onKey(e: KeyboardEvent) {
    if (ui.modal) return;
    const meta = e.ctrlKey || e.metaKey;
    if (meta && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "w" && tab) {
      e.preventDefault();
      void databases.closeTab(tab.id);
    } else if (meta && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s" && tab && !tab.table && !e.defaultPrevented) {
      // Ctrl+S in the editor saves the query itself; this covers focus anywhere else in the workspace.
      e.preventDefault();
      void databases.saveQuery(tab.id);
    } else if (e.altKey && !meta && (e.key === "PageDown" || e.key === "PageUp")) {
      // Alt, not Ctrl: Ctrl+PageUp/PageDown switch the app's own tabs.
      e.preventDefault();
      databases.cycle(e.key === "PageDown" ? 1 : -1);
      if (databases.activeTabId) dbWorkspace.selectDoc(databases.activeTabId);
    } else if (e.key === "F5" && tab && !(e.target as HTMLElement)?.closest("[role=tree]")) {
      // The window's own reload would throw the open vault away; this reloads the table instead.
      e.preventDefault();
      if (tab.table && databases.isPaged(tab)) void databases.refreshPage(tab.id);
    } else if (e.key === "Escape") {
      if (dbWorkspace.explorerDrawerOpen || dbWorkspace.inspectorDrawerOpen) {
        dbWorkspace.explorerDrawerOpen = dbWorkspace.inspectorDrawerOpen = false;
      }
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="relative flex min-h-0 min-w-0 flex-1 overflow-hidden bg-base" bind:clientWidth={width} data-testid="databases">
  {#if layout.explorerDocked}
    <aside class="min-h-0 shrink-0 border-r border-line" style="width: {layout.explorerWidth}px" aria-label="Database explorer">
      <ExplorerPanel oncollapse={hideExplorer} onnewquery={newQuery} />
    </aside>
    <PanelResizer side="left" value={layout.explorerWidth} min={EXPLORER_MIN} max={EXPLORER_MAX} label="Resize the explorer" onchange={(w) => (prefs.dbExplorerWidth = w)} onreset={() => (prefs.dbExplorerWidth = EXPLORER_DEFAULT)} />
  {/if}

  <section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Query workspace">
    <DocTabs
      explorerHidden={!layout.explorerDocked}
      inspectorHidden={!layout.inspectorDocked}
      onshowexplorer={showExplorer}
      onshowinspector={showInspector}
      onnew={newQueryAnywhere}
    />
    {#if tab}
      <ConnectionContext {tab} />
      <div class="min-h-0 flex-1">
        {#if tab.table && databases.isPaged(tab)}
          {#key tab.id}<TableDoc {tab} />{/key}
        {:else}
          <QueryDoc bind:this={queryDoc} {tab} {now} ongotosettings={() => (ui.modal = { kind: "db-connection", id: tab.connId })} />
        {/if}
      </div>
    {:else}
      <div class="min-h-0 flex-1"><Welcome onnewquery={(c) => newQuery(c)} onfocussearch={focusSearch} /></div>
    {/if}
  </section>

  {#if layout.inspectorDocked}
    <PanelResizer side="right" value={layout.inspectorWidth} min={INSPECTOR_MIN} max={INSPECTOR_MAX} label="Resize the inspector" onchange={(w) => (prefs.dbInspectorWidth = w)} onreset={() => (prefs.dbInspectorWidth = INSPECTOR_DEFAULT)} />
    <aside class="min-h-0 shrink-0 border-l border-line" style="width: {layout.inspectorWidth}px" aria-label="Context inspector">
      <Inspector {now} oncollapse={hideInspector} />
    </aside>
  {/if}

  <!-- Panels with no room beside the editor open over it -->
  {#if !layout.explorerDocked && dbWorkspace.explorerDrawerOpen}
    <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close the explorer" tabindex="-1" onclick={() => (dbWorkspace.explorerDrawerOpen = false)}></button>
    <aside class="anim-pop-left absolute inset-y-0 left-0 z-40 w-[min(22rem,92%)] border-r border-line shadow-2xl" aria-label="Database explorer">
      <ExplorerPanel oncollapse={hideExplorer} onnewquery={newQuery} />
    </aside>
  {/if}
  {#if !layout.inspectorDocked && dbWorkspace.inspectorDrawerOpen}
    <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close the inspector" tabindex="-1" onclick={() => (dbWorkspace.inspectorDrawerOpen = false)}></button>
    <aside class="anim-pop-left absolute inset-y-0 right-0 z-40 w-[min(22rem,92%)] border-l border-line shadow-2xl" aria-label="Context inspector">
      <Inspector {now} oncollapse={hideInspector} />
    </aside>
  {/if}
</div>
