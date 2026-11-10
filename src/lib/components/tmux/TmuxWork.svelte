<script lang="ts">
  import { AppWindow, ChevronRight, LayoutGrid, List, Gauge, Info, Layers, Server } from "lucide-svelte";
  import { tmuxMonitor as mon, type WorkTab } from "$lib/stores/tmuxmonitor.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { nodeKey, paneCount, paneRects, when } from "$lib/tmux/monitor";
  import TmuxPreview from "./TmuxPreview.svelte";
  import TmuxResources from "./TmuxResources.svelte";

  // The middle of the page: where you are (host › session › window), the ways to look at it, and the view itself.
  const sel = $derived(mon.selectedRow);
  const target = $derived(mon.focusTarget);
  const hostName = $derived(sel ? (vaultStore.hostById.get(sel.hostId)?.data?.label ?? "host") : "");

  const tabs = $derived<{ id: WorkTab; label: string; icon: typeof Server }[]>([
    { id: "grid", label: "Panes Grid", icon: LayoutGrid },
    { id: "list", label: "Panes List", icon: List },
    { id: "windows", label: `Windows${target ? ` (${target.session.windows.length})` : ""}`, icon: AppWindow },
    { id: "info", label: "Session Info", icon: Info },
    { id: "resources", label: "Resources", icon: Gauge },
  ]);
  function choose(id: WorkTab) {
    mon.tab = id;
    if (id === "grid" || id === "list") mon.prefs.mode = id;
  }
  function tabKey(e: KeyboardEvent) {
    const at = tabs.findIndex((t) => t.id === mon.tab);
    const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const next = tabs[(at + d + tabs.length) % tabs.length];
    choose(next.id);
    queueMicrotask(() => document.getElementById(`tmux-tab-${next.id}`)?.focus());
  }
  const pickWindow = (id: string) => target && sel && mon.revealWindow(sel.server, target.session.id, id);
</script>

