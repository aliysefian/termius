<script lang="ts">
  import { onMount } from "svelte";
  import { ChevronUp, ListChecks, PanelLeftOpen, PanelRightOpen, Plus } from "lucide-svelte";
  import PanelResizer from "./databases/PanelResizer.svelte";
  import EditorPanel from "./runbooks/EditorPanel.svelte";
  import ExecutionPanel from "./runbooks/ExecutionPanel.svelte";
  import HistoryView from "./runbooks/HistoryView.svelte";
  import LibraryPanel from "./runbooks/LibraryPanel.svelte";
  import PreviewDialog from "./runbooks/PreviewDialog.svelte";
  import RunConsole from "./runbooks/RunConsole.svelte";
  import SchedulesView from "./runbooks/SchedulesView.svelte";
  import TemplatesView from "./runbooks/TemplatesView.svelte";
  import { EXEC, LIBRARY, layoutFor, OUTPUT, outputHeight } from "$lib/runbooklayout";
  import { runbooks as rb, type WorkspaceTab } from "$lib/stores/runbooks.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  const TABS: { id: WorkspaceTab; label: string }[] = [
    { id: "runbooks", label: "Runbooks" },
    { id: "history", label: "History" },
    { id: "schedules", label: "Schedules" },
    { id: "templates", label: "Templates" },
  ];

  const prefs = $derived(settings.prefs);
  let width = $state(1400);
  let height = $state(800);
  let now = $state(Date.now());
  let dragging = $state(false);

  const layout = $derived(layoutFor({ width, libraryOpen: prefs.rbLibraryOpen, execOpen: prefs.rbExecOpen, libraryWidth: prefs.rbLibraryWidth, execWidth: prefs.rbExecWidth }));
  const consoleHeight = $derived(outputHeight(prefs.rbOutputHeight, height));
  const showConsole = $derived(!!rb.live);
  const going = $derived(rb.phase === "running" || rb.phase === "starting" || rb.phase === "cancelling");

  // The clock on running hosts moves once a second, only while something is running.
  $effect(() => {
    if (!going) return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  onMount(() => {
    void rb.loadHistory();
    if (!rb.selectedId && rb.items.length && !rb.text) void rb.select(rb.items[0].id);
  });

  // A panel with no room beside the editor opens over it, on request.
  function showLibrary() {
    prefs.rbLibraryOpen = true;
    if (!layoutFor({ width, libraryOpen: true, execOpen: prefs.rbExecOpen, libraryWidth: prefs.rbLibraryWidth, execWidth: prefs.rbExecWidth }).libraryDocked) rb.libraryDrawer = true;
  }
  function hideLibrary() {
    prefs.rbLibraryOpen = false;
    rb.libraryDrawer = false;
  }
  function showExec() {
    prefs.rbExecOpen = true;
    if (!layoutFor({ width, libraryOpen: prefs.rbLibraryOpen, execOpen: true, libraryWidth: prefs.rbLibraryWidth, execWidth: prefs.rbExecWidth }).execDocked) rb.execDrawer = true;
  }
  function hideExec() {
    prefs.rbExecOpen = false;
    rb.execDrawer = false;
  }

  function key(e: KeyboardEvent) {
    if (ui.modal || rb.previewOpen) return;
    const meta = e.ctrlKey || e.metaKey;
    if (meta && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s" && rb.tab === "runbooks" && rb.text && !e.defaultPrevented) {
      e.preventDefault();
      void rb.save();
    } else if (meta && !e.shiftKey && !e.altKey && e.key === "Enter" && rb.tab === "runbooks" && rb.text && !e.defaultPrevented) {
      // Validates, never runs: a run is started on purpose, from the button and its confirmation.
      e.preventDefault();
      void rb.validateNow();
    } else if (meta && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "n" && !e.defaultPrevented) {
      e.preventDefault();
      void rb.newBlank();
    } else if (e.key === "Escape" && (rb.libraryDrawer || rb.execDrawer)) {
      rb.libraryDrawer = rb.execDrawer = false;
    }
  }

  // The divider above the console.
  let startY = 0;
  let startH = 0;
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startY = e.clientY;
    startH = consoleHeight;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (dragging) prefs.rbOutputHeight = outputHeight(startH - (e.clientY - startY), height);
  }
  function stop() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }
  function dividerKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 80 : 24;
    if (e.key === "ArrowUp") prefs.rbOutputHeight = outputHeight(consoleHeight + step, height);
    else if (e.key === "ArrowDown") prefs.rbOutputHeight = outputHeight(consoleHeight - step, height);
    else if (e.key === "Home" || e.key === "Enter") prefs.rbOutputHeight = OUTPUT.def;
    else return;
    e.preventDefault();
  }

  const tabKey = (e: KeyboardEvent) => {
    const i = TABS.findIndex((t) => t.id === rb.tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    rb.tab = next.id;
    queueMicrotask(() => document.getElementById(`rb-tab-${next.id}`)?.focus());
  };
</script>

<svelte:window onkeydown={key} />

<div class="relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-base" data-testid="runbooks" bind:clientWidth={width} bind:clientHeight={height}>
  <header class="flex shrink-0 flex-wrap items-center gap-3 border-b border-line bg-panel px-4 py-2">
    <h1 class="flex items-center gap-2 text-[15px] font-semibold"><ListChecks size={18} class="text-accent" /> Runbooks</h1>
    <div class="flex gap-1" role="tablist" aria-label="Runbooks sections">
      {#each TABS as t (t.id)}
        <button
          id="rb-tab-{t.id}"
          role="tab"
          aria-selected={rb.tab === t.id}
          tabindex={rb.tab === t.id ? 0 : -1}
          class="rounded-md px-3 py-1 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent {rb.tab === t.id ? 'bg-accent/15 font-medium text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          onclick={() => (rb.tab = t.id)}
          onkeydown={tabKey}
        >{t.label}</button>
      {/each}
    </div>
    <div class="flex-1"></div>
    {#if rb.tab === "runbooks"}
      {#if !layout.libraryDocked}<button class="btn-ghost h-8 px-2 text-xs" onclick={showLibrary} title="Show the library"><PanelLeftOpen size={14} /> Library</button>{/if}
      {#if !layout.execDocked}<button class="btn-ghost h-8 px-2 text-xs" onclick={showExec} title="Show the run panel"><PanelRightOpen size={14} /> Run</button>{/if}
    {/if}
    <button class="btn-primary h-8 text-xs" onclick={() => rb.newBlank()} title="New runbook (Ctrl+N)" data-testid="header-new"><Plus size={14} /> New Runbook</button>
  </header>

  {#if rb.tab === "runbooks"}
    <div class="flex min-h-0 flex-1 flex-col">
      <div class="relative flex min-h-0 flex-1">
        {#if layout.libraryDocked}
          <aside class="min-h-0 shrink-0 border-r border-line" style="width: {layout.libraryWidth}px" aria-label="Runbook library">
            <LibraryPanel oncollapse={hideLibrary} ontemplates={() => (rb.tab = "templates")} />
          </aside>
          <PanelResizer side="left" value={layout.libraryWidth} min={LIBRARY.min} max={LIBRARY.max} label="Resize the library" onchange={(w) => (prefs.rbLibraryWidth = w)} onreset={() => (prefs.rbLibraryWidth = LIBRARY.def)} />
        {/if}

        <div class="min-h-0 min-w-0 flex-1"><EditorPanel onbrowse={() => (rb.tab = "templates")} /></div>

        {#if layout.execDocked}
          <PanelResizer side="right" value={layout.execWidth} min={EXEC.min} max={EXEC.max} label="Resize the run panel" onchange={(w) => (prefs.rbExecWidth = w)} onreset={() => (prefs.rbExecWidth = EXEC.def)} />
          <aside class="min-h-0 shrink-0 border-l border-line" style="width: {layout.execWidth}px" aria-label="Run panel">
            <ExecutionPanel oncollapse={hideExec} onstep={(i) => ((rb.mode = "builder"), (rb.editing = { index: i, rollback: false }))} />
          </aside>
        {/if}

        {#if !layout.libraryDocked && rb.libraryDrawer}
          <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close the library" tabindex="-1" onclick={() => (rb.libraryDrawer = false)}></button>
          <aside class="anim-pop-left absolute inset-y-0 left-0 z-40 w-[min(22rem,92%)] border-r border-line shadow-2xl" aria-label="Runbook library">
            <LibraryPanel oncollapse={hideLibrary} ontemplates={() => ((rb.tab = "templates"), (rb.libraryDrawer = false))} />
          </aside>
        {/if}
        {#if !layout.execDocked && rb.execDrawer}
          <button class="absolute inset-0 z-30 cursor-default bg-black/40" aria-label="Close the run panel" tabindex="-1" onclick={() => (rb.execDrawer = false)}></button>
          <aside class="anim-pop-left absolute inset-y-0 right-0 z-40 w-[min(24rem,92%)] border-l border-line shadow-2xl" aria-label="Run panel">
            <ExecutionPanel oncollapse={hideExec} onstep={(i) => ((rb.mode = "builder"), (rb.editing = { index: i, rollback: false }), (rb.execDrawer = false))} />
          </aside>
        {/if}
      </div>

      {#if showConsole && prefs.rbOutputOpen && rb.live}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="h-1.5 shrink-0 cursor-row-resize border-y border-line bg-panel hover:bg-accent/40 focus-visible:bg-accent/60 focus-visible:outline-none {dragging ? 'bg-accent/60' : ''}"
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize the output console"
          aria-valuenow={consoleHeight}
          tabindex="0"
          title="Drag to resize · double-click to reset"
          onpointerdown={down}
          onpointermove={move}
          onpointerup={stop}
          onpointercancel={stop}
          ondblclick={() => (prefs.rbOutputHeight = OUTPUT.def)}
          onkeydown={dividerKey}
        ></div>
        <div class="shrink-0" style="height: {consoleHeight}px"><RunConsole live={rb.live} title={rb.name || "Runbook"} phase={rb.phase} {now} /></div>
      {:else}
        <div class="flex shrink-0 items-center gap-2 border-t border-line bg-panel px-4 py-1.5 text-xs text-fg-muted" data-testid="console-bar">
          <span>Execution output</span>
          <span>·</span>
          <span>{rb.live ? (going ? "A run is going" : "The last run has ended") : "Appears here when you run a runbook."}</span>
          {#if rb.live}<button class="ml-auto inline-flex items-center gap-1 text-accent hover:underline" onclick={() => (prefs.rbOutputOpen = true)}><ChevronUp size={12} /> Show</button>{/if}
        </div>
      {/if}
    </div>
  {:else if rb.tab === "history"}
    <div class="min-h-0 flex-1"><HistoryView {now} /></div>
  {:else if rb.tab === "schedules"}
    <div class="min-h-0 flex-1"><SchedulesView /></div>
  {:else}
    <div class="min-h-0 flex-1"><TemplatesView /></div>
  {/if}

  {#if rb.previewOpen}<PreviewDialog onclose={() => (rb.previewOpen = false)} />{/if}
</div>
