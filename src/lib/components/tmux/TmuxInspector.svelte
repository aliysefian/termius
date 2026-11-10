<script lang="ts">
  import { Columns2, Pencil, Rows2, SquareTerminal, Tag, Trash2 } from "lucide-svelte";
  import { paneKey, tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { bytes, locate, paneCount, socketLabel, when, type Usage } from "$lib/tmux/monitor";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const sel = $derived(mon.selectedRow);
  const snap = $derived(sel ? mon.snapshotOf(sel.server) : undefined);
  const here = $derived(sel ? locate(snap, sel) : {});
  const hostName = $derived(sel ? (vaultStore.hostById.get(sel.hostId)?.data?.label ?? "host") : "");
  const res = $derived(sel ? mon.resources[sel.server] : undefined);
  const resError = $derived(sel ? mon.resourceError[sel.server] : undefined);
  const sessionUsage = $derived(here.session ? res?.sessions.find((s) => s.session_id === here.session!.id) : undefined);
  const paneUsage = $derived<Usage | undefined>(sel?.paneId ? res?.panes[sel.paneId] : undefined);
  const lastText = $derived(sel?.paneId ? mon.previews[paneKey(sel.server, sel.paneId)]?.capture?.text : undefined);

  // Figures follow the selection (and the store's timer keeps them current while it is on screen).
  $effect(() => {
    if (sel && here.session) void mon.loadResources(sel.server);
  });

  const cpu = (u: Usage) => (u.cpu_pct === null ? "measuring…" : `${u.cpu_pct.toFixed(1)}%`);
</script>

{#snippet usage(u: Usage | undefined, seen?: number, total?: number)}
  {#if u}
    <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
      <dt class="text-fg-muted">CPU</dt><dd class="tabular-nums">{cpu(u)}</dd>
      <dt class="text-fg-muted">RAM</dt><dd class="tabular-nums">{bytes(u.rss)}</dd>
      <dt class="text-fg-muted">Processes</dt><dd class="tabular-nums">{u.processes}</dd>
      {#if u.top.length}<dt class="text-fg-muted">Running</dt><dd class="truncate font-mono" title={u.top.map(([n, c]) => `${n} ×${c}`).join(", ")}>{u.top.map(([n, c]) => (c > 1 ? `${n} ×${c}` : n)).join(", ")}</dd>{/if}
    </dl>
    {#if seen !== undefined && total !== undefined && seen < total}<p class="mt-1 text-[11px] text-warning">{total - seen} of {total} panes could not be measured.</p>{/if}
    <p class="mt-1.5 text-[11px] text-fg-muted">Estimate from the processes under the pane shells (resident memory, summed per process). Not container or cgroup accounting.</p>
  {:else if resError}
    <p class="text-xs text-fg-muted">Not measured: {resError}</p>
  {:else if res}
    <p class="text-xs text-fg-muted">Not measured: none of the panes' processes could be seen from this user.</p>
  {:else}
    <p class="text-xs text-fg-muted">Measuring…</p>
  {/if}
{/snippet}

{#snippet section(title: string)}
  <h3 class="mb-1.5 mt-4 text-[11px] font-semibold uppercase tracking-wide text-fg-muted first:mt-0">{title}</h3>
{/snippet}

<aside class="flex h-full min-w-0 flex-col overflow-y-auto bg-panel/40 p-3 text-sm" aria-label="Inspector">
  {#if !sel}
    <p class="p-3 text-center text-xs text-fg-muted">Details of the selected host, session, window or pane appear here.</p>
  {:else if sel.kind === "host"}
    {@render section("Host")}
    <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
      <dt class="text-fg-muted">Host</dt><dd>{hostName}</dd>
      <dt class="text-fg-muted">Socket</dt><dd class="break-all font-mono">{socketLabel(sel.socket)}</dd>
      <dt class="text-fg-muted">tmux</dt><dd>{snap?.version?.raw ?? "unknown"}</dd>
      {#if snap?.server}
        <dt class="text-fg-muted">Server pid</dt><dd class="tabular-nums">{snap.server.pid}</dd>
        <dt class="text-fg-muted">Started</dt><dd>{when(snap.server.started)}</dd>
        <dt class="text-fg-muted">Socket path</dt><dd class="break-all font-mono">{snap.server.socket_path}</dd>
      {/if}
      <dt class="text-fg-muted">Sessions</dt><dd>{snap?.sessions.length ?? 0}</dd>
      <dt class="text-fg-muted">Read</dt><dd>{snap ? new Date(snap.taken_at_ms).toLocaleTimeString() : "not yet"}</dd>
    </dl>
    {#if snap?.other_sockets.length}
      {@render section("Other sockets on this host")}
      <ul class="space-y-1">
        {#each snap.other_sockets as name (name)}
          <li class="flex items-center justify-between text-xs"><span class="font-mono">{name}</span><button class="rounded border border-line px-1.5 py-0.5 hover:bg-panel-hover" onclick={() => mon.addSocket(sel.hostId, { kind: "name", value: name })}>Add</button></li>
        {/each}
      </ul>
    {/if}
  {:else if !here.session}
    <p class="p-3 text-center text-xs text-fg-muted">This {sel.kind} no longer exists on the remote host.</p>
  {:else}
    {@const s = here.session}
    {@render section("Session")}
    <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
      <dt class="text-fg-muted">Host</dt><dd>{hostName}</dd>
      <dt class="text-fg-muted">Name</dt><dd class="break-all font-medium">{s.name}</dd>
      <dt class="text-fg-muted">ID</dt><dd class="font-mono">{s.id}</dd>
      <dt class="text-fg-muted">State</dt><dd>{s.attached > 0 ? `Attached (${s.attached} ${s.attached === 1 ? "client" : "clients"})` : "Detached"}</dd>
      <dt class="text-fg-muted">Created</dt><dd>{when(s.created)}</dd>
      {#if s.activity > 0}<dt class="text-fg-muted">Activity</dt><dd>{when(s.activity)}</dd>{/if}
      <dt class="text-fg-muted">Windows</dt><dd>{s.windows.length}</dd>
      <dt class="text-fg-muted">Panes</dt><dd>{paneCount(s)}</dd>
      {#if s.group}<dt class="text-fg-muted">Group</dt><dd>{s.group}</dd>{/if}
    </dl>

    {@render section("Tags")}
    <div class="flex flex-wrap items-center gap-1">
      {#each mon.tagsFor(sel.server, s.name) as t (t)}<span class="rounded bg-accent/15 px-1.5 py-0.5 text-[11px] text-accent">{t}</span>{:else}<span class="text-xs text-fg-muted">No tags</span>{/each}
      <button class="ml-auto flex items-center gap-1 rounded border border-line px-1.5 py-0.5 text-xs hover:bg-panel-hover" onclick={() => mon.act("tags", sel)}><Tag size={11} /> Edit</button>
    </div>

    {@render section("Resources (estimate)")}
    {@render usage(sessionUsage?.usage ?? undefined, sessionUsage?.panes_seen, sessionUsage?.panes_total)}

    {#if sel.kind !== "session"}
      {#if here.window}
        {@const w = here.window}
        {@render section("Window")}
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
          <dt class="text-fg-muted">Name</dt><dd class="break-all">{w.name}</dd>
          <dt class="text-fg-muted">ID</dt><dd class="font-mono">{w.id}</dd>
          <dt class="text-fg-muted">Index</dt><dd>{w.index}</dd>
          <dt class="text-fg-muted">Active</dt><dd>{w.active ? "Yes" : "No"}{w.zoomed ? " · zoomed" : ""}</dd>
          <dt class="text-fg-muted">Panes</dt><dd>{w.panes.length}</dd>
          <dt class="text-fg-muted">Layout</dt><dd class="break-all font-mono text-[11px]">{w.layout || "unknown"}</dd>
        </dl>
      {/if}
      {#if here.pane}
        {@const p = here.pane}
        {@render section("Pane")}
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
          <dt class="text-fg-muted">ID</dt><dd class="font-mono">{p.id}</dd>
          <dt class="text-fg-muted">Title</dt><dd class="break-all">{p.title || "none"}</dd>
          <dt class="text-fg-muted">Command</dt><dd class="break-all font-mono">{p.command || "unknown"}</dd>
          <dt class="text-fg-muted">Path</dt><dd class="break-all font-mono">{p.path || "unknown"}</dd>
          <dt class="text-fg-muted">Size</dt><dd class="tabular-nums">{p.width}×{p.height}</dd>
          <dt class="text-fg-muted">PID</dt><dd class="tabular-nums">{p.pid ?? "unknown"}</dd>
          <dt class="text-fg-muted">Active</dt><dd>{p.active ? "Yes" : "No"}{p.dead ? " · ended" : ""}{p.in_mode ? " · copy mode" : ""}</dd>
        </dl>
        {@render section("Pane resources (estimate)")}
        {@render usage(paneUsage)}
        {#if lastText}
          {@render section("Last captured output")}
          <pre class="max-h-32 overflow-auto whitespace-pre rounded bg-base p-2 font-mono text-[11px] text-fg-muted">{lastText.replace(/\x1b\[[0-9;:]*m/g, "").trimEnd().split("\n").slice(-6).join("\n")}</pre>
        {/if}
      {/if}
    {:else}
      {@render section("Windows")}
      <ul class="space-y-0.5">
        {#each s.windows as w (w.id)}
          <li><button class="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-xs hover:bg-panel-hover" onclick={() => mon.reveal(sel.server, s.id, w.id, w.panes[0]?.id ?? "")}>
            <span class="h-1.5 w-1.5 rounded-full {w.active ? 'bg-accent' : 'bg-fg-muted/30'}"></span><span class="truncate">{w.index}: {w.name}</span><span class="ml-auto text-fg-muted">{w.panes.length}p</span>
          </button></li>
        {/each}
      </ul>
    {/if}

    {#if sel.kind === "session"}
      {@render section("Settings")}
      <div class="rounded-md border border-line p-2 text-xs">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="font-medium">tmux mouse mode</div>
            <div class="text-fg-muted">{s.mouse === null ? "This tmux does not report it." : s.mouse ? "On for this session." : "Off for this session."}</div>
          </div>
          <button class="shrink-0 rounded border border-line px-2 py-1 hover:bg-panel-hover disabled:opacity-50" disabled={s.mouse === null || !!snap?.stale} onclick={() => mon.act(s.mouse ? "mouse-off" : "mouse-on", sel)}>{s.mouse ? "Turn off" : "Turn on…"}</button>
        </div>
        <p class="mt-1.5 text-[11px] text-fg-muted">Changes this session's option only. Key bindings and the tmux config file are never modified.</p>
      </div>
      <div class="mt-4 flex flex-wrap gap-1.5">
        <button class="flex items-center gap-1 rounded-md border border-accent/50 bg-accent/10 px-2 py-1 text-xs text-accent hover:bg-accent/20" onclick={() => mon.attach(sel.server, s.id, "tab")}><SquareTerminal size={12} /> Attach</button>
        <button class="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs hover:bg-panel-hover disabled:opacity-50" disabled={!mon.canSplit} onclick={() => mon.attach(sel.server, s.id, "right")}><Columns2 size={12} /> Split right</button>
        <button class="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs hover:bg-panel-hover disabled:opacity-50" disabled={!mon.canSplit} onclick={() => mon.attach(sel.server, s.id, "down")}><Rows2 size={12} /> Split down</button>
        <button class="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs hover:bg-panel-hover" onclick={() => mon.act("rename-session", sel)}><Pencil size={12} /> Rename</button>
        <button class="flex items-center gap-1 rounded-md border border-danger/50 px-2 py-1 text-xs text-danger hover:bg-danger/10" onclick={() => mon.act("kill-session", sel)}><Trash2 size={12} /> Kill</button>
      </div>
    {/if}
  {/if}
</aside>
