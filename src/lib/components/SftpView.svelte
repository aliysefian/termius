<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { CircleStop, FilePen, HardDrive, Loader2, Monitor, Server, Unplug, X } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import FilePane from "./FilePane.svelte";
  import { local, remote, sftp, type Direction, type FileSource } from "$lib/sftp";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, formatBytes, type FileEntry, type TransferProgress, type Uuid } from "$lib/types";

  interface Transfer {
    id: string;
    direction: Direction;
    label: string;
    progress: TransferProgress;
  }

  // One remote session per SftpView; the id is stable for its lifetime.
  const sessionId = `sftp-${crypto.randomUUID()}`;

  let localPath = $state("");
  let remotePath = $state("");
  let localRefresh = $state(0);
  let remoteRefresh = $state(0);

  let hostId = $state<Uuid | "">("");
  let connectedHost = $state<Uuid | null>(null);
  let connecting = $state(false);
  let connectError = $state<string | null>(null);
  let askCreds = $state(false);
  let username = $state("");
  let password = $state("");

  let transfers = $state<Transfer[]>([]);

  const remoteSource = $derived<FileSource | null>(connectedHost ? remote(sessionId) : null);
  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const connectedLabel = $derived(connectedHost ? vaultStore.hostById.get(connectedHost)?.data?.label : "");

  onMount(async () => {
    localPath = await local.home();
  });
  // -- remote editing -------------------------------------------------------

  interface Editing {
    id: string;
    name: string;
    remote: string;
    uploads: number;
    lastUpload?: Date;
    error?: string;
  }
  let edits = $state<Editing[]>([]);
  let unlistenEdit: UnlistenFn | null = null;

  onMount(async () => {
    unlistenEdit = await api.remoteEdit.onEvent((e) => {
      const ed = edits.find((x) => x.id === e.edit_id);
      if (!ed) return;
      if (e.state === "uploaded") {
        ed.uploads++;
        ed.lastUpload = new Date();
        ed.error = undefined;
        remoteRefresh++;
      } else {
        ed.error = e.message;
      }
    });
  });

  async function edit(entry: FileEntry) {
    const already = edits.find((e) => e.remote === entry.path);
    if (already) {
      ui.notify("info", `${entry.name} is already open for editing.`);
      return;
    }
    try {
      const started = await api.remoteEdit.start(sessionId, entry.path);
      edits.push({ id: started.edit_id, name: entry.name, remote: entry.path, uploads: 0 });
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function stopEdit(id: string) {
    await api.remoteEdit.stop(id);
    edits = edits.filter((e) => e.id !== id);
  }

  async function stopAllEdits() {
    await Promise.all(edits.map((e) => api.remoteEdit.stop(e.id)));
    edits = [];
  }

  onDestroy(() => {
    unlistenEdit?.();
    void stopAllEdits();
    if (connectedHost) void sftp.close(sessionId);
  });

  async function connect(creds: { username: string; password: string } | null = null) {
    if (!hostId) return;
    if (!creds && host && !vaultStore.effectiveIdentity(host)) {
      askCreds = true;
      return;
    }
    connecting = true;
    connectError = null;
    askCreds = false;
    try {
      const res = await sftp.open(sessionId, hostId, creds);
      connectedHost = hostId;
      remotePath = res.home;
      remoteRefresh++;
      for (const k of res.new_host_keys) ui.notify("info", `New host key recorded for ${k.host}: ${k.fingerprint}`);
    } catch (e) {
      connectError = errorMessage(e);
    } finally {
      connecting = false;
      password = "";
    }
  }

  // "Browse here" from a terminal pane: connect to that host, then go there.
  let handledRequest = 0;
  $effect(() => {
    const r = ui.sftpRequest;
    if (!r || r.n === handledRequest) return;
    handledRequest = r.n;
    void (async () => {
      if (connectedHost !== r.hostId) {
        if (connectedHost) await disconnect();
        hostId = r.hostId;
        await connect();
      }
      if (connectedHost === r.hostId) {
        remotePath = r.path;
        remoteRefresh++;
      }
    })();
  });

  // Files dragged in from the OS file manager upload to the remote folder.
  let osDrop = $state(false);
  $effect(() => {
    let off: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
      off = await getCurrentWebview().onDragDropEvent((ev) => {
        if (ui.view !== "sftp") return;
        const p = ev.payload;
        if (p.type === "enter" || p.type === "over") osDrop = !!connectedHost;
        else if (p.type === "leave") osDrop = false;
        else if (p.type === "drop") {
          osDrop = false;
          if (connectedHost && p.paths.length) void start("upload", p.paths);
          else if (p.paths.length) ui.notify("error", "Connect to a host first, then drop files to upload them.");
        }
      });
    });
    return () => off?.();
  });

  async function disconnect() {
    await stopAllEdits();
    await sftp.close(sessionId);
    connectedHost = null;
    remotePath = "";
  }

  function baseName(p: string) {
    return p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? p;
  }

  async function start(direction: Direction, sources: string[]) {
    if (!connectedHost || sources.length === 0) return;
    const destDir = direction === "upload" ? remotePath : localPath;
    const id = crypto.randomUUID();
    const label = sources.length === 1 ? baseName(sources[0]) : `${sources.length} items`;
    transfers.unshift({ id, direction, label, progress: { state: "started", total_bytes: 0, total_files: 0 } });
    try {
      await sftp.transfer(sessionId, id, direction, sources, destDir, (p) => {
        const t = transfers.find((x) => x.id === id);
        if (!t) return;
        t.progress = p;
        if (p.state === "done") {
          if (direction === "upload") remoteRefresh++;
          else localRefresh++;
        }
      });
    } catch (e) {
      const t = transfers.find((x) => x.id === id);
      if (t) t.progress = { state: "failed", message: errorMessage(e) };
    }
  }

  function pct(p: TransferProgress) {
    if (p.state === "done") return 100;
    if (p.state === "progress" && p.total_bytes > 0) return Math.min(100, (p.bytes / p.total_bytes) * 100);
    return 0;
  }

  const active = (p: TransferProgress) => p.state === "started" || p.state === "progress";
