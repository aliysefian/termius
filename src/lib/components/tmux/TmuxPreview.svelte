<script lang="ts">
  import { AlertTriangle, Camera, Columns2, Copy, LayoutGrid, List, RefreshCw, Rows2, SquareTerminal, type Server } from "lucide-svelte";
  import { onMount } from "svelte";
  import { paneKey, tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { activePane, ago, locate, noticeFor, type Pane, type Session, type Window } from "$lib/tmux/monitor";
  import TmuxMenu, { type MenuItem } from "./TmuxMenu.svelte";
  import TmuxScreen from "./TmuxScreen.svelte";
  import { actionsFor, type ActionId } from "$lib/tmux/actions";

  const sel = $derived(mon.selectedRow);
  const snap = $derived(sel ? mon.snapshotOf(sel.server) : undefined);
  const here = $derived(sel ? locate(snap, sel) : {});

  /** The panes this selection shows. */
  const panes = $derived.by<{ window: Window; pane: Pane; session: Session }[]>(() => {
    const out: { window: Window; pane: Pane; session: Session }[] = [];
    const add = (session: Session, w: Window) => w.panes.forEach((pane) => out.push({ session, window: w, pane }));
    if (here.pane && here.window && here.session) return [{ session: here.session, window: here.window, pane: here.pane }];
    if (here.window && here.session) add(here.session, here.window);
    else if (here.session) here.session.windows.forEach((w) => add(here.session!, w));
    return out;
  });

  /** The pane at the centre of a list-mode preview: the one selected, else the active one. */
  const focus = $derived.by(() => {
    if (here.pane && here.window && here.session) return { session: here.session, window: here.window, pane: here.pane };
    if (here.window && here.session) {
      const p = here.window.panes.find((x) => x.active) ?? here.window.panes[0];
      return p ? { session: here.session, window: here.window, pane: p } : null;
    }
    if (here.session) {
      const a = activePane(here.session);
      return a ? { session: here.session, ...a } : null;
    }
    return null;
  });

  let chosen = $state<string | null>(null);
  const focused = $derived(panes.find((p) => p.pane.id === chosen && sel?.paneId === undefined) ?? focus);
  const grid = $derived(mon.prefs.mode === "grid" && panes.length > 1);
  const tiles = $derived(panes.slice(0, mon.prefs.preview.maxThumbnails));
  const key = (id: string) => paneKey(sel!.server, id);

  // Tell the store what is on screen: only that is captured.
  $effect(() => {
    if (!sel) return mon.setVisiblePanes([]);
    mon.focusKey = focused ? key(focused.pane.id) : null;
    mon.setVisiblePanes(grid ? tiles.map((t) => key(t.pane.id)) : focused ? [key(focused.pane.id)] : []);
    return () => {
      mon.focusKey = null;
      mon.setVisiblePanes([]);
    };
  });
  // Capture at once when something new comes on screen, instead of waiting for the next tick.
  $effect(() => {
    for (const k of mon.visiblePanes) if (!mon.previews[k]) void mon.capture(k);
  });

  let now = $state(Date.now());
  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const SCROLLBACKS = [0, 200, 1000, 5000];
  const fp = $derived(focused ? mon.previews[key(focused.pane.id)] : undefined);

  let menu = $state<{ paneId: string; x: number; y: number } | null>(null);
  const ICONS: Partial<Record<ActionId, typeof Server>> = { "attach-tab": SquareTerminal, "split-right": Columns2, "split-down": Rows2, "copy-pane": Copy };
  function menuItems(paneId: string): MenuItem[] {
    if (!sel) return [];
    const row = { kind: "pane" as const, sessionId: focused?.session.id ?? sel.sessionId, windowId: panes.find((p) => p.pane.id === paneId)?.window.id, paneId };
    return actionsFor(row, snap, { canSplit: mon.canSplit, canReplace: false }).map((a) => ({ id: a.id, label: a.label, danger: a.danger, disabled: a.disabled, why: a.why, group: a.group, icon: ICONS[a.id] }));
  }
  function actOn(id: string, paneId: string) {
    if (!sel) return;
    const p = panes.find((x) => x.pane.id === paneId);
    void mon.act(id as ActionId, { kind: "pane", server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: p?.session.id, windowId: p?.window.id, paneId });
  }
  const attach = (mode: "tab" | "right" | "down", sessionId: string) => sel && mon.attach(sel.server, sessionId, mode);
</script>

<div class="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-base" aria-label="Pane preview">
  {#if !sel}
    <div class="grid flex-1 place-items-center p-8 text-center text-sm text-fg-muted">
      <div>
        <Camera size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p>Select a session, window or pane to preview it.</p>
        <p class="mt-1 text-xs">Nothing is attached or opened until you choose to.</p>
      </div>
    </div>
  {:else if sel.kind === "host"}
    {@const n = noticeFor(snap)}
    <div class="flex-1 overflow-y-auto p-6 text-sm">
      {#if n}
        <div class="mx-auto max-w-md rounded-lg border border-line bg-panel/60 p-4">
          <div class="flex items-center gap-2 font-medium"><AlertTriangle size={15} class="text-warning" /> {n.title}</div>
          <p class="mt-1 text-xs text-fg-muted">{n.detail}</p>
          {#if n.hint}<code class="mt-2 block select-text rounded bg-base px-2 py-1.5 font-mono text-xs">{n.hint}</code>{/if}
          {#if n.canCreate}<button class="mt-3 rounded-md border border-accent/50 bg-accent/10 px-3 py-1.5 text-xs text-accent hover:bg-accent/20" onclick={() => mon.act("new-session", sel)}>Create Session</button>{/if}
        </div>
      {:else}
        <p class="text-fg-muted">{snap?.sessions.length ?? 0} sessions on this host. Choose one to preview its panes.</p>
      {/if}
    </div>
  {:else if !focused}
    <div class="grid flex-1 place-items-center p-8 text-center text-sm text-fg-muted" role="status">
      <div>
        <AlertTriangle size={24} class="mx-auto mb-2 text-warning" />
        <p>This {sel.kind} no longer exists on the remote host.</p>
        <button class="mt-3 rounded-md border border-line px-3 py-1.5 text-xs hover:bg-panel-hover" onclick={() => mon.refresh(sel.hostId, sel.socket)}>Refresh</button>
      </div>
    </div>
  {:else}
    <header class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-line bg-panel/50 px-3 py-1.5 text-xs">
      <div class="min-w-0">
        <div class="truncate font-medium text-fg">
          {focused.session.name} <span class="text-fg-muted">›</span> {focused.window.index}: {focused.window.name}
          {#if !grid}<span class="text-fg-muted">›</span> <span class="font-mono">{focused.pane.id}</span>{/if}
        </div>
        <div class="truncate text-fg-muted">
          {#if grid}{panes.length} panes · snapshots{:else}{focused.pane.command || "shell"} · {fp?.capture?.info.width ?? focused.pane.width}×{fp?.capture?.info.height ?? focused.pane.height}{/if}
          {#if fp && !grid}· {fp.loading ? "refreshing…" : fp.capture ? `captured ${ago((now - fp.capture.taken_at_ms) / 1000)}` : ""}{/if}
        </div>
      </div>
      <span class="rounded border border-line px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-fg-muted" title="This is a snapshot captured from tmux. It does not accept keyboard input; attach to type in the session.">Snapshot · read-only</span>
      <div class="ml-auto flex items-center gap-1">
        {#if panes.length > 1}
          <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label="Preview layout">
            <button class="px-1.5 py-1 {!grid ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" title="One pane at a time" aria-pressed={!grid} onclick={() => (mon.prefs.mode = "list")}><List size={13} /></button>
            <button class="px-1.5 py-1 {grid ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" title="All panes" aria-pressed={grid} onclick={() => (mon.prefs.mode = "grid")}><LayoutGrid size={13} /></button>
          </div>
        {/if}
        <label class="flex items-center gap-1 text-fg-muted" title="Lines of history shown above the screen"
          >History
          <select class="rounded border border-line bg-base px-1 py-0.5 text-xs text-fg" value={mon.prefs.scrollback} onchange={(e) => { mon.prefs.scrollback = Number(e.currentTarget.value); for (const k of mon.visiblePanes) void mon.capture(k); }}>
            {#each SCROLLBACKS as n (n)}<option value={n}>{n === 0 ? "Screen only" : `${n} lines`}</option>{/each}
          </select>
        </label>
        <label class="flex items-center gap-1 text-fg-muted"><input type="checkbox" checked={mon.prefs.ansi} onchange={(e) => { mon.prefs.ansi = e.currentTarget.checked; for (const k of mon.visiblePanes) void mon.capture(k); }} /> Colours</label>
        <button class="grid h-6 w-6 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" title="Refresh the preview" aria-label="Refresh the preview" onclick={() => { for (const k of mon.visiblePanes) void mon.capture(k); }}><RefreshCw size={12} class={fp?.loading ? "animate-spin" : ""} /></button>
        <button class="grid h-6 w-6 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" title="Copy the visible text" aria-label="Copy the visible text" onclick={() => actOn("copy-pane", focused.pane.id)}><Copy size={12} /></button>
        <div class="ml-1 flex overflow-hidden rounded-md border border-accent/50 text-accent" role="group" aria-label="Attach">
          <button class="flex items-center gap-1 bg-accent/10 px-2 py-1 hover:bg-accent/20" title="Attach to this session in a new terminal tab" onclick={() => attach("tab", focused.session.id)}><SquareTerminal size={13} /> Attach</button>
          <button class="border-l border-accent/40 px-1.5 hover:bg-accent/20" title="Open in a split to the right" aria-label="Open in split right" onclick={() => attach("right", focused.session.id)} disabled={!mon.canSplit}><Columns2 size={13} /></button>
          <button class="border-l border-accent/40 px-1.5 hover:bg-accent/20" title="Open in a split below" aria-label="Open in split down" onclick={() => attach("down", focused.session.id)} disabled={!mon.canSplit}><Rows2 size={13} /></button>
        </div>
      </div>
    </header>

    {#if snap?.stale}
      <div class="shrink-0 border-b border-warning/30 bg-warning/10 px-3 py-1 text-xs text-warning" role="status">The host could not be read just now ({snap.stale.message}). Showing what was last known.</div>
    {/if}

    {#if grid}
      <div class="grid min-h-0 flex-1 auto-rows-[minmax(200px,1fr)] gap-2 overflow-y-auto p-2" style="grid-template-columns:repeat(auto-fill,minmax(300px,1fr))" role="list" aria-label="Pane snapshots">
        {#each tiles as t (t.pane.id)}
          {@const p = mon.previews[key(t.pane.id)]}
          <div class="flex min-h-0 flex-col overflow-hidden rounded-lg border {t.pane.active && t.window.active ? 'border-accent/60' : 'border-line'} bg-panel" role="listitem" oncontextmenu={(e) => { e.preventDefault(); menu = { paneId: t.pane.id, x: e.clientX, y: e.clientY }; }}>
            <div class="flex shrink-0 items-center gap-2 border-b border-line px-2 py-1 text-[11px]">
              <span class="font-mono text-fg">{t.pane.id}</span>
              <span class="truncate text-fg-muted">{t.pane.command || "shell"}{t.pane.title && t.pane.title !== t.pane.command ? ` · ${t.pane.title}` : ""}</span>
              <span class="ml-auto shrink-0 text-fg-muted">{t.window.index}: {t.window.name}</span>
              <button class="shrink-0 rounded px-1 text-accent hover:bg-accent/15" title="Attach to this session" aria-label="Attach to {t.session.name}" onclick={() => attach("tab", t.session.id)}><SquareTerminal size={12} /></button>
            </div>
            <div class="min-h-0 flex-1">
              {#if p?.capture}
                <TmuxScreen text={p.capture.text} ansi={mon.prefs.ansi} maxLines={80} compact ariaLabel="Snapshot of pane {t.pane.id}" />
              {:else if p?.error}
                <p class="p-3 text-xs text-fg-muted">Preview unavailable. {p.error}</p>
              {:else}
                <p class="p-3 text-xs text-fg-muted">Capturing…</p>
              {/if}
            </div>
          </div>
        {/each}
        {#if panes.length > tiles.length}<p class="col-span-full text-center text-xs text-fg-muted">{panes.length - tiles.length} more panes are not drawn. Select a window to see its panes.</p>{/if}
      </div>
    {:else}
      {#if panes.length > 1 && !sel.paneId}
        <div class="flex shrink-0 gap-1 overflow-x-auto border-b border-line bg-panel/30 px-2 py-1" role="tablist" aria-label="Panes">
          {#each panes as t (t.pane.id)}
            <button role="tab" aria-selected={focused.pane.id === t.pane.id} class="shrink-0 rounded px-2 py-0.5 font-mono text-[11px] {focused.pane.id === t.pane.id ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => (chosen = t.pane.id)}>
              {t.pane.id} {t.pane.command}
            </button>
          {/each}
        </div>
      {/if}
      <div class="relative min-h-0 flex-1" oncontextmenu={(e) => { e.preventDefault(); menu = { paneId: focused.pane.id, x: e.clientX, y: e.clientY }; }} role="presentation">
        {#if fp?.capture}
          <TmuxScreen text={fp.capture.text} ansi={mon.prefs.ansi} ariaLabel="Snapshot of pane {focused.pane.id}" />
          {#if fp.error}<div class="absolute inset-x-0 top-0 bg-warning/15 px-3 py-1 text-xs text-warning" role="status">Preview unavailable: {fp.error} Showing the last capture.</div>{/if}
          {#if fp.capture.info.dead}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">The program in this pane has ended.</div>
          {:else if fp.capture.info.in_mode}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">This pane is in tmux copy mode; the snapshot shows what that mode shows.</div>
          {:else if fp.capture.info.alternate && mon.prefs.scrollback > 0}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">A full-screen program is running, so the history above is from before it started.</div>{/if}
        {:else if fp?.error}
          <div class="grid h-full place-items-center p-6 text-center text-sm text-fg-muted" role="status">
            <div>
              <AlertTriangle size={22} class="mx-auto mb-2 text-warning" />
              <p>Preview unavailable.</p>
              <p class="mt-1 text-xs">{fp.error}</p>
              <button class="mt-3 rounded-md border border-line px-3 py-1.5 text-xs hover:bg-panel-hover" onclick={() => mon.capture(key(focused.pane.id))}>Try again</button>
            </div>
          </div>
        {:else}
          <p class="p-6 text-sm text-fg-muted">Capturing the pane…</p>
        {/if}
      </div>
    {/if}
  {/if}
</div>

{#if menu}
  {@const m = menu}
  <TmuxMenu items={menuItems(m.paneId)} x={m.x} y={m.y} label="Actions for pane {m.paneId}" onclose={() => (menu = null)} onpick={(id) => actOn(id, m.paneId)} />
{/if}
