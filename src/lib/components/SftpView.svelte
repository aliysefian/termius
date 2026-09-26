<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { CircleStop, HardDrive, Loader2, Monitor, Server, Unplug, X } from "lucide-svelte";
  import FilePane from "./FilePane.svelte";
  import { local, remote, sftp, type Direction, type FileSource } from "$lib/sftp";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, formatBytes, type TransferProgress, type Uuid } from "$lib/types";

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
  onDestroy(() => {
    if (connectedHost) void sftp.close(sessionId);
  });

  async function connect(creds: { username: string; password: string } | null = null) {
    if (!hostId) return;
    if (!creds && host && !host.identity_id) {
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

  async function disconnect() {
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

<div class="flex min-w-0 flex-1 flex-col">
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
