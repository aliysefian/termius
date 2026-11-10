<script lang="ts">
  import { AlertTriangle, AppWindow, ChevronDown, ChevronRight, Columns2, Copy, Eye, Layers, LayoutPanelTop, Maximize2, Pencil, PanelsTopLeft, Plus, RefreshCw, Rows2, Search, Server, SquareTerminal, Tag, Trash2, Unplug, X } from "lucide-svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { actionsFor, type ActionId } from "$lib/tmux/actions";
  import { bytes, parentOf, step, type Row } from "$lib/tmux/monitor";
  import TmuxMenu, { type MenuItem } from "./TmuxMenu.svelte";

  const rows = $derived(mon.rows);
  let menu = $state<{ row: Row; x: number; y: number } | null>(null);
  let list = $state<HTMLDivElement>();
  let adding = $state("");
  let filter = $state<HTMLInputElement>();
  /** Lets the page put the cursor in the filter (Ctrl+F, "/"). */
  export function focusFilter() {
    filter?.focus();
  }
  const hostCount = $derived(new Set(mon.entries.map((e) => e.hostId)).size);

  /** How a host's tmux server is doing, as a colour: read fine, last-known (stale), unreadable, or not read yet. */
  function health(r: Row): { tone: string; text: string } {
    const snap = mon.snapshotOf(r.server);
    if (!snap) return mon.loading[r.server] ? { tone: "bg-fg-muted/50 animate-pulse", text: "Loading sessions from this host…" } : { tone: "bg-fg-muted/40", text: "Not read yet" };
    if (snap.stale) return { tone: "bg-warning", text: "Connection to this host was lost; showing the last known state" };
    if (snap.state.kind === "failed" || snap.state.kind === "permission_denied") return { tone: "bg-danger", text: "This host could not be read" };
    if (snap.state.kind === "ready") return { tone: "bg-success", text: "Connected" };
    return { tone: "bg-fg-muted/60", text: "Connected, no tmux sessions" };
  }
  /** "12.4% · 284 MB" for a session, only when it was measured. */
  function usage(r: Row): string {
    if (r.kind !== "session" || !r.sessionId) return "";
    const u = mon.resources[r.server]?.sessions.find((x) => x.session_id === r.sessionId)?.usage;
    if (!u) return "";
    return `${u.cpu_pct === null ? "" : `${u.cpu_pct.toFixed(1)}% · `}${bytes(u.rss)}`;
  }

  const ICONS: Partial<Record<ActionId, typeof Server>> = {
    refresh: RefreshCw, "new-session": Plus, "remove-host": X, "attach-tab": SquareTerminal, "attach-right": Columns2, "attach-down": Rows2, "attach-replace": LayoutPanelTop,
    "rename-session": Pencil, "rename-window": Pencil, "new-window": Plus, tags: Tag, "kill-session": Trash2, "kill-window": Trash2, "kill-pane": Trash2, "split-right": Columns2,
    "split-down": Rows2, "zoom-pane": Maximize2, "copy-pane": Copy, "select-pane": Eye, "select-window": Eye, "title-pane": Pencil,
  };

  function items(row: Row): MenuItem[] {
    const snap = mon.snapshotOf(row.server);
    return actionsFor(row, snap, { canSplit: mon.canSplit, canReplace: mon.canReplaceOn(row.hostId) }).map((a) => ({ id: a.id, label: a.label, danger: a.danger, disabled: a.disabled, why: a.why, group: a.group, icon: ICONS[a.id] }));
  }

  function openMenu(row: Row, x: number, y: number) {
    mon.select(row.key);
    menu = { row, x, y };
  }

  function key(e: KeyboardEvent) {
    const sel = mon.selectedRow;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      mon.select(step(rows, mon.selected, e.key === "ArrowDown" ? 1 : -1));
    } else if (e.key === "ArrowRight" && sel) {
      e.preventDefault();
      if (sel.expandable && !sel.expanded) mon.setOpen(sel, true);
      else mon.select(step(rows, sel.key, 1));
    } else if (e.key === "ArrowLeft" && sel) {
      e.preventDefault();
      if (sel.expandable && sel.expanded) mon.setOpen(sel, false);
      else {
        const p = parentOf(rows, sel);
        if (p) mon.select(p.key);
      }
    } else if (e.key === "Enter" && sel) {
      e.preventDefault();
      if (sel.kind === "session" && sel.sessionId) void mon.attach(sel.server, sel.sessionId, "tab");
      else mon.toggle(sel);
    } else if ((e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) && sel) {
      e.preventDefault();
      const el = list?.querySelector<HTMLElement>(`[data-key="${CSS.escape(sel.key)}"]`);
      const r = el?.getBoundingClientRect();
      openMenu(sel, (r?.left ?? 0) + 24, (r?.bottom ?? 0) + 2);
    } else if (e.key === "n" && !e.ctrlKey && !e.metaKey && sel) {
      e.preventDefault();
      void mon.act("new-session", sel);
    } else if (e.key === "r" && !e.ctrlKey && !e.metaKey && sel) {
      e.preventDefault();
      void mon.act("refresh", sel);
    }
  }

  const dot = (r: Row) => (r.kind === "session" ? (r.attached ? "bg-success" : "bg-fg-muted/40") : r.kind === "window" || r.kind === "pane" ? (r.active ? "bg-accent" : "bg-fg-muted/30") : "");
  const tone = { info: "text-fg-muted", warn: "text-warning", bad: "text-danger" } as const;
</script>

