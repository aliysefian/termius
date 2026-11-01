<script lang="ts">
  // The Tunnels page: the list of tunnels with their state, and the selected tunnel's details beside it. No terminal
  // lives here; "Open SSH Terminal" opens one in the terminal area and leaves the tunnels as they are.
  import { onMount } from "svelte";
  import { ChevronDown, FileDown, FileUp, Play, Plus, Search, Square, Trash2, X } from "lucide-svelte";
  import { open as pickFile, save as pickSave } from "@tauri-apps/plugin-dialog";
  import Illustration from "../Illustration.svelte";
  import TunnelMetrics from "./TunnelMetrics.svelte";
  import TunnelTable from "./TunnelTable.svelte";
  import TunnelDetails from "./TunnelDetails.svelte";
  import * as api from "$lib/api";
  import { DETAIL_DEFAULT, DETAIL_MAX, DETAIL_MIN, detailWidth } from "$lib/tunnelslayout";
  import { settings } from "$lib/stores/settings.svelte";
  import { tunnels } from "$lib/stores/tunnels.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { counts as countRows, matches, type StatusFilter, type TypeFilter } from "$lib/tunnels";
  import { errorMessage } from "$lib/types";

  let width = $state(1200);
  /** The details panel sits beside the list when there is room, and slides over it when there is not. */
  const docked = $derived(width >= 980);
  const compact = $derived(width < 1240);
  const panelWidth = $derived(detailWidth(settings.prefs.tunnelsDetailWidth, width));

  const all = $derived(tunnels.rows);
  const counts = $derived(countRows(all));
  const shown = $derived(all.filter((r) => matches(r, tunnels.query, tunnels.typeFilter, tunnels.statusFilter)));
  const selected = $derived(tunnels.selected);
  const checked = $derived(tunnels.checkedRows);

  // The slide-over is only for a window too narrow to dock the panel: it never stays open behind a wider layout.
  $effect(() => {
    tunnels.docked = docked;
    if (docked) tunnels.slideOver = false;
  });

  // Keep the selection meaningful: a tunnel that is deleted (here or on another device) stops being selected.
  $effect(() => {
    if (tunnels.selectedId && !all.some((r) => r.id === tunnels.selectedId)) tunnels.selectedId = null;
  });

  // The clock for uptimes ticks only while something is active.
  let now = $state(Date.now());
  $effect(() => {
    if (counts.active === 0) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  onMount(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void tunnels.subscribe().then((f) => (gone ? f() : (off = f)));
    if (tunnels.selectedId) void tunnels.loadLogs(tunnels.selectedId);
    return () => {
      gone = true;
      off?.();
    };
  });

  function pickStatus(s: "all" | "active" | "stopped" | "failed") {
    tunnels.clearFilters();
    tunnels.statusFilter = s as StatusFilter;
  }

  // -- import and export -------------------------------------------------------------------------------------------
  let fileMenu = $state(false);

  async function exportFile() {
    fileMenu = false;
    try {
      const path = await pickSave({ title: "Export tunnels", defaultPath: "sshvault-tunnels.json", filters: [{ name: "Tunnels", extensions: ["json"] }] });
      if (!path) return;
      const n = await api.forwards.exportFile(path);
      ui.notify("info", `Exported ${n} tunnel${n === 1 ? "" : "s"}. The file holds hosts' names and the forwarding, no credentials.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function importFile() {
    fileMenu = false;
    try {
      const path = await pickFile({ title: "Import tunnels", multiple: false, filters: [{ name: "Tunnels", extensions: ["json"] }] });
      if (!path || Array.isArray(path)) return;
      const r = await api.forwards.importFile(path);
      const skipped = r.skipped.length ? ` Skipped ${r.skipped.length}: ${r.skipped.slice(0, 3).join("; ")}${r.skipped.length > 3 ? "…" : ""}` : "";
      ui.notify(r.skipped.length ? "warning" : "info", `Imported ${r.added} tunnel${r.added === 1 ? "" : "s"}.${skipped}`);
      await vaultStore.reloadAll();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- resizing the details panel ------------------------------------------------------------------------------
  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startWidth = panelWidth;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (dragging) settings.prefs.tunnelsDetailWidth = detailWidth(startWidth - (e.clientX - startX), width);
  }
  function stop() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      settings.prefs.tunnelsDetailWidth = detailWidth(panelWidth + (e.key === "ArrowLeft" ? step : -step), width);
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.tunnelsDetailWidth = DETAIL_DEFAULT;
    }
  }

  function pageKey(e: KeyboardEvent) {
    if (e.key === "Escape" && fileMenu) fileMenu = false;
    else if (e.key === "Escape" && tunnels.slideOver && !docked && !(e.target as HTMLElement)?.closest("[role=dialog], [role=menu]")) tunnels.slideOver = false;
  }
</script>

<svelte:window onkeydown={pageKey} />

<div class="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" bind:clientWidth={width}>
  <div class="flex min-h-0 flex-1 flex-col gap-5 overflow-hidden p-6 max-md:p-4">
    <!-- Header -->
    <header class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0">
        <h1 class="text-xl font-semibold tracking-tight">Tunnels</h1>
        <p class="mt-0.5 text-sm text-fg-muted">Manage SSH port forwarding and secure connections.</p>
      </div>
      <div class="flex items-center gap-2">
        <div class="relative">
          <button class="btn-secondary" aria-haspopup="menu" aria-expanded={fileMenu} onclick={() => (fileMenu = !fileMenu)}>Import / Export <ChevronDown size={14} /></button>
          {#if fileMenu}
            <button class="fixed inset-0 z-30 cursor-default" aria-label="Close menu" tabindex="-1" onclick={() => (fileMenu = false)}></button>
            <div class="anim-pop absolute right-0 top-full z-40 mt-1 w-60 rounded-xl border border-line bg-panel p-1 shadow-xl" role="menu">
              <button class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm hover:bg-panel-hover" role="menuitem" onclick={() => void importFile()}><FileDown size={15} /> Import tunnels…</button>
              <button class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm hover:bg-panel-hover disabled:opacity-40" role="menuitem" disabled={all.length === 0} onclick={() => void exportFile()}><FileUp size={15} /> Export tunnels…</button>
              <p class="px-2.5 pb-1.5 pt-1 text-[11px] text-fg-muted">A tunnel file names the host of each tunnel and holds no credentials.</p>
            </div>
          {/if}
        </div>
        <button class="btn-primary" onclick={() => (ui.modal = { kind: "forward", id: null })}><Plus size={15} /> Create Tunnel</button>
      </div>
    </header>

    {#if all.length === 0}
      <div class="anim-rise m-auto max-w-sm text-center">
        <Illustration scene="tunnels" size={140} />
        <h2 class="mt-2 text-[16px] font-semibold leading-6">No tunnels yet</h2>
        <p class="mt-1 text-sm text-fg-muted">Forward a port to reach a service behind a server, expose one of yours through it, or open a SOCKS proxy.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "forward", id: null })}><Plus size={15} /> Create Tunnel</button>
      </div>
    {:else}
      <TunnelMetrics {counts} onpick={pickStatus} compact={!docked} />

      <div class="flex min-h-0 flex-1 gap-4">
        <!-- The list -->
        <section class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-panel" aria-label="Tunnel list">
          <div class="flex flex-wrap items-center gap-2 border-b border-line p-3">
            <div class="relative min-w-[10rem] flex-1">
              <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" aria-hidden="true" />
              <input class="input h-9 py-0 pl-8 text-sm" placeholder="Search tunnels, hosts, ports…" aria-label="Search tunnels" bind:value={tunnels.query} />
            </div>
            <select class="input h-9 w-auto py-0 text-sm" aria-label="Filter by type" bind:value={tunnels.typeFilter}>
              <option value="all">All Types</option>
              <option value="local">Local</option>
              <option value="remote">Remote</option>
              <option value="dynamic">Dynamic / SOCKS</option>
            </select>
            <select class="input h-9 w-auto py-0 text-sm" aria-label="Filter by status" bind:value={tunnels.statusFilter}>
              <option value="all">All Statuses</option>
              <option value="active">Active</option>
              <option value="stopped">Stopped</option>
              <option value="connecting">Connecting</option>
              <option value="failed">Failed</option>
            </select>
          </div>

          {#if checked.length}
            <div class="flex flex-wrap items-center gap-2 border-b border-accent/30 bg-accent/10 px-4 py-2 text-sm" role="toolbar" aria-label="Selected tunnels">
              <span class="font-medium">{checked.length} tunnel{checked.length === 1 ? "" : "s"} selected</span>
              <div class="flex-1"></div>
              <button class="btn-secondary h-8" onclick={() => void tunnels.startChecked()}><Play size={13} /> Start</button>
              <button class="btn-secondary h-8" onclick={() => void tunnels.stopChecked()}><Square size={13} /> Stop</button>
              <button class="btn-secondary h-8 text-danger" onclick={() => void tunnels.deleteChecked()}><Trash2 size={13} /> Delete</button>
              <button class="icon-btn h-8 w-8" title="Clear selection" aria-label="Clear selection" onclick={() => tunnels.setAllChecked([], false)}><X size={15} /></button>
            </div>
          {/if}

          {#if shown.length === 0}
            <div class="m-auto p-8 text-center">
              <p class="text-sm font-medium">No matching tunnels</p>
              <p class="mt-1 text-xs text-fg-muted">Try changing your search or filters.</p>
              <button class="btn-secondary mt-3" onclick={() => tunnels.clearFilters()}>Clear filters</button>
            </div>
          {:else}
            <TunnelTable rows={shown} {compact} {now} />
            <div class="border-t border-line px-4 py-2 text-xs text-fg-muted">
              {tunnels.filtering ? `${shown.length} of ${all.length} tunnels` : `${all.length} tunnel${all.length === 1 ? "" : "s"}`}
            </div>
          {/if}
        </section>

        <!-- Details, beside the list -->
        {#if docked}
          <aside class="relative flex min-h-0 shrink-0 overflow-hidden rounded-xl border border-line bg-panel" style:width="{panelWidth}px" aria-label="Tunnel details">
            {#if selected}
              {#key selected.id}<div class="min-h-0 min-w-0 flex-1"><TunnelDetails row={selected} {now} /></div>{/key}
            {:else}
              <div class="m-auto max-w-[16rem] p-6 text-center text-sm text-fg-muted">Select a tunnel to see its status, settings and log.</div>
            {/if}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="absolute left-0 top-0 z-20 h-full w-2 cursor-col-resize outline-none after:absolute after:left-0 after:top-0 after:h-full after:w-0.5 after:transition-colors {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize the details panel"
              aria-valuenow={panelWidth}
              aria-valuemin={DETAIL_MIN}
              aria-valuemax={DETAIL_MAX}
              tabindex="0"
              title="Drag to resize · double-click to reset"
              onpointerdown={down}
              onpointermove={move}
              onpointerup={stop}
              onpointercancel={stop}
              onlostpointercapture={() => dragging && stop()}
              ondblclick={() => (settings.prefs.tunnelsDetailWidth = DETAIL_DEFAULT)}
              onkeydown={handleKey}
            ></div>
          </aside>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Details, over the list, when the window is too narrow to put them beside it -->
  {#if !docked && selected && tunnels.slideOver}
    <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close details" tabindex="-1" onclick={() => (tunnels.slideOver = false)}></button>
    <aside class="anim-pop-left absolute inset-y-0 right-0 z-40 flex w-[min(28rem,100%)] flex-col border-l border-line bg-panel shadow-2xl" aria-label="Tunnel details">
      {#key selected.id}<TunnelDetails row={selected} {now} onclose={() => (tunnels.slideOver = false)} />{/key}
    </aside>
  {/if}
</div>
