<script lang="ts">
  import { ArrowUp, Eye, EyeOff, File, FilePen, FileSearch, Folder, FolderOpen, FolderPlus, KeyRound, Link, RefreshCw, TextCursorInput, Trash2 } from "lucide-svelte";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { ask, askText } from "$lib/dialogs.svelte";
  import PreviewDialog from "./PreviewDialog.svelte";
  import type { Preview } from "$lib/sftp";
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
    onEdit,
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
    /** Open a file for editing (remote pane only). */
    onEdit?: (entry: FileEntry) => void;
  } = $props();

  let entries = $state<FileEntry[]>([]);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(false);
  let error = $state<string | null>(null);
  let pathInput = $state("");
  let dragOver = $state(false);
  let lastClicked: string | null = null;

  // -- quick look, permissions, reveal ---------------------------------------
  let previewing = $state<{ entry: FileEntry; data: Preview | null } | null>(null);

  const one = $derived.by(() => {
    if (selected.size !== 1) return null;
    const [p] = [...selected];
    return entries.find((x) => x.path === p) ?? null;
  });

  async function quickLook(e: FileEntry | null = one) {
    if (!e || e.is_dir || !source) return;
    previewing = { entry: e, data: null };
    try {
      const data = await source.preview(e.path);
      if (previewing?.entry.path === e.path) previewing = { entry: e, data };
    } catch (err) {
      previewing = null;
      error = errorMessage(err);
    }
  }

  function modeString(m: number | null) {
    if (m == null) return "";
    const r = (b: number, c: string) => (m & b ? c : "-");
    return [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1].map((b, i) => r(b, "rwx"[i % 3])).join("");
  }

  async function chmod() {
    const e = one;
    if (!e || !source?.chmod) return;
    const cur = e.permissions != null ? (e.permissions & 0o7777).toString(8).padStart(3, "0") : "644";
    const v = await askText(`Permissions for ${e.name}`, cur, { title: "Change permissions", placeholder: "644 or 755" });
    if (v == null) return;
    if (!/^[0-7]{3,4}$/.test(v.trim())) return void (error = "Enter permissions as 3 or 4 octal digits, like 644.");
    try {
      await source.chmod(e.path, parseInt(v.trim(), 8));
      await load(path);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function reveal() {
    const target = one?.path ?? path;
    try {
      await revealItemInDir(target);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  function onKey(ev: KeyboardEvent) {
    if (ev.key === " " && one && !one.is_dir) {
      ev.preventDefault();
      void quickLook();
    }
  }

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
    else onEdit?.(e);
  }

  const editable = $derived.by(() => {
    if (!onEdit || selected.size !== 1) return null;
    const [p] = [...selected];
    const e = entries.find((x) => x.path === p);
    return e && !e.is_dir ? e : null;
  });

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
    const name = await askText("New folder name", "", { placeholder: "folder name" });
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
    const name = await askText(`Rename ${entry.name}`, entry.name, { confirm: "Rename" });
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
    if (!await ask(`Delete ${n} item${n > 1 ? "s" : ""}? Folders are deleted recursively.`)) return;
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
      {#if onEdit}
        <button class="icon-btn h-7 w-7" title="Edit in local editor (or double-click a file)" disabled={!editable} onclick={() => editable && onEdit?.(editable)}><FilePen size={14} /></button>
      {/if}
      <button class="icon-btn h-7 w-7" title="Quick look (Space)" disabled={!one || one.is_dir} onclick={() => quickLook()}><FileSearch size={14} /></button>
      {#if source.chmod}
        <button class="icon-btn h-7 w-7" title="Permissions (chmod)" disabled={!one} onclick={chmod}><KeyRound size={14} /></button>
      {/if}
      {#if side === "local"}
        <button class="icon-btn h-7 w-7" title="Show in file manager" onclick={reveal}><FolderOpen size={14} /></button>
      {/if}
      <button class="icon-btn h-7 w-7" title="Rename" disabled={selected.size !== 1} onclick={rename}><TextCursorInput size={14} /></button>
      <button class="icon-btn h-7 w-7 hover:text-danger" title="Delete" disabled={selected.size === 0} onclick={remove}><Trash2 size={14} /></button>
    </div>

    {#if error}
      <div class="border-b border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">{error}</div>
    {/if}

    <div class="min-h-0 flex-1 overflow-auto" tabindex="-1" role="grid" aria-label="{side} file list" onkeydown={onKey}>
      <table class="w-full table-fixed text-sm">
        <thead class="sticky top-0 bg-base text-left text-xs text-fg-muted">
          <tr class="border-b border-line">
            <th class="px-3 py-1.5 font-medium">Name</th>
            <th class="w-24 px-2 py-1.5 text-right font-medium">Size</th>
            <th class="hidden w-36 px-3 py-1.5 font-medium lg:table-cell">Modified</th>
            <th class="hidden w-24 px-2 py-1.5 font-medium xl:table-cell">Mode</th>
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
              <td class="hidden px-2 py-1 font-mono text-[11px] text-fg-muted xl:table-cell">{modeString(e.permissions)}</td>
            </tr>
          {:else}
            {#if !loading && !error}
              <tr><td colspan="4" class="px-3 py-8 text-center text-xs text-fg-muted">Empty folder</td></tr>
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

{#if previewing}
  <PreviewDialog entry={previewing.entry} preview={previewing.data} onclose={() => (previewing = null)} />
{/if}
