<script lang="ts">
  import { ArrowUp, ChevronDown, ChevronUp, Clipboard, Copy, Download, Eye, EyeOff, File, FileArchive, FileCode, FileCog, FileImage, FileKey, FilePen, FileSearch, FileText, Folder, FolderOpen, FolderPlus, KeyRound, Link, RefreshCw, Search, SquareTerminal, TextCursorInput, Trash2, Upload } from "lucide-svelte";
  import { isTextName } from "$lib/conflict";
  import { fileKind, type FileKind } from "$lib/filekind";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { ask, askText } from "$lib/dialogs.svelte";
  import PreviewDialog from "./PreviewDialog.svelte";
  import type { Preview } from "$lib/sftp";
  import { settings } from "$lib/stores/settings.svelte";
  import { tick, type Snippet as SvelteSnippet } from "svelte";
  import { ALL_CAPS, parentPath, type Caps, type FileSource } from "$lib/sftp";
  import { errorMessage, formatBytes, type FileEntry } from "$lib/types";
  import { keepInView } from "$lib/actions";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ui } from "$lib/stores/ui.svelte";

  let {
    side,
    pane = side === "local" ? "left" : "right",
    verb = side === "local" ? "Upload" : "Download",
    source,
    path = $bindable(""),
    refreshKey = 0,
    transferLabel,
    onTransfer,
    onDropFrom,
    header,
    notice,
    placeholder,
    onEdit,
    onOpenEditor,
    onCdHere,
  }: {
    side: "local" | "remote";
    /** Which pane this is: a drag only drops on the other one, even when both show hosts. */
    pane?: "left" | "right";
    /** What copying to the other pane is called: Upload, Download, or Copy between two hosts. */
    verb?: "Upload" | "Download" | "Copy";
    source: FileSource | null;
    path?: string;
    /** Bump to force a reload (e.g. after a transfer lands here). */
    refreshKey?: number;
    transferLabel: string;
    onTransfer: (paths: string[]) => void;
    /** Paths dragged in from the opposite pane. */
    onDropFrom: (paths: string[]) => void;
    header?: SvelteSnippet;
    /** A line under the header, for a hint about this pane (shown only while the pane has something open). */
    notice?: SvelteSnippet;
    placeholder?: SvelteSnippet;
    /** Open a file in the local editor, saving back as it is saved there. */
    onEdit?: (entry: FileEntry) => void;
    /** Open a text file in the in-app editor (double-click, Ctrl+E). */
    onOpenEditor?: (entry: FileEntry) => void;
    /** Take the terminal of this place to a folder (`cd here`). */
    onCdHere?: (entry: FileEntry) => void;
  } = $props();

  /** What this place supports; the actions it can't do are not offered. */
  let caps = $state<Caps>(ALL_CAPS);
  $effect(() => {
    const src = source;
    caps = ALL_CAPS;
    if (src) void src.caps().then((c) => src === source && (caps = c), () => {});
  });

  let entries = $state<FileEntry[]>([]);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(false);
  let error = $state<string | null>(null);
  let pathInput = $state("");
  let pathEl = $state<HTMLInputElement>();
  let dragOver = $state(false);
  let lastClicked: string | null = null;
  let editingPath = $state(false);

  function pathSegments(p: string, sep: string): { name: string; path: string }[] {
    if (sep === "/") {
      const parts = p.split("/").filter(Boolean);
      const segs = [{ name: "", path: "/" }];
      let cur = "";
      for (const part of parts) {
        cur += `/${part}`;
        segs.push({ name: part, path: cur });
      }
      return segs;
    }
    // Windows: the first token is the drive letter.
    const parts = p.split(/[\\/]+/).filter(Boolean);
    if (parts.length === 0) return [{ name: p, path: p }];
    const [drive, ...rest] = parts;
    const segs = [{ name: drive, path: `${drive}\\` }];
    let cur = drive;
    for (const part of rest) {
      cur += `\\${part}`;
      segs.push({ name: part, path: cur });
    }
    return segs;
  }

  // -- quick look, permissions, reveal ---------------------------------------
  let previewing = $state<{ entry: FileEntry; data: Preview | null } | null>(null);

  const one = $derived.by(() => {
    if (selected.size !== 1) return null;
    const [p] = [...selected];
    return entries.find((x) => x.path === p) ?? null;
  });

  async function quickLook(e: FileEntry | null = one) {
    if (!caps.preview) return;
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

  /** Ctrl/Cmd+L from anywhere in the pane: type a path, like a browser's address bar. */
  function onPaneKey(ev: KeyboardEvent) {
    // Ctrl/Cmd+E: edit the one selected file in the app.
    if (onOpenEditor && (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "e" && editable) {
      ev.preventDefault();
      onOpenEditor(editable);
      return;
    }
    if (source && (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "l") {
      ev.preventDefault();
      pathInput = path;
      editingPath = true;
      // `autofocus` only fires when nothing has focus, and the file list has it here.
      void tick().then(() => pathEl?.focus());
    }
  }

  function onKey(ev: KeyboardEvent) {
    if (ev.key === " " && one && !one.is_dir) {
      ev.preventDefault();
      void quickLook();
    }
  }

  const shown = $derived(settings.prefs.showHiddenFiles ? entries : entries.filter((e) => !e.name.startsWith(".")));
  const hiddenCount = $derived(entries.length - shown.length);

  let filterQuery = $state("");
  const filtered = $derived.by(() => {
    const q = filterQuery.trim().toLowerCase();
    return q ? shown.filter((e) => e.name.toLowerCase().includes(q)) : shown;
  });

  type SortKey = "name" | "size" | "modified" | "mode";
  let sortKey = $state<SortKey>("name");
  let sortDir = $state<1 | -1>(1);

  function sortBy(key: SortKey) {
    if (sortKey === key) sortDir = sortDir === 1 ? -1 : 1;
    else {
      sortKey = key;
      sortDir = 1;
    }
  }

  const visible = $derived.by(() => {
    // Folders always sort before files; the chosen key only orders within each group.
    const cmp: Record<SortKey, (a: FileEntry, b: FileEntry) => number> = {
      name: (a, b) => a.name.localeCompare(b.name),
      size: (a, b) => a.size - b.size,
      modified: (a, b) => (a.modified ?? 0) - (b.modified ?? 0),
      mode: (a, b) => (a.permissions ?? 0) - (b.permissions ?? 0),
    };
    return [...filtered].sort((a, b) => (a.is_dir !== b.is_dir ? (a.is_dir ? -1 : 1) : sortDir * cmp[sortKey](a, b)));
  });

  /** Only the newest request may fill the list: a slow listing of a folder already left must not replace the current one. */
  let loadSeq = 0;

  async function load(p: string) {
    if (!source || !p) return;
    const seq = ++loadSeq;
    loading = true;
    error = null;
    try {
      const list = await source.list(p);
      if (seq !== loadSeq) return;
      entries = list;
      selected = new Set();
    } catch (e) {
      if (seq === loadSeq) {
        entries = [];
        error = errorMessage(e);
      }
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  $effect(() => {
    void refreshKey;
    pathInput = path;
    editingPath = false;
    load(path);
  });

  const breadcrumbs = $derived(source ? pathSegments(path, source.sep) : []);

  function open(e: FileEntry) {
    if (e.is_dir) path = e.path;
    else if (onOpenEditor && isTextName(e.name)) onOpenEditor(e);
    else onEdit?.(e);
  }

  const editable = $derived.by(() => {
    if (!(onEdit || onOpenEditor) || selected.size !== 1) return null;
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
    if (!caps.mkdir) return;
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
    if (!caps.rename) return;
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
    if (!caps.delete) return;
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
    ev.dataTransfer?.setData("application/x-sshvault-files", JSON.stringify({ pane, paths: [...selected] }));
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "copy";
  }

  function drop(ev: DragEvent) {
    dragOver = false;
    const raw = ev.dataTransfer?.getData("application/x-sshvault-files");
    if (!raw) return;
    ev.preventDefault();
    const { pane: from, paths } = JSON.parse(raw) as { pane: string; paths: string[] };
    if (from !== pane && paths.length) onDropFrom(paths);
  }

  function fmtDate(s: number | null) {
    return s ? new Date(s * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" }) : "";
  }

  const ICONS = { folder: Folder, code: FileCode, archive: FileArchive, config: FileCog, key: FileKey, image: FileImage, text: FileText, file: File } as const;
  const ICON_TONE: Record<FileKind, string> = {
    folder: "text-accent",
    code: "text-success",
    archive: "text-warning",
    config: "text-fg-muted",
    key: "text-danger",
    image: "text-accent",
    text: "text-fg-muted",
    file: "text-fg-muted",
  };

  /** Run a row's hover action on that row alone, the way the toolbar does on a single selection. */
  function rowAction(ev: Event, e: FileEntry, run: () => unknown) {
    ev.stopPropagation();
    selected = new Set([e.path]);
    void run();
  }

  // -- right-click menu ---------------------------------------------------

  let menu = $state<{ x: number; y: number; entry: FileEntry } | null>(null);

  function openMenu(ev: MouseEvent, e: FileEntry) {
    ev.preventDefault();
    if (!selected.has(e.path)) selected = new Set([e.path]);
    menu = { x: ev.clientX, y: ev.clientY, entry: e };
  }

  async function copyPath(e: FileEntry) {
    try {
      await writeText(e.path);
      ui.notify("info", "Path copied.");
    } catch (err) {
      error = errorMessage(err);
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (menu && e.key === "Escape") menu = null; }} onresize={() => (menu = null)} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="flex min-h-0 min-w-0 flex-1 flex-col bg-base {dragOver ? 'ring-2 ring-inset ring-accent' : ''}"
  role="region"
  aria-label="{side} files"
  onkeydown={onPaneKey}
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
  {#if source}{@render notice?.()}{/if}

  {#if !source}
    <div class="flex flex-1 items-center justify-center p-6 text-center">
      {@render placeholder?.()}
    </div>
  {:else}
    <div class="flex items-center gap-1 border-b border-line px-2 py-1.5">
      <button class="icon-btn h-7 w-7" title="Up" onclick={() => (path = parentPath(path, source.sep))}><ArrowUp size={14} /></button>
      {#if editingPath}
        <form
          class="flex-1"
          onsubmit={(e) => {
            e.preventDefault();
            path = pathInput;
            editingPath = false;
          }}
        >
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="input py-1 font-mono text-xs"
            bind:value={pathInput}
            spellcheck="false"
            aria-label="Path"
            bind:this={pathEl}
            autofocus
            onfocus={(e) => e.currentTarget.select()}
            onblur={() => (editingPath = false)}
          />
        </form>
      {:else}
        <nav class="flex min-w-0 flex-1 items-center overflow-x-auto whitespace-nowrap rounded-md border border-line bg-base px-2 py-1 font-mono text-xs" aria-label="Path">
          {#each breadcrumbs as b, i (b.path)}
            {#if i > 0}<span class="px-1 text-fg-muted">{source.sep}</span>{/if}
            <button
              class="shrink-0 rounded px-0.5 hover:bg-panel-hover hover:text-accent {i === breadcrumbs.length - 1 ? 'text-fg' : 'text-fg-muted'}"
              onclick={() => (path = b.path)}
            >
              {b.name || source.sep}
            </button>
          {/each}
          <button class="ml-auto shrink-0 pl-2 text-fg-muted hover:text-fg" title="Type a path (Ctrl+L)" aria-label="Type a path" onclick={() => (editingPath = true)}>
            <TextCursorInput size={12} />
          </button>
        </nav>
      {/if}
      <button class="icon-btn h-7 w-7" title="Refresh" onclick={() => load(path)}><RefreshCw size={14} class={loading ? "animate-spin" : ""} /></button>
      <button
        class="icon-btn h-7 w-7 {settings.prefs.showHiddenFiles ? 'text-accent' : ''}"
        title={settings.prefs.showHiddenFiles ? "Hide hidden files" : "Show hidden files"}
        onclick={() => (settings.prefs.showHiddenFiles = !settings.prefs.showHiddenFiles)}
      >
        {#if settings.prefs.showHiddenFiles}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
      </button>
      {#if caps.mkdir}
        <button class="icon-btn h-7 w-7" title="New folder" onclick={mkdir}><FolderPlus size={14} /></button>
      {/if}
      {#if onOpenEditor}
        <button class="icon-btn h-7 w-7" title="Edit file (Ctrl+E, or double-click a text file)" aria-label="Edit file" disabled={!editable} onclick={() => editable && onOpenEditor?.(editable)}><FilePen size={14} /></button>
      {:else if onEdit && caps.edit}
        <button class="icon-btn h-7 w-7" title="Edit in local editor (or double-click a file)" disabled={!editable} onclick={() => editable && onEdit?.(editable)}><FilePen size={14} /></button>
      {/if}
      {#if caps.preview}
        <button class="icon-btn h-7 w-7" title="Quick look (Space)" disabled={!one || one.is_dir} onclick={() => quickLook()}><FileSearch size={14} /></button>
      {/if}
      {#if caps.chmod}
        <button class="icon-btn h-7 w-7" title="Permissions (chmod)" disabled={!one} onclick={chmod}><KeyRound size={14} /></button>
      {/if}
      {#if side === "local"}
        <button class="icon-btn h-7 w-7" title="Show in file manager" onclick={reveal}><FolderOpen size={14} /></button>
      {/if}
      {#if caps.rename}
        <button class="icon-btn h-7 w-7" title="Rename" disabled={selected.size !== 1} onclick={rename}><TextCursorInput size={14} /></button>
      {/if}
      {#if caps.delete}
        <button class="icon-btn h-7 w-7 hover:text-danger" title="Delete" disabled={selected.size === 0} onclick={remove}><Trash2 size={14} /></button>
      {/if}
    </div>

    {#if entries.length > 0 || filterQuery}
      <div class="relative border-b border-line px-2 py-1">
        <Search size={11} class="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input py-1 pl-6 text-xs" placeholder="Filter this folder…" bind:value={filterQuery} aria-label="Filter files" />
      </div>
    {/if}

    {#if error}
      <div class="border-b border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">{error}</div>
    {/if}

    <div class="min-h-0 flex-1 overflow-auto" tabindex="-1" role="grid" aria-label="{side} file list" onkeydown={onKey}>
      <table class="w-full table-fixed text-sm">
        <thead class="sticky top-0 bg-base text-left text-xs text-fg-muted">
          <tr class="border-b border-line">
            <th class="px-3 py-1.5 font-medium"><button class="flex items-center gap-1 hover:text-fg" onclick={() => sortBy("name")}>Name {#if sortKey === "name"}{#if sortDir === 1}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}{/if}</button></th>
            <th class="w-24 px-2 py-1.5 text-right font-medium"><button class="flex w-full items-center justify-end gap-1 hover:text-fg" onclick={() => sortBy("size")}>{#if sortKey === "size"}{#if sortDir === 1}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}{/if} Size</button></th>
            <th class="hidden w-44 px-3 py-1.5 font-medium lg:table-cell"><button class="flex items-center gap-1 hover:text-fg" onclick={() => sortBy("modified")}>Modified {#if sortKey === "modified"}{#if sortDir === 1}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}{/if}</button></th>
            <th class="hidden w-24 px-2 py-1.5 font-medium xl:table-cell"><button class="flex items-center gap-1 hover:text-fg" onclick={() => sortBy("mode")}>Mode {#if sortKey === "mode"}{#if sortDir === 1}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}{/if}</button></th>
          </tr>
        </thead>
        <tbody>
          {#each visible as e (e.path)}
            {@const kind = fileKind(e.name, e.is_dir)}
            {@const Icon = ICONS[kind]}
            <tr
              class="group cursor-default select-none {selected.has(e.path) ? 'bg-accent/20' : 'hover:bg-panel-hover'}"
              draggable="true"
              ondragstart={(ev) => dragStart(ev, e)}
              onclick={(ev) => click(ev, e)}
              ondblclick={() => open(e)}
              oncontextmenu={(ev) => openMenu(ev, e)}
            >
              <td class="relative truncate px-3 py-1">
                <span class="inline-flex items-center gap-2">
                  <Icon size={14} class="shrink-0 {ICON_TONE[kind]}" />
                  <span class="truncate">{e.name}</span>
                  {#if e.is_symlink}<Link size={11} class="shrink-0 text-fg-muted" />{/if}
                </span>
                <!-- Row actions float over the name's end on hover or keyboard focus, so a row costs no width. -->
                <span
                  class="absolute inset-y-0 right-1 my-auto hidden h-6 items-center rounded bg-panel pl-0.5 shadow-[-8px_0_8px_-2px_var(--color-panel)] group-hover:flex group-focus-within:flex"
                  role="toolbar"
                  tabindex="-1"
                  aria-label="Actions for {e.name}"
                  ondblclick={(ev) => ev.stopPropagation()}
                >
                  <button class="icon-btn h-6 w-6" title={verb} aria-label={verb} onclick={(ev) => rowAction(ev, e, () => onTransfer([e.path]))}>
                    {#if verb === "Upload"}<Upload size={12} />{:else if verb === "Download"}<Download size={12} />{:else}<Copy size={12} />{/if}
                  </button>
                  {#if caps.preview && !e.is_dir}
                    <button class="icon-btn h-6 w-6" title="View (Space)" aria-label="View" onclick={(ev) => rowAction(ev, e, () => quickLook(e))}><Eye size={12} /></button>
                  {/if}
                  {#if (onOpenEditor || (onEdit && caps.edit)) && !e.is_dir}
                    <button class="icon-btn h-6 w-6" title="Edit (Ctrl+E)" aria-label="Edit" onclick={(ev) => rowAction(ev, e, () => (onOpenEditor ?? onEdit)?.(e))}><FilePen size={12} /></button>
                  {/if}
                  {#if caps.rename}
                    <button class="icon-btn h-6 w-6" title="Rename" aria-label="Rename" onclick={(ev) => rowAction(ev, e, rename)}><TextCursorInput size={12} /></button>
                  {/if}
                  {#if caps.delete}
                    <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" aria-label="Delete" onclick={(ev) => rowAction(ev, e, remove)}><Trash2 size={12} /></button>
                  {/if}
                </span>
              </td>
              <td class="px-2 py-1 text-right font-mono text-xs tabular-nums text-fg-muted">{e.is_dir ? "" : formatBytes(e.size)}</td>
              <td class="hidden whitespace-nowrap px-3 py-1 font-mono text-xs tabular-nums text-fg-muted lg:table-cell">{fmtDate(e.modified)}</td>
              <td class="hidden px-2 py-1 font-mono text-[11px] tabular-nums text-fg-muted xl:table-cell">{modeString(e.permissions)}</td>
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

{#if menu}
  {@const e = menu.entry}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (menu = null)} oncontextmenu={(ev) => { ev.preventDefault(); menu = null; }}></button>
  <div class="fixed z-50 w-52 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" use:keepInView={menu} role="menu">
    {#if e.is_dir}
      <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { open(e); menu = null; }}><FolderOpen size={13} /> Open</button>
      {#if onCdHere}
        <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { onCdHere?.(e); menu = null; }}><SquareTerminal size={13} /> Open in terminal (cd here)</button>
      {/if}
    {:else}
      {#if onOpenEditor}
        <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { onOpenEditor?.(e); menu = null; }}><FilePen size={13} /> Edit <span class="ml-auto text-xs text-fg-muted">Ctrl+E</span></button>
      {/if}
      {#if onEdit && caps.edit}
        <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { onEdit?.(e); menu = null; }}><FilePen size={13} /> Edit in local editor</button>
      {/if}
      {#if caps.preview}
        <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void quickLook(e); menu = null; }}><FileSearch size={13} /> Quick look</button>
      {/if}
    {/if}
    <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { onTransfer([...selected]); menu = null; }}>
      {#if verb === "Upload"}<Copy size={13} /> Upload{:else if verb === "Download"}<Clipboard size={13} /> Download{:else}<Copy size={13} /> Copy to the other pane{/if}
    </button>
    <div class="my-1 border-t border-line"></div>
    <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void copyPath(e); menu = null; }}><Copy size={13} /> Copy path</button>
    {#if caps.rename}
      <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void rename(); menu = null; }}><TextCursorInput size={13} /> Rename</button>
    {/if}
    {#if caps.chmod}
      <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void chmod(); menu = null; }}><KeyRound size={13} /> Permissions…</button>
    {/if}
    {#if side === "local"}
      <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { void reveal(); menu = null; }}><FolderOpen size={13} /> Show in file manager</button>
    {/if}
    {#if caps.delete}
      <div class="my-1 border-t border-line"></div>
      <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-danger hover:bg-danger/10" role="menuitem" onclick={() => { void remove(); menu = null; }}><Trash2 size={13} /> Delete</button>
    {/if}
  </div>
{/if}
