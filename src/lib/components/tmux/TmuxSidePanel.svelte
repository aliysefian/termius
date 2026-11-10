<script lang="ts">
  import { AlertTriangle, ChevronDown, ChevronRight, ExternalLink, Plus, RefreshCw, SquareTerminal, X } from "lucide-svelte";
  import { untrack } from "svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { actionsFor, type ActionId } from "$lib/tmux/actions";
  import { DEFAULT_SOCKET, buildRows, nodeKey, noticeFor, serverKey, type Row } from "$lib/tmux/monitor";
  import Splitter from "./Splitter.svelte";
  import TmuxMenu from "./TmuxMenu.svelte";
  import TmuxSettings from "./TmuxSettings.svelte";

  let { paneId, hostId }: { paneId: string; hostId: string } = $props();

  const socket = DEFAULT_SOCKET;
  const server = $derived(serverKey(hostId, socket));
  const snap = $derived(mon.snapshots[server]);
  const name = $derived(vaultStore.hostById.get(hostId)?.data?.label ?? "host");
  const attachment = $derived(mon.attachmentOf(paneId));
  const failure = $derived(mon.failures[server]);
  const notice = $derived(snap ? noticeFor(snap) : (failure ?? null));

  // The list is kept current while the panel is open, through the shared loop: opening several panels costs one reading per host.
  $effect(() => {
    const [p, h] = [paneId, hostId];
    return untrack(() => mon.watchPanel(p, h, socket));
  });

  const rows = $derived(
    buildRows([{ hostId, name, socket }], new Map(snap ? [[server, snap]] : []), new Set(Object.keys(mon.expanded)), "", mon.tagsFor).filter((r) => r.kind !== "host" && r.kind !== "notice" && r.kind !== "pane"),
  );

  let menu = $state<{ row: Row; x: number; y: number } | null>(null);
  let list = $state<HTMLDivElement>();

  function toggle(r: Row) {
    if (r.expanded) delete mon.expanded[r.key];
    else mon.expanded[r.key] = true;
  }

  /** A click on a session moves this terminal's own tmux client there if it has one; otherwise it opens the session in a new tab (nothing is typed into this terminal). */
  async function open(r: Row) {
    if (!r.sessionId) return;
    if (r.kind === "window" && r.windowId) {
      await mon.act("select-window", r);
      return;
    }
    if (attachment && attachment.server === server) await mon.attach(server, r.sessionId, "replace");
    else await mon.attach(server, r.sessionId, "tab");
  }

  function items(r: Row) {
    return actionsFor(r, snap, { canSplit: mon.canSplit, canReplace: true }).map((a) => ({ id: a.id, label: a.label, danger: a.danger, disabled: a.disabled, why: a.why, group: a.group }));
  }

  function key(e: KeyboardEvent) {
    // Only keys pressed while this panel has focus; the terminal keeps everything typed in it.
    const sel = mon.selectedRow;
    if (e.key === "Escape") {
      e.stopPropagation();
      mon.panelOpen = false;
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const i = rows.findIndex((r) => r.key === mon.selected);
      const n = rows[Math.min(rows.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (n) mon.select(n.key);
    } else if (e.key === "Enter" && sel) {
      e.preventDefault();
      void open(sel);
    } else if (e.key === "ArrowRight" && sel?.expandable && !sel.expanded) toggle(sel);
    else if (e.key === "ArrowLeft" && sel?.expanded) toggle(sel);
  }
  const side = $derived(mon.prefs.panel.side);
</script>

{#snippet header()}
  <header class="flex shrink-0 items-center gap-1 border-b border-line px-2 py-1.5">
    <SquareTerminal size={13} class="shrink-0 text-accent" />
    <span class="min-w-0 truncate text-xs font-medium" title={name}>{name}</span>
    {#if snap?.stale}<span title="The host could not be read just now; this is the last known list."><AlertTriangle size={12} class="shrink-0 text-warning" /></span>{/if}
    <div class="ml-auto flex items-center">
      <button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" title="New session" aria-label="New session" onclick={() => mon.newSession(hostId, socket)}><Plus size={13} /></button>
      <button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" title="Refresh" aria-label="Refresh" onclick={() => mon.refresh(hostId, socket)}><RefreshCw size={12} class={mon.loading[server] ? "animate-spin" : ""} /></button>
      <button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" title="Open the Tmux Monitor" aria-label="Open the Tmux Monitor" onclick={() => { mon.addHost(hostId); ui.view = "tmux"; }}><ExternalLink size={12} /></button>
      <TmuxSettings {hostId} />
      <button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" title="Close the panel (the sessions keep running)" aria-label="Close the tmux panel" onclick={() => (mon.panelOpen = false)}><X size={13} /></button>
    </div>
  </header>
{/snippet}

<div class="flex min-h-0 shrink-0 {side === 'left' ? 'flex-row' : 'flex-row-reverse'}" data-testid="tmux-panel">
  <aside class="flex min-h-0 flex-col bg-panel/30" style="width:{mon.prefs.panel.width}px" aria-label="tmux sessions on {name}">
    {@render header()}
    <div bind:this={list} class="min-h-0 flex-1 overflow-y-auto py-1" role="tree" aria-label="tmux sessions and windows" tabindex="0" onkeydown={key}>
      {#if attachment && attachment.server === server}
        <p class="mx-2 mb-1 rounded bg-accent/10 px-2 py-1 text-[11px] text-accent">This terminal is on {snap?.sessions.find((s) => s.id === attachment.sessionId)?.name ?? "a session"}. Choosing another session moves it.</p>
      {/if}
      {#each rows as r (r.key)}
        <div
          class="group mx-1 flex h-7 cursor-default items-center gap-1 rounded-md pr-1 text-[13px] {mon.selected === r.key ? 'bg-accent/15' : 'hover:bg-panel-hover'}"
          style="padding-left:{(r.depth - 1) * 14 + 4}px"
          role="treeitem"
          tabindex="-1"
          aria-selected={mon.selected === r.key}
          aria-expanded={r.expandable ? r.expanded : undefined}
          aria-level={r.depth}
          onclick={() => mon.select(r.key)}
          ondblclick={() => open(r)}
          oncontextmenu={(e) => { e.preventDefault(); mon.select(r.key); menu = { row: r, x: e.clientX, y: e.clientY }; }}
          onkeydown={() => {}}
        >
          {#if r.expandable}
            <button class="grid h-5 w-5 shrink-0 place-items-center text-fg-muted hover:text-fg" tabindex="-1" aria-label={r.expanded ? "Collapse" : "Expand"} onclick={(e) => { e.stopPropagation(); toggle(r); }}>{#if r.expanded}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}</button>
          {:else}<span class="w-5 shrink-0"></span>{/if}
          <span class="h-1.5 w-1.5 shrink-0 rounded-full {r.kind === 'session' ? (r.attached ? 'bg-success' : 'bg-fg-muted/40') : r.active ? 'bg-accent' : 'bg-fg-muted/30'}"></span>
          <button class="min-w-0 flex-1 truncate text-left" title={r.kind === "session" ? `${r.label}: click to ${attachment && attachment.server === server ? "move this terminal here" : "open in a new tab"}` : r.label} onclick={() => open(r)}>{r.label}</button>
          <span class="shrink-0 text-[10px] text-fg-muted">{r.kind === "session" ? (r.attached ? "attached" : "") : r.meta}</span>
        </div>
      {/each}
      {#if notice && rows.length === 0}
        <div class="m-2 rounded-md border border-line bg-panel/60 p-2 text-xs" role="status">
          <div class="font-medium {notice.tone === 'bad' ? 'text-danger' : notice.tone === 'warn' ? 'text-warning' : ''}">{notice.title}</div>
          <div class="mt-0.5 text-fg-muted">{notice.detail}</div>
          {#if notice.hint}<code class="mt-1 block select-text break-all rounded bg-base px-1.5 py-1 font-mono text-[11px]">{notice.hint}</code>{/if}
          {#if notice.canCreate}<button class="mt-2 rounded-md border border-accent/50 bg-accent/10 px-2 py-1 text-accent hover:bg-accent/20" onclick={() => mon.newSession(hostId, socket)}>Create Session</button>{/if}
        </div>
      {:else if !snap && !failure}
        <p class="p-3 text-xs text-fg-muted">Reading the host's tmux…</p>
      {/if}
    </div>
  </aside>
  <Splitter width={mon.prefs.panel.width} min={200} max={560} side={side === "left" ? "left" : "right"} label="Resize the tmux panel" onchange={(w) => (mon.prefs.panel.width = w)} />
</div>

{#if menu}
  {@const m = menu}
  <TmuxMenu items={items(m.row)} x={m.x} y={m.y} label="Actions for {m.row.label}" onclose={() => (menu = null)} onpick={(id) => mon.act(id as ActionId, m.row)} />
{/if}
