<script lang="ts">
  // A tmux session drawn with the app's own widgets: its windows are the tab strip below, its panes are
  // terminals placed where tmux's layout puts them, and the buttons send tmux commands. Nothing of tmux's own
  // status line or key bindings is on screen.
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { Columns2, Plus, Rows2, Unplug, X } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { ACTIONS, comboFor } from "$lib/shortcuts";
  import { DEFAULT_PREFS, settings } from "$lib/stores/settings.svelte";
  import { allThemes, themeById } from "$lib/themes";
  import { effectiveLook } from "$lib/terminalprofile";
  import { isIdleShell, type TmuxControlClient } from "$lib/tmux/TmuxControlClient";
  import { layoutDividers, layoutPanes, type TmuxDivider } from "$lib/tmux/tmuxLayoutParser";
  import { multiplexers } from "$lib/tmux/useRemoteMultiplexer.svelte";
  import Kbd from "./Kbd.svelte";
  import TmuxPane from "./TmuxPane.svelte";

  let { paneId, client, active }: { paneId: string; client: TmuxControlClient; active: boolean } = $props();

  // The client isn't reactive; it says when something changed, and the template reads `rev` to follow.
  const mux = $derived(multiplexers.get(paneId));
  const windows = $derived((void mux.rev, client.windowList));
  const current = $derived(windows.find((w) => w.id === client.activeWindowId) ?? windows[0]);

  /** Every pane of every window, by number, so a hidden window keeps its terminals and what they show. */
  const allPanes = $derived.by(() => {
    void mux.rev;
    const out: { windowId: number; paneId: number; x: number; y: number; w: number; h: number }[] = [];
    for (const win of client.windowList) {
      if (win.layout) for (const p of layoutPanes(win.layout.root)) out.push({ windowId: win.id, ...p });
    }
    return out;
  });
  const dividers = $derived<TmuxDivider[]>(current?.layout ? layoutDividers(current.layout.root) : []);

  /** The pane tmux says is active in the shown window. */
  const focusedPane = $derived(current?.activePaneId ?? null);

  const route = new Map<number, (data: Uint8Array) => void>();
  let unsubscribe: (() => void) | undefined;

  // -- size: how many characters fit, which is what tmux is told ---------------------------------------------------
  let area: HTMLDivElement;
  let measure: HTMLDivElement;
  let probe: Terminal | undefined;
  let fit: FitAddon | undefined;
  let observer: ResizeObserver | undefined;
  let total = $state({ cols: 80, rows: 24 });
  let cell = $state({ w: 8, h: 16 });

  function options() {
    const p = settings.prefs;
    const known = new Set(allThemes(p.customThemes).map((t) => t.id));
    const look = effectiveLook(p, undefined, DEFAULT_PREFS.fontSize, (id) => known.has(id));
    return {
      theme: themeById(look.themeId, p.customThemes).theme,
      fontFamily: p.fontFamily,
      fontSize: look.fontSize,
      lineHeight: p.lineHeight,
      cursorStyle: p.cursorStyle,
      cursorBlink: p.cursorBlink,
      scrollback: look.scrollback,
      letterSpacing: p.letterSpacing,
      minimumContrastRatio: p.minimumContrastRatio,
      drawBoldTextInBrightColors: p.boldAsBright,
    };
  }
  const termOptions = $derived(options());

  function measureArea() {
    if (!probe || !fit || area.clientWidth === 0 || area.clientHeight === 0) return;
    const d = fit.proposeDimensions();
    if (!d || !d.cols || !d.rows) return;
    total = { cols: d.cols, rows: d.rows };
    // The probe fills the area, so its character count gives the size of one character.
    cell = { w: area.clientWidth / d.cols, h: area.clientHeight / d.rows };
    client.resizeClient(d.cols, d.rows);
  }

  onMount(() => {
    unsubscribe = client.on((e) => {
      if (e.type === "output") route.get(e.paneId)?.(e.data);
    });
    probe = new Terminal({ ...options(), scrollback: 0, cols: 80, rows: 24 });
    fit = new FitAddon();
    probe.loadAddon(fit);
    probe.open(measure);
    observer = new ResizeObserver(measureArea);
    observer.observe(area);
    measureArea();
  });

  onDestroy(() => {
    unsubscribe?.();
    observer?.disconnect();
    probe?.dispose();
  });

  // Font and size changes in Settings change how many characters fit.
  $effect(() => {
    const o = termOptions;
    if (!probe) return;
    probe.options.fontFamily = o.fontFamily;
    probe.options.fontSize = o.fontSize;
    probe.options.lineHeight = o.lineHeight;
    probe.options.letterSpacing = o.letterSpacing;
    queueMicrotask(measureArea);
  });

  // -- controls ----------------------------------------------------------------------------------------------------

  /** Programs still running in these panes, other than a shell waiting at its prompt. */
  async function busy(ids: number[]): Promise<string[]> {
    const names = await Promise.all(ids.map((id) => client.foregroundCommand(id).catch(() => "")));
    return [...new Set(names.filter((n) => n && !isIdleShell(n)))];
  }

  async function closePane(id: number) {
    const running = await busy([id]);
    if (running.length && !(await ask(`${running[0]} is still running in this pane. Close it anyway?`, { title: "Close pane", confirm: "Close pane", danger: true }))) return;
    client.killPane(id);
  }

  async function closeWindow(id: number) {
    const win = client.windows.get(id);
    const running = win?.layout ? await busy(layoutPanes(win.layout.root).map((p) => p.paneId)) : [];
    if (running.length && !(await ask(`${running.join(", ")} ${running.length === 1 ? "is" : "are"} still running in this window. Close it anyway?`, { title: "Close window", confirm: "Close window", danger: true }))) return;
    client.killWindow(id);
  }

  const split = (how: "side" | "below") => client.splitWindow(focusedPane, how);

  // -- dragging a divider: tmux owns the layout, so a drag asks it to resize the pane before the divider --------------
  let drag: { d: TmuxDivider; start: number; base: number } | null = null;

  function startDrag(e: PointerEvent, d: TmuxDivider) {
    const edge = current?.layout && layoutPanes(current.layout.root).find((p) => p.paneId === d.edgePaneId);
    if (!edge) return;
    drag = { d, start: d.dir === "row" ? e.clientX : e.clientY, base: d.dir === "row" ? edge.w : edge.h };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function moveDrag(e: PointerEvent) {
    if (!drag) return;
    const row = drag.d.dir === "row";
    const cells = Math.round(((row ? e.clientX : e.clientY) - drag.start) / (row ? cell.w : cell.h));
    client.resizePaneAxis(drag.d.edgePaneId, row ? "x" : "y", Math.max(1, drag.base + cells));
  }

  /** Keyboard: an arrow key moves the divider by one character. */
  function nudge(e: KeyboardEvent, d: TmuxDivider) {
    const key = d.dir === "row" ? { ArrowLeft: -1, ArrowRight: 1 }[e.key] : { ArrowUp: -1, ArrowDown: 1 }[e.key];
    const edge = current?.layout && layoutPanes(current.layout.root).find((p) => p.paneId === d.edgePaneId);
    if (!key || !edge) return;
    e.preventDefault();
    client.resizePaneAxis(d.edgePaneId, d.dir === "row" ? "x" : "y", Math.max(1, (d.dir === "row" ? edge.w : edge.h) + key));
  }

  const detachCombo = $derived.by(() => {
    const a = ACTIONS.find((x) => x.id === "tmux-detach");
    return a ? comboFor(a) : "";
  });
</script>

<div class="flex h-full w-full flex-col bg-base" role="region" aria-label="tmux session {client.sessionName}">
  <div class="flex h-8 shrink-0 items-center gap-1 border-b border-line bg-panel/60 px-2 text-xs">
    <span class="mr-1 shrink-0 font-mono tabular-nums text-fg-muted" title="tmux session">{client.sessionName}</span>
    <div class="flex min-w-0 items-center gap-0.5 overflow-x-auto" role="tablist" aria-label="tmux windows">
      {#each windows as w (w.id)}
        <div class="group flex shrink-0 items-center rounded-sm border {w.id === current?.id ? 'border-line bg-base text-fg' : 'border-transparent text-fg-muted hover:bg-panel-hover hover:text-fg'}">
          <button class="px-2 py-0.5 font-mono tabular-nums" role="tab" aria-selected={w.id === current?.id} onclick={() => client.selectWindow(w.id)}>
            {w.index}:{w.name}{w.id === current?.id ? "*" : ""}
          </button>
          <button class="mr-0.5 flex h-4 w-4 items-center justify-center rounded-sm opacity-0 hover:text-danger focus:opacity-100 group-hover:opacity-100" title="Close window" aria-label="Close window {w.index}" onclick={() => closeWindow(w.id)}>
            <X size={11} />
          </button>
        </div>
      {/each}
      <button class="icon-btn h-6 w-6" title="New window (new-window)" aria-label="New window" onclick={() => client.newWindow()}><Plus size={13} /></button>
    </div>
    <div class="flex-1"></div>
    <span class="mr-1 shrink-0 font-mono tabular-nums text-fg-muted" title="Size of the session in characters">{total.cols}×{total.rows}</span>
    <button class="icon-btn h-6 w-6" title="Split side by side (split-window -h)" aria-label="Split side by side" onclick={() => split("side")}><Columns2 size={13} /></button>
    <button class="icon-btn h-6 w-6" title="Split top and bottom (split-window -v)" aria-label="Split top and bottom" onclick={() => split("below")}><Rows2 size={13} /></button>
    <button
      class="inline-flex h-6 items-center gap-1.5 rounded-sm border border-line px-2 text-fg-muted hover:bg-panel-hover hover:text-fg"
      title="Leave the session. Everything keeps running on the host."
      onclick={() => multiplexers.detach(paneId)}
    >
      <Unplug size={12} aria-hidden="true" /> Detach {#if detachCombo}<Kbd keys={detachCombo} />{/if}
    </button>
  </div>

  <div bind:this={area} class="relative min-h-0 flex-1 overflow-hidden">
    <!-- Measures how many characters fit; never seen. -->
    <div bind:this={measure} class="pointer-events-none invisible absolute inset-0" aria-hidden="true"></div>

    {#each allPanes as p (p.paneId)}
      {@const shown = p.windowId === current?.id}
      <div
        class="absolute overflow-hidden {shown ? '' : 'hidden'} {p.paneId === focusedPane && shown && allPanes.filter((x) => x.windowId === p.windowId).length > 1 ? 'ring-1 ring-inset ring-accent/40' : ''}"
        style:left="{p.x * cell.w}px"
        style:top="{p.y * cell.h}px"
        style:width="{p.w * cell.w}px"
        style:height="{p.h * cell.h}px"
        role="presentation"
      >
        <TmuxPane
          {client}
          paneId={p.paneId}
          cols={p.w}
          rows={p.h}
          options={termOptions}
          focused={p.paneId === focusedPane && active}
          visible={shown && active}
          {route}
          onfocus={() => client.selectPane(p.paneId)}
          onclose={() => closePane(p.paneId)}
        />
      </div>
    {/each}

    {#each dividers as d, i (i)}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        class="group absolute z-10 flex items-center justify-center {d.dir === 'row' ? 'cursor-col-resize' : 'cursor-row-resize'}"
        style:left={d.dir === "row" ? `${d.at.x * cell.w + cell.w / 2 - 3}px` : `${d.at.x * cell.w}px`}
        style:top={d.dir === "row" ? `${d.at.y * cell.h}px` : `${d.at.y * cell.h + cell.h / 2 - 3}px`}
        style:width={d.dir === "row" ? "6px" : `${d.at.w * cell.w}px`}
        style:height={d.dir === "row" ? `${d.at.h * cell.h}px` : "6px"}
        role="separator"
        aria-orientation={d.dir === "row" ? "vertical" : "horizontal"}
        aria-label="Resize split"
        tabindex="0"
        title="Drag to resize"
        onpointerdown={(e) => startDrag(e, d)}
        onpointermove={moveDrag}
        onpointerup={() => (drag = null)}
        onpointercancel={() => (drag = null)}
        onkeydown={(e) => nudge(e, d)}
      >
        <div class="bg-line transition-colors group-hover:bg-accent {d.dir === 'row' ? 'h-full w-px' : 'h-px w-full'}"></div>
      </div>
    {/each}

    {#if windows.length === 0}
      <div class="flex h-full items-center justify-center text-xs text-fg-muted">Waiting for tmux…</div>
    {/if}
  </div>
</div>
