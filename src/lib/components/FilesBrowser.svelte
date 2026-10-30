<script lang="ts">
  // The files of the host (or this computer) the focused terminal is on, for the right panel. It has its own
  // connection, so it never disturbs the terminal, and it stays connected while the panel is hidden.
  import { onDestroy } from "svelte";
  import { ArrowLeft, ArrowRight, ArrowUp, Copy, EllipsisVertical, File, Folder, FolderSync, Loader2, RefreshCw } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
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

  // Back and forward through the folders visited on this host.
  let hist = $state<string[]>([]);
  let at = $state(-1);
  $effect(() => {
    const p = conn.path;
    if (!p || p === hist[at]) return;
    hist = [...hist.slice(0, at + 1), p];
    at = hist.length - 1;
  });
  $effect(() => {
    // A different host starts a fresh trail.
    void conn.connectedHost;
    hist = [];
    at = -1;
  });
  const back = () => at > 0 && (conn.path = hist[--at]);
  const forward = () => at < hist.length - 1 && (conn.path = hist[++at]);

  // The ⋮ menu on a row.
  let menu = $state<{ entry: FileEntry; x: number; y: number } | null>(null);
  function openMenu(e: MouseEvent, entry: FileEntry) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { entry, x: Math.max(8, r.right - 208), y: r.bottom + 2 };
  }
  async function copyPath(path: string) {
    menu = null;
    try {
      await writeText(path);
      ui.notify("info", `Copied: ${path}`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
  const hostId = $derived(conn.connectedHost);
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
      <input
        class="h-8 min-w-0 flex-1 rounded-lg border border-line bg-base px-2.5 font-mono text-xs outline-none focus:border-accent/60"
        aria-label="Folder"
        title={conn.path}
        spellcheck="false"
        bind:value={pathInput}
        onkeydown={(e) => e.key === "Enter" && (conn.path = pathInput.trim() || conn.path)}
      />
      <button class="icon-btn h-8 w-8 shrink-0 border border-line bg-base" title="Back" aria-label="Back" disabled={at <= 0} onclick={back}><ArrowLeft size={15} /></button>
      <button class="icon-btn h-8 w-8 shrink-0 border border-line bg-base" title="Forward" aria-label="Forward" disabled={at >= hist.length - 1} onclick={forward}><ArrowRight size={15} /></button>
      <button class="icon-btn h-8 w-8 shrink-0 border border-line bg-base" title="Parent folder" aria-label="Parent folder" onclick={up}><ArrowUp size={15} /></button>
      <button class="icon-btn h-8 w-8 shrink-0 border border-line bg-base" title="Refresh" aria-label="Refresh" onclick={() => conn.refresh++}><RefreshCw size={15} class={loading ? "animate-spin" : ""} /></button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if error}<p class="p-3 text-danger">{error}</p>{/if}
      {#each entries as e (e.path)}
        <div class="group flex items-center gap-1 pr-1 hover:bg-panel-hover">
          <button
            class="flex min-w-0 flex-1 items-center gap-2.5 px-3 py-1.5 text-left text-[13px]"
            ondblclick={() => e.is_dir && (conn.path = e.path)}
            onclick={(ev) => ev.detail === 1 && e.is_dir && (conn.path = e.path)}
            title="{e.path}{e.is_dir ? '' : ` · ${formatBytes(e.size)}`}{e.modified ? ` · ${when(e.modified)}` : ''}"
          >
            {#if e.is_dir}<Folder size={18} class="shrink-0 fill-accent/25 text-accent" />{:else}<File size={18} class="shrink-0 text-fg-muted" />{/if}
            <span class="truncate">{e.name}</span>
          </button>
          <button class="icon-btn h-7 w-7 shrink-0 opacity-60 group-hover:opacity-100 focus-visible:opacity-100" title="More" aria-label="More actions for {e.name}" aria-haspopup="menu" onclick={(ev) => openMenu(ev, e)}><EllipsisVertical size={15} /></button>
        </div>
      {:else}
        {#if !loading && !error}<p class="p-3 text-fg-muted">This folder is empty.</p>{/if}
      {/each}
    </div>
  {/if}
</div>

{#if menu}
  {@const m = menu}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (menu = null)}></button>
  <div class="fixed z-50 w-52 rounded-lg border border-line bg-panel p-1 text-sm shadow-xl" style:left="{m.x}px" style:top="{m.y}px" role="menu">
    <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => void copyPath(m.entry.path)}><Copy size={14} /> Copy path</button>
    {#if hostId}
      {@const hid = hostId}
      <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { menu = null; ui.openSftpAt(hid, m.entry.is_dir ? m.entry.path : parentPath(m.entry.path, "/")); }}><FolderSync size={14} /> Open in the Files view</button>
    {/if}
  </div>
{/if}
