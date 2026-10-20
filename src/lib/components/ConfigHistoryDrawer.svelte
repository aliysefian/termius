<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Columns2, History, Loader2, RotateCcw, Rows2, Search, Terminal, Trash2, X } from "lucide-svelte";
  import ConfigDiff from "./ConfigDiff.svelte";
  import * as api from "$lib/api";
  import type { ConfigSnapshot, TrackedConfigFile } from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { diffLines } from "$lib/diff";
  import { SecuritySession } from "$lib/security/securitySession.svelte";
  import { parseRead, readScript, reloadCommand, writeScript, type LiveFile } from "$lib/confighistory/remoteFile";
  import { formatBytes, snapshotVault } from "$lib/confighistory/snapshotVaultStore.svelte";
  import { maxBytes, sshUserFor } from "$lib/confighistory/useConfigTimeMachine.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { writeToPane } from "$lib/terminalio";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  const open = $derived(ui.configHistory);
  const session = new SecuritySession();

  let hostFilter = $state<Uuid | "">("");
  let query = $state("");
  let selected = $state<TrackedConfigFile | null>(null);
  let versions = $state<ConfigSnapshot[]>([]);
  /** Oldest to newest, which is the direction the slider runs. */
  const timeline = $derived([...versions].reverse());
  let index = $state(0);
  let text = $state<string | null>(null);
  let live = $state<LiveFile | null>(null);
  let liveError = $state<string | null>(null);
  let layout = $state<"split" | "unified">("split");
  let busy = $state<string | null>(null);
  let banner = $state<{ text: string; command?: string } | null>(null);
  let sudoPassword = $state("");

  const snap = $derived(timeline[index]);
  const hostLabel = (id: Uuid) => vaultStore.hostById.get(id)?.data?.label ?? "Removed host";
  const production = $derived(selected ? envInfo(vaultStore.effectiveEnv(vaultStore.hostById.get(selected.host_id)?.data)).value === "production" : false);

  const shown = $derived(
    snapshotVault.files.filter((f) => (!hostFilter || f.host_id === hostFilter) && (!query || f.file_path.toLowerCase().includes(query.toLowerCase()))),
  );
  /** Host, then folder, then file. */
  const groups = $derived.by(() => {
    const byHost = new Map<Uuid, Map<string, TrackedConfigFile[]>>();
    for (const f of shown) {
      const dir = f.file_path.slice(0, f.file_path.lastIndexOf("/")) || "/";
      const hosts = byHost.get(f.host_id) ?? new Map<string, TrackedConfigFile[]>();
      hosts.set(dir, [...(hosts.get(dir) ?? []), f]);
      byHost.set(f.host_id, hosts);
    }
    return [...byHost.entries()].map(([host, dirs]) => ({ host, dirs: [...dirs.entries()].sort(([a], [b]) => a.localeCompare(b)) })).sort((a, b) => hostLabel(a.host).localeCompare(hostLabel(b.host)));
  });
  const hostsWithFiles = $derived([...new Set(snapshotVault.files.map((f) => f.host_id))]);

  const delta = $derived.by(() => {
    if (text === null || !live?.exists) return null;
    const d = diffLines(text, live.text);
    return { added: d.added, removed: d.removed };
  });

  const stamp = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  const via = (t: string) => t.replace(/^Terminal \((.*)\)$/, "$1").replace("Manual Pre-Rollback", "pre-rollback");
  const SHORT = 8;

  onMount(async () => {
    await snapshotVault.refresh();
    if (open) {
      hostFilter = open.hostId;
      const f = snapshotVault.files.find((x) => x.host_id === open.hostId && (!open.path || x.file_path === open.path));
      if (f) await pick(f);
    }
  });
  onDestroy(() => void session.close());

  async function pick(f: TrackedConfigFile) {
    selected = f;
    banner = null;
    live = null;
    liveError = null;
    versions = await api.configHistory.versions(f.host_id, f.file_path).catch(() => []);
    index = Math.max(0, versions.length - 1);
    void loadVersion();
    void loadLive();
  }

  async function loadVersion() {
    if (!selected || !snap) return;
    text = null;
    try {
      text = await api.configHistory.get(selected.host_id, selected.file_path, snap.id);
    } catch (e) {
      text = null;
      liveError = errorMessage(e);
    }
  }

  async function connect() {
    if (!selected) return false;
    if (session.connected && session.hostId === selected.host_id) return true;
    await session.open(selected.host_id);
    return session.connected;
  }

  /** The file as it is on the host now (right side of the diff). */
  async function loadLive() {
    if (!selected) return;
    liveError = null;
    if (!(await connect())) {
      liveError = session.error ?? "Could not connect to read the live file.";
      return;
    }
    try {
      const r = await session.run(readScript(selected.file_path), { mode: "auto" });
      live = parseRead(r.stdout);
    } catch (e) {
      liveError = errorMessage(e);
    }
  }

  $effect(() => {
    void index;
    if (selected && timeline.length) void loadVersion();
  });

  async function restore() {
    if (!selected || !snap || text === null) return;
    const host = vaultStore.hostById.get(selected.host_id)?.data;
    const ok = await ask(`Put version ${snap.version} of ${selected.file_path} back on ${host?.label ?? "the host"}? The file as it is now is saved first as a Pre-Rollback Backup.`, {
      title: production ? "Production host" : "Restore this version",
      confirm: "Restore",
      danger: production,
      requireText: production ? host?.label : undefined,
    });
    if (!ok) return;
    busy = "Restoring";
    banner = null;
    try {
      if (!(await connect())) throw new Error(session.error ?? "Not connected.");
      // 1. Whatever is there now, so a rollback is never a one-way door.
      const now = parseRead((await session.run(readScript(selected.file_path), { mode: "auto" })).stdout);
      if (now.exists) {
        const kept = await api.configHistory.store(selected.host_id, selected.file_path, now.text, now.permissions, now.owner, "Manual Pre-Rollback", sshUserFor(selected.host_id), Math.max(maxBytes(settings.prefs), now.text.length));
        if (kept.status === "failed") throw new Error(`The current file could not be saved first: ${kept.reason}`);
      }
      // 2. Write the old text back, with the mode and owner it had.
      const r = await session.run(writeScript(selected.file_path, text, snap.permissions || now.permissions, snap.owner || now.owner), { mode: "auto", timeoutSecs: 30 });
      if (r.code !== 0) throw new Error((r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.code}`);
      const name = selected.file_path.slice(selected.file_path.lastIndexOf("/") + 1);
      banner = { text: `Restored ${name} to version ${snap.version}.`, command:
        // The terminal runs as the login user, so the reload takes sudo unless that user is root.
        reloadCommand(selected.file_path, session.me?.uid === 0) ?? undefined };
      await snapshotVault.refresh();
      const f = snapshotVault.files.find((x) => x.host_id === selected!.host_id && x.file_path === selected!.file_path);
      if (f) await pick(f);
    } catch (e) {
      banner = { text: `Not restored: ${errorMessage(e)}` };
    } finally {
      busy = null;
    }
  }

  /** Type the reload into a terminal on this host (or open one). */
  async function runInTerminal(command: string) {
    if (!selected) return;
    const hostId = selected.host_id;
    for (const t of ui.tabs) {
      const pane = t.panes.find((p) => p.target.kind === "host" && p.target.hostId === hostId && ui.paneInfo[p.id]?.status === "connected");
      if (pane) {
        ui.activeTabId = t.id;
        t.activePaneId = pane.id;
        await writeToPane(pane, command + "\r");
        ui.configHistory = null;
        return;
      }
    }
    ui.openTerminal(hostId, hostLabel(hostId), command);
    ui.configHistory = null;
  }

  async function forget() {
    if (!selected) return;
    if (!(await ask(`Delete all ${selected.versions} saved versions of ${selected.file_path}? The file on the host is not touched.`, { title: "Forget this file", confirm: "Delete history", danger: true }))) return;
    await snapshotVault.deleteFile(selected.host_id, selected.file_path);
    selected = null;
    versions = [];
    text = null;
  }

  function submitPassword(e: SubmitEvent) {
    e.preventDefault();
    const p = sudoPassword;
    sudoPassword = "";
    session.answerPassword(p);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !session.passwordPrompt && (ui.configHistory = null)} />

<div class="fixed inset-0 z-50 flex justify-end bg-black/50" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (ui.configHistory = null)}>
  <div class="flex h-full w-[85vw] max-w-5xl flex-col border-l border-line bg-base" role="dialog" aria-modal="true" aria-label="Config Time-Machine" data-testid="config-history">
    <header class="flex h-10 shrink-0 items-center gap-2 border-b border-line px-4">
      <History size={15} class="text-accent" />
      <h2 class="text-sm font-semibold">Config Time-Machine</h2>
      <span class="font-mono text-xs tabular-nums text-fg-muted">{snapshotVault.usage.snapshots} snapshots · {formatBytes(snapshotVault.usage.bytes)}</span>
      <div class="flex-1"></div>
      <button class="icon-btn h-7 w-7" aria-label="Close" onclick={() => (ui.configHistory = null)}><X size={15} /></button>
    </header>

    <div class="flex min-h-0 flex-1">
      <!-- Tracked files -->
      <nav class="flex w-72 shrink-0 flex-col border-r border-line" aria-label="Tracked files">
        <div class="space-y-2 border-b border-line p-2">
          <select class="input h-8 py-0 text-xs" bind:value={hostFilter} aria-label="Host">
            <option value="">All hosts</option>
            {#each hostsWithFiles as h (h)}<option value={h}>{hostLabel(h)}</option>{/each}
          </select>
          <div class="relative"><Search size={12} class="pointer-events-none absolute left-2 top-2.5 text-fg-muted" /><input class="input h-8 py-0 pl-7 text-xs" placeholder="Filter tracked configs…" bind:value={query} /></div>
        </div>
        <div class="min-h-0 flex-1 overflow-y-auto">
          {#each groups as g (g.host)}
            <div class="sticky top-0 border-b border-line bg-panel px-3 py-1 text-xs font-medium">{hostLabel(g.host)}</div>
            {#each g.dirs as [dir, files] (dir)}
              <div class="truncate px-3 pt-1.5 font-mono text-[11px] text-fg-muted" title={dir}>{dir}</div>
              {#each files as f (f.file_path)}
                <button class="flex w-full items-center gap-2 px-3 py-1 text-left transition-colors hover:bg-panel-hover/50 {selected?.file_path === f.file_path && selected.host_id === f.host_id ? 'bg-panel-hover' : ''}" onclick={() => void pick(f)}>
                  <span class="min-w-0 flex-1 truncate font-mono text-xs" title={f.file_path}>{f.file_path.slice(f.file_path.lastIndexOf("/") + 1)}</span>
                  <span class="shrink-0 rounded-sm border border-line px-1.5 font-mono text-[10px] tabular-nums text-fg-muted">{f.versions} {f.versions === 1 ? "version" : "versions"}</span>
                </button>
              {/each}
            {/each}
          {:else}
            <p class="p-4 text-xs text-fg-muted">{snapshotVault.files.length ? "Nothing matches." : "Nothing has been kept yet. Turn the Time-Machine on in Settings → Safety, then edit a file in /etc."}</p>
          {/each}
        </div>
      </nav>

      <!-- The file -->
      <section class="flex min-w-0 flex-1 flex-col">
        {#if !selected}
          <div class="flex flex-1 items-center justify-center text-xs text-fg-muted">Choose a file on the left.</div>
        {:else}
          <div class="shrink-0 space-y-2 border-b border-line p-3">
            <div class="flex items-center gap-2">
              <span class="min-w-0 flex-1 truncate font-mono text-xs tabular-nums" title={selected.file_path}>{hostLabel(selected.host_id)}:{selected.file_path}</span>
              <button class="icon-btn h-7 w-7 hover:text-danger" title="Forget this file's history" aria-label="Forget this file's history" onclick={forget}><Trash2 size={13} /></button>
            </div>

            {#if timeline.length > 0}
              <!-- The scrubber: oldest on the left, newest on the right -->
              <input type="range" class="accent-input w-full" min="0" max={Math.max(0, timeline.length - 1)} step="1" bind:value={index} aria-label="Version" aria-valuetext="version {snap?.version}, {snap ? stamp(snap.captured_at) : ''}" />
              <div class="flex gap-1 overflow-x-auto pb-1" role="listbox" aria-label="Versions">
                {#each timeline as v, i (v.id)}
                  <button role="option" aria-selected={i === index} class="shrink-0 rounded-sm border px-2 py-0.5 font-mono text-[11px] tabular-nums transition-colors {i === index ? 'border-accent/60 bg-accent/10 text-fg' : 'border-line text-fg-muted hover:text-fg'}" onclick={() => (index = i)}>
                    v{v.version} · {stamp(v.captured_at)} — via {via(v.trigger_source)}
                  </button>
                {/each}
              </div>
              {#if snap}
                <div class="flex flex-wrap gap-1.5 font-mono text-xs tabular-nums">
                  {#each [["Timestamp", stamp(snap.captured_at)], ["SHA-256", snap.sha256.slice(0, SHORT)], ["Trigger", via(snap.trigger_source)], ["Size", formatBytes(snap.size_bytes)], ["Mode", snap.permissions || "—"], ["Owner", snap.owner || "—"], ["User", snap.ssh_user || "—"]] as [k, val] (k)}
                    <span class="rounded-sm border border-line px-1.5 py-0.5"><span class="text-fg-muted">{k}</span> {val}</span>
                  {/each}
                  {#if delta}
                    <span class="rounded-sm border border-line px-1.5 py-0.5" title="Lines that differ between this version and the file on the host now"><span class="text-fg-muted">Line Δ</span> <span class="text-success">+{delta.added}</span> / <span class="text-danger">−{delta.removed}</span></span>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>

          {#if session.passwordPrompt}
            <form class="flex shrink-0 flex-wrap items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2 text-xs text-warning" role="alert" onsubmit={submitPassword}>
              <label for="ch-sudo">{session.passwordPrompt.reason ?? "sudo needs your password on the host"}</label>
              <input id="ch-sudo" class="input max-w-56 py-1 text-xs" type="password" autocomplete="off" bind:value={sudoPassword} required />
              <button class="btn-secondary px-2 py-1 text-xs" type="button" onclick={() => { sudoPassword = ""; session.answerPassword(null); }}>Cancel</button>
              <button class="btn-primary px-2 py-1 text-xs" type="submit">Continue</button>
            </form>
          {/if}
          {#if banner}
            <div class="flex shrink-0 flex-wrap items-center gap-3 border-b border-line bg-panel/60 px-4 py-2 text-xs" role="status" data-testid="restore-banner">
              <span class="min-w-0 flex-1">
                {banner.text}
                {#if banner.command}Run <code class="font-mono tabular-nums">{banner.command}</code> in terminal?{/if}
              </span>
              {#if banner.command}
                <button class="btn-primary px-2 py-1 text-xs" onclick={() => banner?.command && void runInTerminal(banner.command)}><Terminal size={12} /> Run in terminal</button>
              {/if}
              <button class="icon-btn h-6 w-6" aria-label="Dismiss" onclick={() => (banner = null)}><X size={12} /></button>
            </div>
          {/if}

          <!-- Diff: the kept version against the file on the host now -->
          <div class="flex shrink-0 items-center gap-2 border-b border-line px-3 py-1.5 text-xs">
            <span class="text-fg-muted">Selected version <span class="text-fg">(left)</span> against the live file <span class="text-fg">(right)</span></span>
            <div class="flex-1"></div>
            <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label="Diff layout">
              <button class="px-2 py-1 {layout === 'split' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" aria-pressed={layout === "split"} onclick={() => (layout = "split")}><Columns2 size={12} class="inline" /> Split Diff</button>
              <button class="px-2 py-1 {layout === 'unified' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" aria-pressed={layout === "unified"} onclick={() => (layout = "unified")}><Rows2 size={12} class="inline" /> Unified Diff</button>
            </div>
            <button class="btn-primary px-3 py-1 text-xs" disabled={!snap || text === null || !!busy || !!session.connecting} onclick={restore}>
              {#if busy}<Loader2 size={12} class="animate-spin" />{:else}<RotateCcw size={12} />{/if} Restore This Version
            </button>
          </div>
          <div class="min-h-0 flex-1">
            {#if text === null}
              <p class="p-4 text-xs text-fg-muted">Loading the version…</p>
            {:else if liveError}
              <p class="p-4 text-xs text-danger" role="alert">{liveError}</p>
            {:else if !live}
              <p class="flex items-center gap-2 p-4 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> Reading the file on the host…</p>
            {:else}
              <ConfigDiff before={text} after={live.exists ? live.text : ""} {layout} leftLabel="v{snap?.version} · {snap ? stamp(snap.captured_at) : ''}" rightLabel={live.exists ? "On the server now" : "Missing on the server"} />
            {/if}
          </div>
        {/if}
      </section>
    </div>
  </div>
</div>
