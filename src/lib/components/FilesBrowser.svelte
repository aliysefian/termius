<script lang="ts">
  // The files of the host (or this computer) the focused terminal is on, for the right panel. It has its own
  // connection, so it never disturbs the terminal, and it stays connected while the panel is hidden.
  import { onDestroy } from "svelte";
  import { ArrowUp, File, Folder, FolderSync, Loader2, RefreshCw } from "lucide-svelte";
  import { PaneConn, THIS_COMPUTER } from "$lib/paneconn.svelte";
  import { parentPath } from "$lib/sftp";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, formatBytes, type FileEntry } from "$lib/types";

  let { active }: { active: boolean } = $props();

  const conn = new PaneConn("right", false);
  let entries = $state<FileEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let pathInput = $state("");

  const tab = $derived(ui.activeTab);
  const target = $derived(tab?.panes.find((p) => p.id === tab.activePaneId)?.target);
  /** What to show: a saved host, this computer, or nothing for the kinds that have no files. */
  const wanted = $derived(target?.kind === "host" ? target.hostId : target?.kind === "local" ? THIS_COMPUTER : "");
  const supported = $derived(wanted === THIS_COMPUTER || (wanted !== "" && ["ssh", "ftp", undefined, ""].includes(vaultStore.hostById.get(wanted)?.data?.protocol)));

  // Connect only once the panel is in front, and follow the focused terminal's host from then on.
  $effect(() => {
    if (!active || !wanted || !supported || conn.connecting || conn.choice === wanted) return;
    void conn.pick(wanted);
  });

  $effect(() => {
    const src = conn.source;
    const path = conn.path;
    void conn.refresh;
    if (!active || !src || !path) return;
    loading = true;
    let stale = false;
    src
      .list(path)
      .then((list) => {
        if (stale) return;
        entries = list.sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || a.name.localeCompare(b.name));
        error = null;
        pathInput = path;
      })
      .catch((e) => !stale && ((error = errorMessage(e)), (entries = [])))
      .finally(() => !stale && (loading = false));
    return () => (stale = true);
  });

  const when = (s: number | null) => (s ? new Date(s * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" }) : "");
  const up = () => (conn.path = parentPath(conn.path, conn.source?.sep ?? "/"));
  onDestroy(() => void conn.dispose());
</script>

<div class="flex h-full min-h-0 flex-col text-sm">
  {#if !wanted || !supported}
    <p class="p-4 text-fg-muted">Files are shown for an SSH, SFTP or local terminal. Select one of those tabs.</p>
  {:else if conn.askCreds}
    <div class="space-y-2 p-4 text-fg-muted">
      <p>This host has no saved login, so its files can't open here.</p>
      <button class="btn-secondary" onclick={() => { if (wanted) { conn.cancelCreds(); ui.openSftpAt(wanted, ""); } }}><FolderSync size={14} /> Open in the Files view</button>
    </div>
  {:else if conn.connecting}
    <p class="flex items-center gap-2 p-4 text-fg-muted"><Loader2 size={14} class="animate-spin" /> Connecting…</p>
  {:else if conn.connectError}
    <div class="space-y-2 p-4">
      <p class="text-danger">{conn.connectError}</p>
      <button class="btn-secondary" onclick={() => void conn.connect()}><RefreshCw size={14} /> Try again</button>
    </div>
  {:else if conn.open}
    <div class="flex shrink-0 items-center gap-1 px-2 py-2">
      <button class="icon-btn h-7 w-7" title="Parent folder" aria-label="Parent folder" onclick={up}><ArrowUp size={14} /></button>
      <input
        class="h-7 min-w-0 flex-1 rounded-md border border-line bg-base px-2 font-mono text-xs outline-none focus:border-accent/60"
        aria-label="Folder"
        spellcheck="false"
        bind:value={pathInput}
        onkeydown={(e) => e.key === "Enter" && (conn.path = pathInput.trim() || conn.path)}
      />
      <button class="icon-btn h-7 w-7" title="Refresh" aria-label="Refresh" onclick={() => conn.refresh++}><RefreshCw size={14} class={loading ? "animate-spin" : ""} /></button>
    </div>
    <div class="grid shrink-0 grid-cols-[1fr_4.5rem_8.5rem] gap-2 border-b border-line px-3 pb-1 text-xs text-fg-muted @max-[22rem]:grid-cols-[1fr_4.5rem]">
      <span>Name</span><span>Size</span><span class="@max-[22rem]:hidden">Modified</span>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if error}<p class="p-3 text-danger">{error}</p>{/if}
      {#each entries as e (e.path)}
        <button
          class="grid w-full grid-cols-[1fr_4.5rem_8.5rem] items-center gap-2 px-3 py-1 text-left text-[13px] hover:bg-panel-hover @max-[22rem]:grid-cols-[1fr_4.5rem]"
          ondblclick={() => e.is_dir && (conn.path = e.path)}
          onclick={(ev) => ev.detail === 1 && e.is_dir && (conn.path = e.path)}
          title={e.path}
        >
          <span class="flex min-w-0 items-center gap-2">
            {#if e.is_dir}<Folder size={14} class="shrink-0 text-accent" />{:else}<File size={14} class="shrink-0 text-fg-muted" />{/if}
            <span class="truncate">{e.name}</span>
          </span>
          <span class="text-xs text-fg-muted">{e.is_dir ? "–" : formatBytes(e.size)}</span>
          <span class="text-xs tabular-nums text-fg-muted @max-[22rem]:hidden">{when(e.modified)}</span>
        </button>
      {:else}
        {#if !loading && !error}<p class="p-3 text-fg-muted">This folder is empty.</p>{/if}
      {/each}
    </div>
  {/if}
</div>
