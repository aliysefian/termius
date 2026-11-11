<script lang="ts">
  import { AlertTriangle, Camera, Columns2, Copy, Ellipsis, ExternalLink, RefreshCw, Rows2, SquareTerminal, type Server } from "lucide-svelte";
  import { onMount, untrack } from "svelte";
  import { paneKey, tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { ago, locate, noticeFor, paneRects, type Pane, type Session, type Window } from "$lib/tmux/monitor";
  import TmuxMenu, { type MenuItem } from "./TmuxMenu.svelte";
  import TmuxScreen from "./TmuxScreen.svelte";
  import { actionsFor, type ActionId } from "$lib/tmux/actions";

  // The panes of one window, as read-only snapshots. "Grid" places them where tmux put them (from the window's own layout) when
  // that layout checks out, and in an even grid otherwise; "List" shows one pane large. Nothing here takes input: to type, use the Terminal tab (or Attach).

  const sel = $derived(mon.selectedRow);
  const snap = $derived(sel ? mon.snapshotOf(sel.server) : undefined);
  const here = $derived(sel ? locate(snap, sel) : {});
  const target = $derived(mon.focusTarget);

  type Tile = { window: Window; pane: Pane; session: Session };
  const panes = $derived<Tile[]>(target ? target.window.panes.map((pane) => ({ session: target.session, window: target.window, pane })) : []);
  const focused = $derived<Tile | null>(target?.pane ? { session: target.session, window: target.window, pane: target.pane } : null);
  const grid = $derived(mon.tab === "grid" && panes.length > 1);
  const tiles = $derived(panes.slice(0, mon.prefs.preview.maxThumbnails));
  const rects = $derived(target && grid && tiles.length === panes.length ? paneRects(target.window) : null);
  const key = (id: string) => paneKey(sel!.server, id);

  // Tell the store what is on screen: only that is captured. The store is written without being tracked (this effect must not
  // depend on what it writes), and cleared once, when the preview goes away.
  const keysOnScreen = $derived(!sel ? [] : grid ? tiles.map((t) => key(t.pane.id)) : focused ? [key(focused.pane.id)] : []);
  const focusOnScreen = $derived(sel && focused ? key(focused.pane.id) : null);
  $effect(() => {
    const [keys, focusKey] = [keysOnScreen, focusOnScreen];
    untrack(() => {
      mon.focusKey = focusKey;
      mon.setVisiblePanes(keys);
    });
  });
  $effect(() => () => {
    untrack(() => {
      mon.focusKey = null;
      mon.setVisiblePanes([]);
    });
  });
  // Capture at once when something new comes on screen, instead of waiting for the next tick.
  $effect(() => {
    const keys = keysOnScreen;
    untrack(() => {
      for (const k of keys) if (!mon.previews[k]) void mon.capture(k);
    });
  });

  let now = $state(Date.now());
  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const SCROLLBACKS = [0, 200, 1000, 5000];
  const fp = $derived(focused ? mon.previews[key(focused.pane.id)] : undefined);

  let menu = $state<{ paneId: string; x: number; y: number } | null>(null);
  const ICONS: Partial<Record<ActionId, typeof Server>> = { "attach-live": SquareTerminal, "attach-tab": SquareTerminal, "split-right": Columns2, "split-down": Rows2, "copy-pane": Copy };
  function menuItems(paneId: string): MenuItem[] {
    if (!sel) return [];
    const p = panes.find((x) => x.pane.id === paneId);
    const row = { kind: "pane" as const, sessionId: p?.session.id ?? sel.sessionId, windowId: p?.window.id, paneId };
    return actionsFor(row, snap, { canSplit: mon.canSplit, canReplace: false, live: { allowed: !mon.liveBlocker(sel.hostId), why: mon.liveBlocker(sel.hostId) ?? undefined } }).map((a) => ({ id: a.id, label: a.label, danger: a.danger, disabled: a.disabled, why: a.why, group: a.group, icon: ICONS[a.id] }));
  }
  function actOn(id: string, paneId: string) {
    if (!sel) return;
    const p = panes.find((x) => x.pane.id === paneId);
    void mon.act(id as ActionId, { kind: "pane", server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: p?.session.id, windowId: p?.window.id, paneId });
  }
  const attach = (mode: "tab" | "right" | "down", sessionId: string) => sel && mon.attach(sel.server, sessionId, mode);
  const pick = (t: Tile) => sel && mon.reveal(sel.server, t.session.id, t.window.id, t.pane.id);
  const refreshShown = () => {
    for (const k of mon.visiblePanes) void mon.capture(k);
  };
  const live = $derived(mon.prefs.live);
</script>

{#snippet tile(t: Tile, style: string)}
  {@const p = mon.previews[key(t.pane.id)]}
  {@const on = sel?.paneId === t.pane.id}
  <div
    class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-panel {on ? 'border-accent ring-2 ring-accent/50' : t.pane.active ? 'border-accent/40' : 'border-line'}"
    {style}
    role="listitem"
    oncontextmenu={(e) => { e.preventDefault(); menu = { paneId: t.pane.id, x: e.clientX, y: e.clientY }; }}
  >
    <div class="flex shrink-0 items-center gap-1.5 border-b border-line px-2 py-1 text-[11px]">
      <span class="h-2 w-2 shrink-0 rounded-full {t.pane.dead ? 'bg-danger' : t.pane.active ? 'bg-success' : 'bg-fg-muted/40'}" title={t.pane.dead ? "Ended" : t.pane.active ? "Active pane" : "Not the active pane"}></span>
      <button class="flex min-w-0 flex-1 items-center gap-1.5 text-left" aria-pressed={on} title="Select pane {t.pane.id}" onclick={() => pick(t)}>
        <span class="font-mono font-semibold text-fg">{t.pane.id}</span>
        <span class="truncate text-fg-muted">{t.pane.command || "shell"}</span>
      </button>
      <button class="shrink-0 rounded p-0.5 text-fg-muted hover:bg-accent/15 hover:text-accent" title="Open session {t.session.name} in a terminal tab" aria-label="Open {t.session.name} in a terminal tab" onclick={() => attach("tab", t.session.id)}><ExternalLink size={12} /></button>
      <button class="shrink-0 rounded p-0.5 text-fg-muted hover:bg-panel-hover hover:text-fg" title="More actions" aria-label="More actions for pane {t.pane.id}" aria-haspopup="menu" onclick={(e) => { const r = e.currentTarget.getBoundingClientRect(); menu = { paneId: t.pane.id, x: r.left, y: r.bottom }; }}><Ellipsis size={12} /></button>
    </div>
    <div class="min-h-0 flex-1">
      {#if p?.capture}
        <TmuxScreen text={p.capture.text} ansi={mon.prefs.ansi} maxLines={80} compact ariaLabel="Snapshot of pane {t.pane.id}" />
      {:else if p?.error}
        <div class="p-3 text-xs text-fg-muted" role="status">
          <p>Unable to capture pane output.</p>
          <button class="mt-2 rounded border border-line px-2 py-0.5 hover:bg-panel-hover" onclick={() => mon.capture(key(t.pane.id))}>Retry</button>
        </div>
      {:else}
        <p class="p-3 text-xs text-fg-muted">Capturing…</p>
      {/if}
    </div>
  </div>
{/snippet}

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" aria-label="Pane preview">
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
        <p>This {here.session ? "window has no panes" : `${sel.kind} no longer exists on the remote host`}.</p>
        <button class="mt-3 rounded-md border border-line px-3 py-1.5 text-xs hover:bg-panel-hover" onclick={() => mon.refresh(sel.hostId, sel.socket)}>Refresh Sessions</button>
      </div>
    </div>
  {:else}
    <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-line bg-panel/40 px-3 py-1.5 text-xs">
      <span class="flex items-center gap-1.5 rounded border border-line px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-fg-muted" title="Captured from tmux and redrawn on a timer. It does not accept keyboard input; attach to type in the session.">
        <span class="h-1.5 w-1.5 rounded-full {live ? 'bg-success' : 'bg-fg-muted/50'}"></span>{live ? "Live Preview" : "Preview paused"} · Read Only
      </span>
      <span class="truncate text-fg-muted">
        {#if grid}{panes.length} panes{:else}{focused.pane.command || "shell"} · {fp?.capture?.info.width ?? focused.pane.width}×{fp?.capture?.info.height ?? focused.pane.height}{/if}
        {#if fp && !grid}· {fp.loading ? "refreshing…" : fp.capture ? `captured ${ago((now - fp.capture.taken_at_ms) / 1000)}` : ""}{/if}
      </span>
      <div class="ml-auto flex flex-wrap items-center gap-1.5">
        <label class="flex items-center gap-1 text-fg-muted" title="Lines of history shown above the screen"
          >History
          <select class="rounded border border-line bg-base px-1 py-0.5 text-xs text-fg" value={mon.prefs.scrollback} onchange={(e) => { mon.prefs.scrollback = Number(e.currentTarget.value); refreshShown(); }}>
            {#each SCROLLBACKS as n (n)}<option value={n}>{n === 0 ? "Screen only" : `${n} lines`}</option>{/each}
          </select>
        </label>
        <label class="flex items-center gap-1 text-fg-muted"><input type="checkbox" checked={mon.prefs.ansi} onchange={(e) => { mon.prefs.ansi = e.currentTarget.checked; refreshShown(); }} /> Colours</label>
        <button class="grid h-6 w-6 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" title="Refresh the preview" aria-label="Refresh the preview" onclick={refreshShown}><RefreshCw size={12} class={fp?.loading ? "animate-spin" : ""} /></button>
        <button class="grid h-6 w-6 place-items-center rounded-md border border-line text-fg-muted hover:bg-panel-hover hover:text-fg" title="Copy the visible text" aria-label="Copy the visible text" onclick={() => actOn("copy-pane", focused.pane.id)}><Copy size={12} /></button>
        <button class="flex items-center gap-1 rounded-md bg-accent-solid px-2 py-1 text-white disabled:opacity-50" title={mon.liveBlocker(sel.hostId) ?? "Type in this session: open a live terminal here in the workspace"} disabled={!!mon.liveBlocker(sel.hostId) || !!snap?.stale} onclick={() => actOn("attach-live", focused.pane.id)}><SquareTerminal size={13} /> Live Terminal</button>
        <div class="flex overflow-hidden rounded-md border border-accent/50 text-accent" role="group" aria-label="Attach">
          <button class="flex items-center gap-1 bg-accent/10 px-2 py-1 hover:bg-accent/20" title="Attach to this session in a new terminal tab" onclick={() => attach("tab", focused.session.id)}><SquareTerminal size={13} /> Attach</button>
          <button class="border-l border-accent/40 px-1.5 hover:bg-accent/20 disabled:opacity-40" title="Open in a split to the right" aria-label="Open in split right" onclick={() => attach("right", focused.session.id)} disabled={!mon.canSplit}><Columns2 size={13} /></button>
          <button class="border-l border-accent/40 px-1.5 hover:bg-accent/20 disabled:opacity-40" title="Open in a split below" aria-label="Open in split down" onclick={() => attach("down", focused.session.id)} disabled={!mon.canSplit}><Rows2 size={13} /></button>
        </div>
      </div>
    </div>

    {#if snap?.stale}
      <div class="flex shrink-0 items-center gap-2 border-b border-warning/30 bg-warning/10 px-3 py-1 text-xs text-warning" role="status">
        <span>Connection to this host was lost ({snap.stale.message}). Showing what was last known, not live output.</span>
        <button class="ml-auto rounded border border-warning/40 px-2 py-0.5 hover:bg-warning/10" onclick={() => mon.refresh(sel.hostId, sel.socket)}>Reconnect</button>
      </div>
    {/if}

    {#if grid && rects}
      <!-- Placed where tmux put them: each pane's share of the window, with a gap between neighbours. -->
      <div class="relative min-h-0 flex-1 overflow-hidden p-1" role="list" aria-label="Pane snapshots, laid out as in the tmux window">
        <div class="relative h-full w-full">
          {#each tiles as t (t.pane.id)}
            {@const r = rects.get(t.pane.id)}
            {#if r}
              <div class="absolute p-1" style="left:{r.left}%;top:{r.top}%;width:{r.width}%;height:{r.height}%">
                {@render tile(t, "height:100%")}
              </div>
            {/if}
          {/each}
        </div>
      </div>
    {:else if grid}
      <div class="grid min-h-0 flex-1 auto-rows-[minmax(200px,1fr)] gap-2 overflow-y-auto p-2" style="grid-template-columns:repeat(auto-fill,minmax(300px,1fr))" role="list" aria-label="Pane snapshots">
        {#each tiles as t (t.pane.id)}{@render tile(t, "")}{/each}
        {#if panes.length > tiles.length}<p class="col-span-full text-center text-xs text-fg-muted">{panes.length - tiles.length} more panes are not drawn. Raise "Panes drawn at once" in Tmux settings.</p>{/if}
      </div>
    {:else}
      {#if panes.length > 1}
        <div class="flex shrink-0 gap-1 overflow-x-auto border-b border-line bg-panel/30 px-2 py-1" role="tablist" aria-label="Panes">
          {#each panes as t (t.pane.id)}
            <button role="tab" aria-selected={focused.pane.id === t.pane.id} class="shrink-0 rounded px-2 py-0.5 font-mono text-[11px] {focused.pane.id === t.pane.id ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => pick(t)}>
              {t.pane.id} {t.pane.command}
            </button>
          {/each}
        </div>
      {/if}
      <div class="relative min-h-0 flex-1" oncontextmenu={(e) => { e.preventDefault(); menu = { paneId: focused.pane.id, x: e.clientX, y: e.clientY }; }} role="presentation">
        {#if fp?.capture}
          <TmuxScreen text={fp.capture.text} ansi={mon.prefs.ansi} ariaLabel="Snapshot of pane {focused.pane.id}" />
          {#if fp.error}<div class="absolute inset-x-0 top-0 bg-warning/15 px-3 py-1 text-xs text-warning" role="status">Unable to capture pane output: {fp.error} Showing the last capture.</div>{/if}
          {#if fp.capture.info.dead}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">The program in this pane has ended.</div>
          {:else if fp.capture.info.in_mode}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">This pane is in tmux copy mode; the snapshot shows what that mode shows.</div>
          {:else if fp.capture.info.alternate && mon.prefs.scrollback > 0}<div class="absolute inset-x-0 bottom-0 bg-base/90 px-3 py-1 text-xs text-fg-muted">A full-screen program is running, so the history above is from before it started.</div>{/if}
        {:else if fp?.error}
          <div class="grid h-full place-items-center p-6 text-center text-sm text-fg-muted" role="status">
            <div>
              <AlertTriangle size={22} class="mx-auto mb-2 text-warning" />
              <p>Unable to capture pane output.</p>
              <p class="mt-1 text-xs">{fp.error}</p>
              <button class="mt-3 rounded-md border border-line px-3 py-1.5 text-xs hover:bg-panel-hover" onclick={() => mon.capture(key(focused.pane.id))}>Retry</button>
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
