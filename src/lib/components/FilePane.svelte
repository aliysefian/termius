<script lang="ts">
  import { ArrowUp, Eye, EyeOff, File, Folder, FolderPlus, Link, Pencil, RefreshCw, Trash2 } from "lucide-svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import type { Snippet as SvelteSnippet } from "svelte";
  import { parentPath, type FileSource } from "$lib/sftp";
  import { errorMessage, formatBytes, type FileEntry } from "$lib/types";

  let {
    side,
    source,
    path = $bindable(""),
    refreshKey = 0,
    transferLabel,
    onTransfer,
    onDropFrom,
    header,
    placeholder,
  }: {
    side: "local" | "remote";
    source: FileSource | null;
    path?: string;
    /** Bump to force a reload (e.g. after a transfer lands here). */
    refreshKey?: number;
    transferLabel: string;
    onTransfer: (paths: string[]) => void;
    /** Paths dragged in from the opposite pane. */
    onDropFrom: (paths: string[]) => void;
    header?: SvelteSnippet;
    placeholder?: SvelteSnippet;
  } = $props();

  let entries = $state<FileEntry[]>([]);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(false);
  let error = $state<string | null>(null);
  let pathInput = $state("");
  let dragOver = $state(false);
  let lastClicked: string | null = null;

  const visible = $derived(settings.prefs.showHiddenFiles ? entries : entries.filter((e) => !e.name.startsWith(".")));
  const hiddenCount = $derived(entries.length - visible.length);

  async function load(p: string) {
    if (!source || !p) return;
    loading = true;
    error = null;
    try {
      entries = await source.list(p);
      selected = new Set();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void refreshKey;
    pathInput = path;
    load(path);
  });

  function open(e: FileEntry) {
    if (e.is_dir) path = e.path;
  }

  function click(ev: MouseEvent, e: FileEntry) {
    const next = new Set(ev.ctrlKey || ev.metaKey ? selected : []);
    if (ev.shiftKey && lastClicked) {
      const a = visible.findIndex((x) => x.path === lastClicked);
      const b = visible.findIndex((x) => x.path === e.path);
      if (a >= 0 && b >= 0) for (const x of visible.slice(Math.min(a, b), Math.max(a, b) + 1)) next.add(x.path);
    } else if (next.has(e.path)) {
      next.delete(e.path);
    } else {
      next.add(e.path);
    }
    lastClicked = e.path;
    selected = next;
  }

  async function mkdir() {
    const name = prompt("New folder name");
    if (!name || !source) return;
    try {
      await source.mkdir(path, name);
      await load(path);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function rename() {
    const [p] = [...selected];
    const entry = entries.find((e) => e.path === p);
    if (!entry || !source) return;
    const name = prompt("Rename to", entry.name);
    if (!name || name === entry.name) return;
    try {
      await source.rename(entry.path, name);
      await load(path);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function remove() {
    if (!source || selected.size === 0) return;
    const n = selected.size;
    if (!confirm(`Delete ${n} item${n > 1 ? "s" : ""}? Folders are deleted recursively.`)) return;
    try {
      await source.remove([...selected]);
      await load(path);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function dragStart(ev: DragEvent, e: FileEntry) {
    if (!selected.has(e.path)) selected = new Set([e.path]);
    ev.dataTransfer?.setData("application/x-sshvault-files", JSON.stringify({ side, paths: [...selected] }));
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "copy";
  }

  function drop(ev: DragEvent) {
    dragOver = false;
    const raw = ev.dataTransfer?.getData("application/x-sshvault-files");
    if (!raw) return;
    ev.preventDefault();
    const { side: from, paths } = JSON.parse(raw) as { side: string; paths: string[] };
    if (from !== side && paths.length) onDropFrom(paths);
  }

  function fmtDate(s: number | null) {
    return s ? new Date(s * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" }) : "";
  }
</script>

<div
  class="flex min-w-0 flex-1 flex-col bg-base {dragOver ? 'ring-2 ring-inset ring-accent' : ''}"
  role="region"
  aria-label="{side} files"
  ondragover={(e) => {
    if (e.dataTransfer?.types.includes("application/x-sshvault-files")) {
      e.preventDefault();
      dragOver = true;
    }
  }}
  ondragleave={() => (dragOver = false)}
  ondrop={drop}
>
  <div class="flex items-center gap-2 border-b border-line bg-panel px-3 py-2">
    {@render header?.()}
  </div>

  {#if !source}
    <div class="flex flex-1 items-center justify-center p-6 text-center">
      {@render placeholder?.()}
    </div>
  {:else}
    <div class="flex items-center gap-1 border-b border-line px-2 py-1.5">
      <button class="icon-btn h-7 w-7" title="Up" onclick={() => (path = parentPath(path, source.sep))}><ArrowUp size={14} /></button>
      <form class="flex-1" onsubmit={(e) => { e.preventDefault(); path = pathInput; }}>
        <input class="input py-1 font-mono text-xs" bind:value={pathInput} spellcheck="false" aria-label="Path" />
      </form>
      <button class="icon-btn h-7 w-7" title="Refresh" onclick={() => load(path)}><RefreshCw size={14} class={loading ? "animate-spin" : ""} /></button>
      <button
        class="icon-btn h-7 w-7 {settings.prefs.showHiddenFiles ? 'text-accent' : ''}"
        title={settings.prefs.showHiddenFiles ? "Hide hidden files" : "Show hidden files"}
        onclick={() => (settings.prefs.showHiddenFiles = !settings.prefs.showHiddenFiles)}
      >
        {#if settings.prefs.showHiddenFiles}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
      </button>
      <button class="icon-btn h-7 w-7" title="New folder" onclick={mkdir}><FolderPlus size={14} /></button>
      <button class="icon-btn h-7 w-7" title="Rename" disabled={selected.size !== 1} onclick={rename}><Pencil size={14} /></button>
      <button class="icon-btn h-7 w-7 hover:text-danger" title="Delete" disabled={selected.size === 0} onclick={remove}><Trash2 size={14} /></button>
    </div>

    {#if error}
      <div class="border-b border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">{error}</div>
    {/if}

    <div class="min-h-0 flex-1 overflow-auto">
      <table class="w-full table-fixed text-sm">
        <thead class="sticky top-0 bg-base text-left text-xs text-fg-muted">
          <tr class="border-b border-line">
            <th class="px-3 py-1.5 font-medium">Name</th>
            <th class="w-24 px-2 py-1.5 text-right font-medium">Size</th>
            <th class="hidden w-36 px-3 py-1.5 font-medium lg:table-cell">Modified</th>
          </tr>
        </thead>
        <tbody>
          {#each visible as e (e.path)}
            <tr
              class="cursor-default select-none {selected.has(e.path) ? 'bg-accent/20' : 'hover:bg-panel-hover'}"
              draggable="true"
              ondragstart={(ev) => dragStart(ev, e)}
              onclick={(ev) => click(ev, e)}
              ondblclick={() => open(e)}
            >
              <td class="truncate px-3 py-1">
                <span class="inline-flex items-center gap-2">
                  {#if e.is_dir}<Folder size={14} class="shrink-0 text-accent" />{:else}<File size={14} class="shrink-0 text-fg-muted" />{/if}
                  <span class="truncate">{e.name}</span>
                  {#if e.is_symlink}<Link size={11} class="shrink-0 text-fg-muted" />{/if}
                </span>
              </td>
              <td class="px-2 py-1 text-right font-mono text-xs text-fg-muted">{e.is_dir ? "" : formatBytes(e.size)}</td>
              <td class="hidden px-3 py-1 text-xs text-fg-muted lg:table-cell">{fmtDate(e.modified)}</td>
            </tr>
          {:else}
            {#if !loading && !error}
              <tr><td colspan="3" class="px-3 py-8 text-center text-xs text-fg-muted">Empty folder</td></tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>

    <div class="flex items-center justify-between border-t border-line px-3 py-1.5 text-xs text-fg-muted">
      <span>{selected.size ? `${selected.size} selected` : `${visible.length} items${hiddenCount ? ` (${hiddenCount} hidden)` : ""}`}</span>
      <button class="btn-primary py-1 text-xs" disabled={selected.size === 0} onclick={() => onTransfer([...selected])}>
        {transferLabel}
      </button>
    </div>
  {/if}
</div>
