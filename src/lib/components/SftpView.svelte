<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import { onDestroy, onMount } from "svelte";
  import { getCurrentWindow, ProgressBarStatus } from "@tauri-apps/api/window";
  import { CircleStop, FilePen, HardDrive, Loader2, Monitor, Pause, Play, RotateCcw, Server, Unplug, X } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import FilePane from "./FilePane.svelte";
  import { local, LOCAL_ID, remote, sftp, type Conflict, type Direction, type FileSource } from "$lib/sftp";
  import { choose } from "$lib/dialogs.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { errorMessage, formatBytes, type FileEntry, type TransferProgress, type Uuid } from "$lib/types";

  interface Transfer {
    id: string;
    direction: Direction;
    label: string;
    progress: TransferProgress;
    sources: string[];
    destDir: string;
    /** Continue partial files instead of starting over (retries). */
    resume: boolean;
    /** What to do with names already at the destination. */
    conflict: Conflict;
  }

  /** Transfers running at once; the rest wait their turn. */
  const MAX_RUNNING = 2;

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
  let paneRow = $state<HTMLDivElement>();

  function startSplitDrag(e: PointerEvent) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    el.focus();
    const box = paneRow!.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      const ratio = (ev.clientX - box.left) / box.width;
      settings.prefs.sftpSplitRatio = Math.min(0.85, Math.max(0.15, ratio));
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  // Taskbar/dock progress across every transfer, so the queue's state is
  // visible without switching back to this window.
  $effect(() => {
    let done = 0;
    let total = 0;
    let activeCount = 0;
    for (const t of transfers) {
      const p = t.progress;
      if (p.state === "progress" || p.state === "paused") {
        activeCount++;
        done += p.bytes;
        total += p.total_bytes;
      } else if (p.state === "started") {
        activeCount++;
        total += p.total_bytes;
      }
    }
    if (activeCount === 0) {
      void getCurrentWindow().setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
      return;
    }
    const progress = total > 0 ? Math.round((done / total) * 100) : 0;
    void getCurrentWindow().setProgressBar({ status: ProgressBarStatus.Normal, progress }).catch(() => {});
  });

  const remoteSource = $derived<FileSource | null>(connectedHost ? remote(sessionId) : null);
  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const hostChoices = $derived(hostOptions(vaultStore.hosts));
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

  /** Add a transfer to the queue; it starts when a slot is free. */
  async function start(direction: Direction, sources: string[]) {
    if (!connectedHost || sources.length === 0) return;
    const destDir = direction === "upload" ? remotePath : localPath;
    const conflict = await resolveConflicts(direction === "upload" ? remote(sessionId) : local, destDir, sources);
    if (!conflict) return;
    const label = sources.length === 1 ? baseName(sources[0]) : `${sources.length} items`;
    transfers.push({ id: crypto.randomUUID(), direction, label, progress: { state: "queued" }, sources, destDir, resume: false, conflict });
    pump();
  }

  /** Ask what to do about names that are already in the destination folder; null if the person backs out. */
  async function resolveConflicts(dest: FileSource, destDir: string, sources: string[]): Promise<Conflict | null> {
    let there: Set<string>;
    try {
      there = new Set((await dest.list(destDir)).map((e) => e.name));
    } catch {
      return "overwrite"; // can't tell; the transfer itself reports a problem with the folder
    }
    const clashes = sources.map(baseName).filter((n) => there.has(n));
    if (clashes.length === 0) return "overwrite";
    const shown = clashes.slice(0, 5).join(", ") + (clashes.length > 5 ? `, and ${clashes.length - 5} more` : "");
    const answer = await choose(
      `${clashes.length === 1 ? "This name is" : "These names are"} already in the destination folder: ${shown}.`,
      [
        { value: "overwrite", label: "Replace" },
        { value: "skip", label: "Skip" },
        { value: "rename", label: "Keep both" },
      ],
      { title: "Already there" },
    );
    return (answer as Conflict | null) ?? null;
  }

  /** Start queued transfers, oldest first, up to the limit. */
  function pump() {
    let running = transfers.filter((t) => active(t.progress)).length;
    for (const t of transfers) {
      if (running >= MAX_RUNNING) break;
      if (t.progress.state === "queued") {
        running++;
        void run(t);
      }
    }
  }

  async function run(t: Transfer) {
    const id = t.id;
    t.progress = { state: "started", total_bytes: 0, total_files: 0 };
    const find = () => transfers.find((x) => x.id === id);
    try {
      const [from, to] = t.direction === "upload" ? [LOCAL_ID, sessionId] : [sessionId, LOCAL_ID];
      await sftp.transfer(
        from,
        to,
        id,
        t.sources,
        t.destDir,
        (p) => {
          const cur = find();
          if (!cur) return;
          cur.progress = p;
          if (p.state === "done") {
            if (cur.direction === "upload") remoteRefresh++;
            else localRefresh++;
          }
          if (p.state === "done" || p.state === "failed" || p.state === "cancelled") pump();
        },
        { resume: t.resume, conflict: t.conflict },
      );
    } catch (e) {
      const cur = find();
      if (cur) cur.progress = { state: "failed", message: errorMessage(e) };
      pump();
    }
  }

  /** Try again, continuing partial files rather than starting over. */
  function retry(t: Transfer) {
    // Retries are appended with a fresh id so late events from the old run can't clobber them.
    transfers = transfers.filter((x) => x.id !== t.id);
    transfers.push({ ...t, id: crypto.randomUUID(), progress: { state: "queued" }, resume: true });
    pump();
  }

  function cancel(t: Transfer) {
    if (t.progress.state === "queued") t.progress = { state: "cancelled" };
    else void sftp.cancel(t.id);
    pump();
  }

  function pct(p: TransferProgress) {
    if (p.state === "done") return 100;
    if ((p.state === "progress" || p.state === "paused") && p.total_bytes > 0) return Math.min(100, (p.bytes / p.total_bytes) * 100);
    return 0;
  }

  const active = (p: TransferProgress) => p.state === "started" || p.state === "progress" || p.state === "paused";
  const running = $derived(transfers.filter((t) => active(t.progress)).length);
  const queued = $derived(transfers.filter((t) => t.progress.state === "queued").length);
