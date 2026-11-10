<script lang="ts">
  import { AppWindow, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Columns2, Layers, Maximize2, PanelsTopLeft, Pencil, Plus, Rows2, Server, Settings2, SquareTerminal, Tag, Trash2 } from "lucide-svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { bytes, locate, paneCount, socketLabel, when } from "$lib/tmux/monitor";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import TmuxResources from "./TmuxResources.svelte";

  // The details of what is selected, and only the actions that apply to it: a session offers session actions, a window window
  // actions, a pane pane actions. Anything that destroys something is set apart and asks first.
  const sel = $derived(mon.selectedRow);
  const snap = $derived(sel ? mon.snapshotOf(sel.server) : undefined);
  const here = $derived(sel ? locate(snap, sel) : {});
  const target = $derived(mon.focusTarget);
  const host = $derived(sel ? vaultStore.hostById.get(sel.hostId)?.data : undefined);
  const hostName = $derived(host?.label || host?.hostname || "host");
  const stale = $derived(!!snap?.stale);

  const row = $derived(
    sel && here.session
      ? { kind: sel.kind, server: sel.server, hostId: sel.hostId, socket: sel.socket, sessionId: here.session.id, windowId: here.window?.id, paneId: here.pane?.id }
      : undefined,
  );
  // A session or window is split through the pane tmux would split: the one active in it.
  const splitRow = $derived(row && target?.pane ? { ...row, kind: "pane" as const, windowId: target.window.id, paneId: target.pane.id } : undefined);

  function resize(dir: "up" | "down" | "left" | "right") {
    if (sel?.paneId) void mon.run(sel.hostId, sel.socket, { op: "resize_pane", pane: sel.paneId, dir, amount: 5 });
  }
  const ago = (unix: number) => {
    if (unix <= 0) return "unknown";
    const s = Math.max(0, Date.now() / 1000 - unix);
    if (s < 90) return "just now";
    if (s < 5400) return `${Math.round(s / 60)} min ago`;
    if (s < 172800) return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} m ago`;
    return `${Math.floor(s / 86400)} days ago`;
  };
  const editHost = () => sel && (ui.modal = { kind: "host", id: sel.hostId } as typeof ui.modal);
  const kindName = $derived(sel?.kind === "pane" ? "Pane" : sel?.kind === "window" ? "Window" : sel?.kind === "session" ? "Session" : "Host");

  const btn = "flex items-center justify-center gap-1.5 rounded-lg border px-2 py-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50";
  const plain = `${btn} border-line bg-base hover:bg-panel-hover`;
  const primary = `${btn} border-accent-solid bg-accent-solid text-white hover:opacity-90`;
  const danger = `${btn} border-danger/50 text-danger hover:bg-danger/10`;
</script>

{#snippet card(title: string, icon: typeof Server)}
  {@const Icon = icon}
  <h3 class="mb-2 flex items-center gap-1.5 text-xs font-semibold"><Icon size={13} class="text-fg-muted" /> {title}</h3>
{/snippet}

<aside class="flex h-full min-w-0 flex-col gap-3 overflow-y-auto bg-panel/30 p-3 text-sm" aria-label="Inspector">
  {#if !sel}
    <p class="p-3 text-center text-xs text-fg-muted">Details of the selected host, session, window or pane appear here.</p>
  {:else if sel.kind === "host"}
    <section class="rounded-xl border border-line bg-panel p-3">
      {@render card("Host: " + hostName, Server)}
      <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs">
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
    </section>
    <section class="rounded-xl border border-line bg-panel p-3">
      {@render card("Actions", Settings2)}
      <div class="grid grid-cols-2 gap-2">
        <button class={primary} onclick={() => mon.act("new-session", sel)}><Plus size={14} /> New Session</button>
        <button class={plain} onclick={() => mon.act("refresh", sel)}>Refresh</button>
        <button class={plain} onclick={() => mon.act("add-socket", sel)}>Add Socket…</button>
        <button class={plain} onclick={() => mon.act("remove-host", sel)} title="Stop showing this host here. Nothing on the host changes.">Remove Host</button>
      </div>
    </section>
    {#if snap?.other_sockets.length}
      <section class="rounded-xl border border-line bg-panel p-3">
        {@render card("Other sockets on this host", Layers)}
        <ul class="space-y-1">
          {#each snap.other_sockets as name (name)}
            <li class="flex items-center justify-between text-xs"><span class="font-mono">{name}</span><button class="rounded border border-line px-1.5 py-0.5 hover:bg-panel-hover" onclick={() => mon.addSocket(sel.hostId, { kind: "name", value: name })}>Add</button></li>
          {/each}
        </ul>
      </section>
    {/if}
    <section class="rounded-xl border border-line bg-panel p-3 text-xs">
      {@render card(`Tmux Settings (Host: ${hostName})`, Settings2)}
      <div class="flex items-center justify-between gap-2"><span>Auto tmux</span><span class="rounded-full px-2 py-0.5 {host?.auto_tmux?.enabled ? 'bg-success/15 text-success' : 'bg-base text-fg-muted'}">{host?.auto_tmux?.enabled ? "On" : "Off"}</span></div>
      <button class="mt-2 w-full rounded-lg border border-line px-2 py-1.5 hover:bg-panel-hover" onclick={editHost}>Change in the host's settings</button>
    </section>
  {:else if !here.session || !target}
    <div class="rounded-xl border border-line bg-panel p-4 text-center text-xs text-fg-muted" role="status">
      <p>This {sel.kind} no longer exists on the remote host.</p>
      <button class="mt-3 rounded-lg border border-line px-3 py-1.5 hover:bg-panel-hover" onclick={() => mon.refresh(sel.hostId, sel.socket)}>Refresh Sessions</button>
    </div>
  {:else}
    {@const s = here.session}
    <section class="rounded-xl border border-line bg-panel p-3">
      <div class="mb-2 flex items-center gap-2">
        <span class="grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-accent/15 text-accent">{#if sel.kind === "pane"}<PanelsTopLeft size={14} />{:else if sel.kind === "window"}<AppWindow size={14} />{:else}<Layers size={14} />{/if}</span>
        <h2 class="min-w-0 flex-1 truncate text-sm font-semibold" title={sel.kind === "session" ? s.name : sel.kind === "window" ? here.window?.name : here.pane?.id}>
          {kindName}: {sel.kind === "session" ? s.name : sel.kind === "window" ? `${here.window?.index}: ${here.window?.name}` : `${here.pane?.id} ${here.pane?.command ?? ""}`}
        </h2>
        <span class="shrink-0 rounded-full px-2 py-0.5 text-[11px] {s.attached > 0 ? 'bg-success/15 text-success' : 'bg-base text-fg-muted'}">{s.attached > 0 ? "Attached" : "Detached"}</span>
      </div>
      <dl class="grid grid-cols-[4.5rem_1fr] gap-x-3 gap-y-1.5 text-xs">
        <dt class="text-fg-muted">Host</dt><dd>{hostName}</dd>
        <dt class="text-fg-muted">Session</dt><dd class="break-all">{s.name} <span class="font-mono text-fg-muted">{s.id}</span></dd>
        <dt class="text-fg-muted">Created</dt><dd title={when(s.created)}>{ago(s.created)}</dd>
        {#if s.activity > 0}<dt class="text-fg-muted">Activity</dt><dd title={when(s.activity)}>{ago(s.activity)}</dd>{/if}
        <dt class="text-fg-muted">Attached</dt><dd>{s.attached > 0 ? `Yes (${s.attached} ${s.attached === 1 ? "client" : "clients"})` : "No"}</dd>
        <dt class="text-fg-muted">Windows</dt><dd>{s.windows.length}</dd>
        <dt class="text-fg-muted">Panes</dt><dd>{paneCount(s)}</dd>
        {#if s.group}<dt class="text-fg-muted">Group</dt><dd>{s.group}</dd>{/if}
        {#if sel.kind === "window" && here.window}
          <dt class="text-fg-muted">Window</dt><dd class="break-all">{here.window.index}: {here.window.name} <span class="font-mono text-fg-muted">{here.window.id}</span>{here.window.zoomed ? " · zoomed" : ""}</dd>
        {/if}
        {#if sel.kind === "pane" && here.pane}
          {@const p = here.pane}
          <dt class="text-fg-muted">Pane</dt><dd class="font-mono">{p.id}{p.dead ? " · ended" : ""}{p.in_mode ? " · copy mode" : ""}</dd>
          <dt class="text-fg-muted">Command</dt><dd class="break-all font-mono">{p.command || "unknown"}</dd>
          <dt class="text-fg-muted">Path</dt><dd class="break-all font-mono">{p.path || "unknown"}</dd>
          <dt class="text-fg-muted">Size</dt><dd class="tabular-nums">{p.width}×{p.height}</dd>
          <dt class="text-fg-muted">PID</dt><dd class="tabular-nums">{p.pid ?? "unknown"}</dd>
        {/if}
        <dt class="text-fg-muted">Tags</dt>
        <dd class="flex flex-wrap items-center gap-1">
          {#each mon.tagsFor(sel.server, s.name) as t (t)}<span class="rounded-full bg-accent/15 px-2 py-0.5 text-[11px] text-accent">{t}</span>{:else}<span class="text-fg-muted">None</span>{/each}
          <button class="grid h-5 w-5 place-items-center rounded-full border border-line hover:bg-panel-hover" aria-label="Edit tags" title="Edit tags" onclick={() => mon.act("tags", sel)}><Tag size={10} /></button>
        </dd>
      </dl>
    </section>

    <section class="rounded-xl border border-line bg-panel p-3">
      {@render card("Resource Usage (session)", Server)}
      <TmuxResources server={sel.server} session={s} compact onall={() => (mon.tab = "resources")} />
    </section>

    {#if row}
      <section class="rounded-xl border border-line bg-panel p-3">
        {@render card("Actions", Settings2)}
        <div class="grid grid-cols-2 gap-2">
          <button class={primary} title="Open this session in a terminal tab (it keeps running when the tab closes)" onclick={() => mon.attach(sel.server, s.id, "tab")}><SquareTerminal size={14} /> Attach</button>
          {#if sel.kind === "session"}
            <button class={plain} disabled={stale} onclick={() => mon.act("new-window", row)}><AppWindow size={14} /> New Window</button>
          {:else if sel.kind === "window"}
            <button class={plain} disabled={stale || here.window?.active} title="Make this the window tmux shows in the session" onclick={() => mon.act("select-window", row)}>Make Active</button>
          {:else}
            <button class={plain} disabled={stale || here.pane?.active} title="Make this the active pane of its window" onclick={() => mon.act("select-pane", row)}>Make Active</button>
          {/if}
          <button class={plain} disabled={stale || !splitRow} title="Split the {sel.kind === 'pane' ? '' : 'active '}pane in tmux, side by side" onclick={() => splitRow && mon.act("split-right", splitRow)}><Columns2 size={14} /> Split Right</button>
          <button class={plain} disabled={stale || !splitRow} title="Split the {sel.kind === 'pane' ? '' : 'active '}pane in tmux, one above the other" onclick={() => splitRow && mon.act("split-down", splitRow)}><Rows2 size={14} /> Split Down</button>
          {#if sel.kind === "session"}
            <button class={plain} disabled={stale} onclick={() => mon.act("rename-session", row)}><Pencil size={14} /> Rename</button>
          {:else if sel.kind === "window"}
            <button class={plain} disabled={stale} onclick={() => mon.act("rename-window", row)}><Pencil size={14} /> Rename</button>
          {:else}
            <button class={plain} disabled={stale} onclick={() => mon.act("title-pane", row)}><Pencil size={14} /> Set Title</button>
          {/if}
          <button class={plain} disabled={!mon.canSplit} title="Open this session in a terminal split to the right" onclick={() => mon.attach(sel.server, s.id, "right")}><Columns2 size={14} /> Terminal Right</button>
          <button class={plain} disabled={!mon.canSplit} title="Open this session in a terminal split below" onclick={() => mon.attach(sel.server, s.id, "down")}><Rows2 size={14} /> Terminal Below</button>
          {#if sel.kind === "pane"}
            <button class={plain} disabled={stale || (here.window?.panes.length ?? 0) < 2} onclick={() => mon.act("zoom-pane", row)}><Maximize2 size={14} /> {here.window?.zoomed ? "Unzoom" : "Zoom"}</button>
          {/if}
        </div>
        <div class="mt-3 border-t border-line pt-3">
          <h4 class="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Danger zone</h4>
          {#if sel.kind === "session"}
            <button class="{danger} w-full" disabled={stale} onclick={() => mon.act("kill-session", row)}><Trash2 size={14} /> Kill Session…</button>
          {:else if sel.kind === "window"}
            <button class="{danger} w-full" disabled={stale} onclick={() => mon.act("kill-window", row)}><Trash2 size={14} /> Kill Window…</button>
          {:else}
            <button class="{danger} w-full" disabled={stale} onclick={() => mon.act("kill-pane", row)}><Trash2 size={14} /> Kill Pane…</button>
          {/if}
        </div>
      </section>
    {/if}

    {#if sel.kind === "pane" && here.pane}
      <section class="rounded-xl border border-line bg-panel p-3">
        {@render card("Resize", Maximize2)}
        <div class="flex items-center gap-3">
          <div class="grid grid-cols-3 gap-1 text-fg-muted" role="group" aria-label="Move this pane's borders">
            <span></span>
            <button class="grid h-7 w-7 place-items-center rounded border border-line hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Move the border up by 5 rows" aria-label="Resize up" disabled={stale} onclick={() => resize("up")}><ArrowUp size={12} /></button>
            <span></span>
            <button class="grid h-7 w-7 place-items-center rounded border border-line hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Move the border left by 5 columns" aria-label="Resize left" disabled={stale} onclick={() => resize("left")}><ArrowLeft size={12} /></button>
            <span></span>
            <button class="grid h-7 w-7 place-items-center rounded border border-line hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Move the border right by 5 columns" aria-label="Resize right" disabled={stale} onclick={() => resize("right")}><ArrowRight size={12} /></button>
            <span></span>
            <button class="grid h-7 w-7 place-items-center rounded border border-line hover:bg-panel-hover hover:text-fg disabled:opacity-50" title="Move the border down by 5 rows" aria-label="Resize down" disabled={stale} onclick={() => resize("down")}><ArrowDown size={12} /></button>
            <span></span>
          </div>
          <p class="text-[11px] text-fg-muted">Moves the pane's border by 5 cells, as tmux's resize-pane does.</p>
        </div>
      </section>
    {/if}

    <section class="rounded-xl border border-line bg-panel p-3 text-xs">
      {@render card(`Tmux Settings (Host: ${hostName})`, Settings2)}
      <div class="flex items-center justify-between gap-2 py-1"><span>Auto tmux</span><span class="rounded-full px-2 py-0.5 {host?.auto_tmux?.enabled ? 'bg-success/15 text-success' : 'bg-base text-fg-muted'}">{host?.auto_tmux?.enabled ? "On" : "Off"}</span></div>
      <div class="flex items-center justify-between gap-2 py-1">
        <div>
          <div>Mouse support</div>
          <div class="text-[11px] text-fg-muted">{s.mouse === null ? "This tmux does not report it." : s.mouse ? "On for this session." : "Off for this session."} Key bindings and the config file are never changed.</div>
        </div>
        <button class="shrink-0 rounded-lg border border-line px-2 py-1 hover:bg-panel-hover disabled:opacity-50" disabled={s.mouse === null || stale} onclick={() => mon.act(s.mouse ? "mouse-off" : "mouse-on", row ?? sel)}>{s.mouse ? "Turn off" : "Turn on…"}</button>
      </div>
      <button class="mt-1 w-full rounded-lg border border-line px-2 py-1.5 hover:bg-panel-hover" onclick={editHost}>Auto tmux settings for this host</button>
    </section>
  {/if}
</aside>