<div class="shrink-0 space-y-2 border-b border-line p-2">
  <div class="relative">
    <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
    <input
      bind:this={filter}
      class="w-full rounded-lg border border-line bg-base py-1.5 pl-8 pr-7 text-xs focus:border-accent focus:outline-none"
      placeholder="Search sessions, windows, or panes…"
      aria-label="Search sessions, windows, panes, commands and tags"
      bind:value={mon.query}
    />
    {#if mon.query}<button class="absolute right-1.5 top-1/2 grid h-5 w-5 -translate-y-1/2 place-items-center rounded text-fg-muted hover:text-fg" aria-label="Clear search" onclick={() => (mon.query = "")}><X size={12} /></button>{/if}
  </div>
  <div class="flex items-center gap-2">
    <span class="flex items-center gap-1.5 text-xs font-semibold"><Server size={13} class="text-fg-muted" /> All Hosts ({hostCount})</span>
    <select
      class="ml-auto max-w-40 rounded-md border border-line bg-base px-1.5 py-1 text-[11px] text-fg focus:border-accent focus:outline-none"
      aria-label="Add a host to the monitor"
      bind:value={adding}
      onchange={() => { if (adding) mon.addHost(adding as never); adding = ""; }}
    >
      <option value="">+ Add host…</option>
      {#each mon.addable as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
    </select>
  </div>
</div>

<div bind:this={list} class="min-h-0 flex-1 overflow-y-auto py-1" role="tree" aria-label="Hosts, sessions, windows and panes" tabindex="0" onkeydown={key}>
  {#each rows as row (row.key)}
    {#if row.kind === "notice" && row.notice}
      <div class="mx-2 my-1 rounded-md border border-line bg-panel/60 px-3 py-2 text-xs" style="margin-left:{row.depth * 14 + 8}px" role="status">
        <div class="flex items-start gap-1.5 {tone[row.notice.tone]}">
          <AlertTriangle size={13} class="mt-px shrink-0" />
          <div class="min-w-0">
            <div class="font-medium">{row.notice.title}</div>
            <div class="mt-0.5 text-fg-muted">{row.notice.detail}</div>
            {#if row.notice.hint}<code class="mt-1 block select-text break-all rounded bg-base px-1.5 py-1 font-mono text-[11px] text-fg">{row.notice.hint}</code>{/if}
            {#if row.notice.canCreate}
              <button class="mt-2 rounded-md border border-accent/50 bg-accent/10 px-2 py-1 text-xs text-accent hover:bg-accent/20" onclick={() => mon.act("new-session", row)}>Create Session</button>
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div
        data-key={row.key}
        class="group mx-1 flex h-7 cursor-default items-center gap-1 rounded-md pr-1 text-[13px] {mon.selected === row.key ? 'bg-accent/15 text-fg' : 'hover:bg-panel-hover'} {row.kind === 'host' ? 'mt-1 font-medium' : ''}"
        style="padding-left:{row.depth * 14 + 4}px"
        role="treeitem"
        tabindex="-1"
        aria-level={row.depth + 1}
        aria-selected={mon.selected === row.key}
        aria-expanded={row.expandable ? row.expanded : undefined}
        onclick={() => mon.select(row.key)}
        ondblclick={() => (row.kind === "session" && row.sessionId ? void mon.attach(row.server, row.sessionId, "tab") : mon.toggle(row))}
        oncontextmenu={(e) => { e.preventDefault(); openMenu(row, e.clientX, e.clientY); }}
        onkeydown={() => {}}
      >
        {#if row.expandable}
          <button class="grid h-5 w-5 shrink-0 place-items-center rounded text-fg-muted hover:text-fg" tabindex="-1" aria-label={row.expanded ? "Collapse" : "Expand"} onclick={(e) => { e.stopPropagation(); mon.toggle(row); }}>
            {#if row.expanded}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
          </button>
        {:else}<span class="w-5 shrink-0"></span>{/if}
        {#if row.kind === "host"}
          {@const h = health(row)}
          <span class="h-2 w-2 shrink-0 rounded-full {h.tone}" title={h.text} role="img" aria-label={h.text}></span>
        {:else}
          {@const Icon = row.kind === "session" ? Layers : row.kind === "window" ? AppWindow : PanelsTopLeft}
          <Icon size={13} class="shrink-0 {row.kind === 'session' && row.attached ? 'text-success' : row.active ? 'text-accent' : 'text-fg-muted'}" />
        {/if}
        <span class="min-w-0 truncate {row.kind === 'pane' ? 'font-mono text-xs' : ''}" title={row.label}>{row.label}</span>
        {#each row.tags ?? [] as tag (tag)}<span class="shrink-0 rounded bg-accent/15 px-1 text-[10px] text-accent">{tag}</span>{/each}
        <span class="ml-auto shrink-0 truncate pl-2 text-[11px] text-fg-muted">{usage(row) || row.meta}</span>
        {#if row.stale}<span class="shrink-0" title="The host could not be read just now; this is the last known state."><Unplug size={12} class="text-warning" /></span>{/if}
      </div>
    {/if}
  {/each}
  {#if rows.length === 0}
    <p class="px-4 py-6 text-center text-xs text-fg-muted">{mon.query ? "Nothing matches that search." : "No hosts yet. Add one with “Add host” above."}</p>
  {/if}
</div>

{#if menu}
  {@const m = menu}
  <TmuxMenu items={items(m.row)} x={m.x} y={m.y} label="Actions for {m.row.label}" onclose={() => (menu = null)} onpick={(id) => mon.act(id as ActionId, m.row)} />
{/if}