</script>

<div class="relative flex min-w-0 flex-1 flex-col">
  {#if osDrop}
    <div class="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-dashed border-accent bg-accent/10 text-sm font-medium text-accent">
      Drop to upload to {remotePath || "the remote folder"} on {connectedLabel}
    </div>
  {/if}
  <div class="flex min-h-0 flex-1 divide-x divide-line">
    <FilePane
      side="local"
      source={local}
      bind:path={localPath}
      refreshKey={localRefresh}
      transferLabel="Upload →"
      onTransfer={(paths) => start("upload", paths)}
      onDropFrom={(paths) => start("download", paths)}
    >
      {#snippet header()}
        <Monitor size={14} class="text-accent" />
        <span class="text-sm font-medium">This computer</span>
      {/snippet}
    </FilePane>

    <FilePane
      side="remote"
      source={remoteSource}
      bind:path={remotePath}
      refreshKey={remoteRefresh}
      transferLabel="← Download"
      onTransfer={(paths) => start("download", paths)}
      onDropFrom={(paths) => start("upload", paths)}
      onEdit={edit}
    >
      {#snippet header()}
        <Server size={14} class="text-accent" />
        {#if connectedHost}
          <span class="flex-1 truncate text-sm font-medium">{connectedLabel}</span>
          <button class="btn-ghost py-1 text-xs" onclick={disconnect}><Unplug size={12} /> Disconnect</button>
        {:else}
          <select class="input flex-1 py-1 text-sm" bind:value={hostId} aria-label="Remote host">
            <option value="">Choose a host…</option>
            {#each vaultStore.hosts as h (h.id)}
              <option value={h.id}>{h.data?.label} ({h.data?.hostname})</option>
            {/each}
          </select>
          <button class="btn-primary py-1 text-xs" disabled={!hostId || connecting} onclick={() => connect()}>
            {#if connecting}<Loader2 size={12} class="animate-spin" />{/if} Connect
          </button>
        {/if}
      {/snippet}
      {#snippet placeholder()}
        <div class="max-w-xs">
          {#if askCreds}
            <form
              class="space-y-3 rounded-xl border border-line bg-panel p-5 text-left"
              onsubmit={(e) => {
                e.preventDefault();
                connect({ username, password });
              }}
            >
              <div class="text-sm font-semibold">Credentials for {host?.label}</div>
              <input class="input font-mono" placeholder="username" bind:value={username} required />
              <input class="input" type="password" placeholder="password" bind:value={password} required />
              <button class="btn-primary w-full" type="submit">Connect</button>
            </form>
          {:else}
            <HardDrive size={28} class="mx-auto mb-3 text-fg-muted/50" />
            <p class="text-sm text-fg-muted">Choose a host above to browse its files over SFTP.</p>
            {#if connectError}
              <p class="mt-3 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{connectError}</p>
            {/if}
          {/if}
        </div>
      {/snippet}
    </FilePane>
  </div>

  {#if edits.length}
    <div class="border-t border-line bg-panel">
      <div class="flex items-center justify-between px-3 py-1.5 text-xs text-fg-muted">
        <span class="font-medium uppercase tracking-wide">Editing</span>
        <span>Saves in your editor upload automatically. Stop to delete the local copy.</span>
      </div>
      {#each edits as ed (ed.id)}
        <div class="flex items-center gap-3 px-3 py-1.5 text-xs">
          <FilePen size={13} class="shrink-0 text-accent" />
          <span class="min-w-0 flex-1 truncate font-mono" title={ed.remote}>{ed.remote}</span>
          {#if ed.error}
            <span class="truncate text-danger" title={ed.error}>Upload failed</span>
          {:else if ed.lastUpload}
            <span class="text-success">Uploaded {ed.lastUpload.toLocaleTimeString()}</span>
          {:else}
            <span class="text-fg-muted">Open in editor</span>
          {/if}
          <button class="btn-ghost py-0.5 text-xs" onclick={() => stopEdit(ed.id)}>Stop</button>
        </div>
      {/each}
    </div>
  {/if}

  {#if transfers.length}
    <div class="max-h-48 overflow-y-auto border-t border-line bg-panel">
      <div class="flex items-center justify-between px-3 py-1.5 text-xs text-fg-muted">
        <span class="font-medium uppercase tracking-wide">Transfers</span>
        <button class="hover:text-fg" onclick={() => (transfers = transfers.filter((t) => active(t.progress)))}>Clear finished</button>
      </div>
      {#each transfers as t (t.id)}
        <div class="flex items-center gap-3 px-3 py-1.5 text-xs">
          <span class="w-4 text-fg-muted">{t.direction === "upload" ? "↑" : "↓"}</span>
          <div class="min-w-0 flex-1">
            <div class="flex justify-between gap-2">
              <span class="truncate">{t.label}</span>
              <span class="shrink-0 text-fg-muted">
                {#if t.progress.state === "progress"}
                  {formatBytes(t.progress.bytes)} / {formatBytes(t.progress.total_bytes)} · {t.progress.files_done}/{t.progress.total_files} files
                {:else if t.progress.state === "done"}
                  <span class="text-success">Done · {formatBytes(t.progress.bytes)}</span>
                {:else if t.progress.state === "failed"}
                  <span class="text-danger" title={t.progress.message}>Failed</span>
                {:else if t.progress.state === "cancelled"}
                  Cancelled
                {:else}
                  Scanning…
                {/if}
              </span>
            </div>
            <div class="mt-1 h-1 overflow-hidden rounded bg-base">
              <div
                class="h-full transition-[width] {t.progress.state === 'failed' ? 'bg-danger' : t.progress.state === 'done' ? 'bg-success' : 'bg-accent'}"
                style:width="{pct(t.progress)}%"
              ></div>
            </div>
            {#if t.progress.state === "failed"}
              <div class="mt-0.5 truncate text-danger" title={t.progress.message}>{t.progress.message}</div>
            {/if}
          </div>
          {#if active(t.progress)}
            <button class="icon-btn h-6 w-6 hover:text-danger" title="Cancel" onclick={() => sftp.cancel(t.id)}><CircleStop size={13} /></button>
          {:else}
            <button class="icon-btn h-6 w-6" title="Dismiss" onclick={() => (transfers = transfers.filter((x) => x.id !== t.id))}><X size={13} /></button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