<section class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden" aria-label="Selection">
  {#if sel && target}
    <div class="flex shrink-0 flex-wrap items-center gap-1.5 px-3 pt-2.5 text-sm" aria-label="Where you are">
      <button class="flex items-center gap-1 text-fg-muted hover:text-fg" onclick={() => mon.select(nodeKey.host(sel.server))}><Server size={13} /> {hostName}</button>
      <ChevronRight size={13} class="text-fg-muted" />
      <button class="flex items-center gap-1 font-medium hover:text-accent" onclick={() => mon.select(nodeKey.session(sel.server, target.session.id))}><Layers size={13} /> {target.session.name}</button>
      <ChevronRight size={13} class="text-fg-muted" />
      <button class="flex items-center gap-1 font-medium hover:text-accent" onclick={() => pickWindow(target.window.id)}>{target.window.index}: {target.window.name}</button>
      <span class="rounded-full bg-panel px-2 py-0.5 text-[11px] text-fg-muted">{target.window.panes.length} {target.window.panes.length === 1 ? "pane" : "panes"}</span>
      {#if sel.kind === "pane" && target.pane}<ChevronRight size={13} class="text-fg-muted" /><span class="font-mono text-xs">{target.pane.id}</span>{/if}
    </div>
    <div class="flex shrink-0 flex-wrap gap-1 border-b border-line px-3 pb-2 pt-2" role="tablist" aria-label="View" tabindex="-1" onkeydown={tabKey}>
      {#each tabs as t (t.id)}
        <button
          id="tmux-tab-{t.id}"
          role="tab"
          aria-selected={mon.tab === t.id}
          tabindex={mon.tab === t.id ? 0 : -1}
          class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs transition-colors {mon.tab === t.id ? 'bg-accent-solid text-white' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          onclick={() => choose(t.id)}
        ><t.icon size={14} /> {t.label}</button>
      {/each}
    </div>

    {#if mon.tab === "grid" || mon.tab === "list"}
      <TmuxPreview />
    {:else if mon.tab === "windows"}
      <div class="min-h-0 flex-1 overflow-y-auto p-3" role="tabpanel" aria-label="Windows">
        <ul class="grid gap-3" style="grid-template-columns:repeat(auto-fill,minmax(220px,1fr))">
          {#each target.session.windows as w (w.id)}
            {@const rects = paneRects(w)}
            {@const on = w.id === target.window.id}
            <li class="overflow-hidden rounded-lg border {on ? 'border-accent ring-1 ring-accent/40' : 'border-line'} bg-panel">
              <button class="block w-full text-left" aria-pressed={on} onclick={() => pickWindow(w.id)}>
                <div class="relative m-2 h-24 overflow-hidden rounded bg-base" aria-hidden="true">
                  {#if rects}
                    {#each w.panes as p (p.id)}
                      {@const r = rects.get(p.id)}
                      {#if r}<div class="absolute p-px" style="left:{r.left}%;top:{r.top}%;width:{r.width}%;height:{r.height}%"><div class="h-full w-full rounded-sm border {p.active ? 'border-accent bg-accent/15' : 'border-line bg-panel'}"></div></div>{/if}
                    {/each}
                  {:else}
                    <div class="grid h-full place-items-center text-[11px] text-fg-muted">{w.panes.length} {w.panes.length === 1 ? "pane" : "panes"}</div>
                  {/if}
                </div>
                <div class="flex items-center gap-2 px-3 pb-2 text-xs">
                  <span class="h-2 w-2 rounded-full {w.active ? 'bg-success' : 'bg-fg-muted/40'}" title={w.active ? "Active window" : ""}></span>
                  <span class="min-w-0 flex-1 truncate font-medium">{w.index}: {w.name}</span>
                  <span class="text-fg-muted">{w.panes.length}p{w.zoomed ? " · zoomed" : ""}</span>
                </div>
              </button>
              <div class="flex gap-1 border-t border-line px-2 py-1 text-[11px]">
                <button class="rounded px-1.5 py-0.5 text-fg-muted hover:bg-panel-hover hover:text-fg disabled:opacity-40" disabled={!!mon.snapshotOf(sel.server)?.stale || w.active} title="Make this the window tmux shows in the session" onclick={() => mon.act("select-window", { kind: "window", server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: target.session.id, windowId: w.id })}>Make active</button>
                <button class="rounded px-1.5 py-0.5 text-fg-muted hover:bg-panel-hover hover:text-fg" onclick={() => mon.act("rename-window", { kind: "window", server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: target.session.id, windowId: w.id })}>Rename</button>
                <button class="ml-auto rounded px-1.5 py-0.5 text-danger hover:bg-danger/10" onclick={() => mon.act("kill-window", { kind: "window", server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: target.session.id, windowId: w.id })}>Kill…</button>
              </div>
            </li>
          {/each}
        </ul>
      </div>
    {:else if mon.tab === "info"}
      {@const s = target.session}
      <div class="min-h-0 flex-1 overflow-y-auto p-4" role="tabpanel" aria-label="Session info">
        <dl class="grid max-w-xl grid-cols-[8rem_1fr] gap-x-4 gap-y-2 text-sm">
          <dt class="text-fg-muted">Name</dt><dd class="break-all font-medium">{s.name}</dd>
          <dt class="text-fg-muted">Host</dt><dd>{hostName}</dd>
          <dt class="text-fg-muted">ID</dt><dd class="font-mono">{s.id}</dd>
          <dt class="text-fg-muted">State</dt><dd>{s.attached > 0 ? `Attached (${s.attached} ${s.attached === 1 ? "client" : "clients"})` : "Detached"}</dd>
          <dt class="text-fg-muted">Created</dt><dd>{when(s.created)}</dd>
          {#if s.activity > 0}<dt class="text-fg-muted">Last activity</dt><dd>{when(s.activity)}</dd>{/if}
          <dt class="text-fg-muted">Windows</dt><dd>{s.windows.length}</dd>
          <dt class="text-fg-muted">Panes</dt><dd>{paneCount(s)}</dd>
          {#if s.group}<dt class="text-fg-muted">Group</dt><dd>{s.group}</dd>{/if}
          <dt class="text-fg-muted">Mouse mode</dt><dd>{s.mouse === null ? "Not reported by this tmux" : s.mouse ? "On" : "Off"}</dd>
          <dt class="text-fg-muted">Tags</dt><dd class="flex flex-wrap gap-1">{#each mon.tagsFor(sel.server, s.name) as t (t)}<span class="rounded bg-accent/15 px-1.5 py-0.5 text-[11px] text-accent">{t}</span>{:else}<span class="text-fg-muted">None</span>{/each}</dd>
        </dl>
      </div>
    {:else}
      <div class="min-h-0 flex-1 overflow-y-auto p-4" role="tabpanel" aria-label="Resources">
        <TmuxResources server={sel.server} session={target.session} />
      </div>
    {/if}
  {:else}
    <TmuxPreview />
  {/if}
</section>