</script>

<div class="relative flex min-w-0 flex-1 flex-col">
  {#if osDrop}
    <div class="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-dashed border-accent bg-accent/10 text-sm font-medium text-accent">
      Drop to upload to {remotePath || "the remote folder"} on {connectedLabel}
    </div>
  {/if}
  <div bind:this={paneRow} class="flex min-h-0 flex-1">
    <div class="min-w-0 overflow-hidden" style:flex="0 0 {settings.prefs.sftpSplitRatio * 100}%">
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
    </div>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="relative w-1.5 shrink-0 cursor-col-resize outline-none after:absolute after:left-1/2 after:top-0 after:h-full after:w-px after:-translate-x-1/2 after:bg-line after:transition-colors hover:after:bg-accent/60 focus-visible:after:bg-accent"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the local and remote panes"
      aria-valuenow={Math.round(settings.prefs.sftpSplitRatio * 100)}
      aria-valuemin={15}
      aria-valuemax={85}
      tabindex="0"
      title="Drag to resize · double-click to centre"
      onpointerdown={startSplitDrag}
      ondblclick={() => (settings.prefs.sftpSplitRatio = 0.5)}
      onkeydown={(e) => {
        const step = e.shiftKey ? 0.05 : 0.01;
        if (e.key === "ArrowLeft") { e.preventDefault(); settings.prefs.sftpSplitRatio = Math.max(0.15, settings.prefs.sftpSplitRatio - step); }
        else if (e.key === "ArrowRight") { e.preventDefault(); settings.prefs.sftpSplitRatio = Math.min(0.85, settings.prefs.sftpSplitRatio + step); }
        else if (e.key === "Home" || e.key === "Enter") { e.preventDefault(); settings.prefs.sftpSplitRatio = 0.5; }
      }}
    ></div>

    <div class="min-w-0 flex-1 overflow-hidden border-l border-line">
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
          <Combobox
            class="min-w-0 flex-1"
            inputClass="py-1"
            options={hostChoices}
            bind:value={hostId}
            ariaLabel="Remote host"
            placeholder="Search hosts…"
            emptyText="No host matches"
            onchange={(v) => {
              // Picking a host is intent enough: connect straight away.
              if (v && !connecting) void connect();
            }}
          />
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
              <div>
                <label class="label" for="sftp-user">Username</label>
                <input id="sftp-user" class="input font-mono" autocomplete="username" bind:value={username} required />
              </div>
              <div>
                <label class="label" for="sftp-pass">Password</label>
                <input id="sftp-pass" class="input" type="password" autocomplete="current-password" bind:value={password} required />
              </div>
              <div class="flex gap-2">
                <button
                  class="btn-ghost flex-1"
                  type="button"
                  onclick={() => {
                    askCreds = false;
                    hostId = "";
                    username = "";
                    password = "";
                  }}
                >
                  Cancel
                </button>
                <button class="btn-primary flex-1" type="submit">Connect</button>
              </div>
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
        <span class="font-medium uppercase tracking-wide">Transfers{running || queued ? ` · ${running} running${queued ? `, ${queued} queued` : ""}` : ""}</span>
        <span class="flex gap-3">
          {#if transfers.some((t) => t.progress.state === "failed" || t.progress.state === "cancelled")}
            <button class="hover:text-fg" onclick={() => transfers.filter((t) => t.progress.state === "failed" || t.progress.state === "cancelled").forEach(retry)}>Retry all</button>
          {/if}
          <button class="hover:text-fg" onclick={() => (transfers = transfers.filter((t) => active(t.progress) || t.progress.state === "queued"))}>Clear finished</button>
        </span>
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
                {:else if t.progress.state === "paused"}
                  <span class="text-warning">Paused · {formatBytes(t.progress.bytes)} / {formatBytes(t.progress.total_bytes)}</span>
                {:else if t.progress.state === "queued"}
                  Queued
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
          {#if t.progress.state === "paused"}
            <button class="icon-btn h-6 w-6" title="Resume" onclick={() => sftp.resume(t.id)}><Play size={13} /></button>
          {:else if t.progress.state === "progress" || t.progress.state === "started"}
            <button class="icon-btn h-6 w-6" title="Pause" onclick={() => sftp.pause(t.id)}><Pause size={13} /></button>
          {/if}
          {#if t.progress.state === "failed" || t.progress.state === "cancelled"}
            <button class="icon-btn h-6 w-6" title="Retry (continues partial files)" onclick={() => retry(t)}><RotateCcw size={13} /></button>
          {/if}
          {#if active(t.progress) || t.progress.state === "queued"}
            <button class="icon-btn h-6 w-6 hover:text-danger" title="Cancel" onclick={() => cancel(t)}><CircleStop size={13} /></button>
          {:else}
            <button class="icon-btn h-6 w-6" title="Dismiss" onclick={() => (transfers = transfers.filter((x) => x.id !== t.id))}><X size={13} /></button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
