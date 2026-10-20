<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { bracketMatching, indentOnInput } from "@codemirror/language";
  import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
  import { Compartment, EditorState } from "@codemirror/state";
  import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers, rectangularSelection } from "@codemirror/view";
  import { KeyRound, Loader2, ShieldAlert, X } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { baseName, languageForContent, LANGUAGE_LABEL, type LangId } from "$lib/editorlang";
  import { editorLook, languageExtension } from "$lib/editorsetup";
  import { modeString } from "$lib/filemode";
  import { fileops, LOCAL_ID, type TextFile } from "$lib/sftp";
  import { ui } from "$lib/stores/ui.svelte";
  import { timeMachine } from "$lib/confighistory/useConfigTimeMachine.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, isApiError, type Uuid } from "$lib/types";

  // Edit a text file where it is: read into memory over the pane's own connection, saved back in place (⌘S). Nothing
  // is written to this computer's disk. A file the person may not write can be saved through `sudo` after asking.
  let {
    sessionId,
    path,
    hostId = null,
    onclose,
    onsaved,
  }: {
    sessionId: string;
    path: string;
    /** The saved host the file is on, for the environment badge; none for this computer. */
    hostId?: Uuid | null;
    onclose: () => void;
    /** Called after every successful save, so the file list can refresh. */
    onsaved?: () => void;
  } = $props();

  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const env = $derived(envInfo(vaultStore.effectiveEnv(host)));
  const where = $derived(sessionId === LOCAL_ID ? "This computer" : (host?.label ?? "remote host"));
  const name = $derived(baseName(path));

  let file = $state<TextFile | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let saving = $state(false);
  let dirty = $state(false);
  let lang = $state<LangId>("plain");
  let cursor = $state({ line: 1, col: 1 });
  let lines = $state(1);
  /** What the bar under the header is asking or telling, if anything. */
  type Bar = { kind: "sudo" } | { kind: "sudo-password"; error?: string } | { kind: "changed" } | { kind: "error"; text: string } | { kind: "saved" };
  let bar = $state<Bar | null>(null);
  let sudoPassword = $state("");

  let host_el = $state<HTMLDivElement>();
  let view: EditorView | null = null;
  const language = new Compartment();
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  const text = () => view?.state.doc.toString() ?? "";

  async function build(f: TextFile) {
    lang = languageForContent(path, f.text);
    const ext = await languageExtension(lang);
    await tick();
    if (!host_el) return;
    view?.destroy();
    view = new EditorView({
      parent: host_el,
      state: EditorState.create({
        doc: f.text,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightSpecialChars(),
          history(),
          drawSelection(),
          rectangularSelection(),
          indentOnInput(),
          bracketMatching(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          editorLook,
          language.of(ext),
          keymap.of([{ key: "Mod-s", preventDefault: true, run: () => (void save(), true) }, ...searchKeymap, ...historyKeymap, indentWithTab, ...defaultKeymap]),
          EditorView.contentAttributes.of({ "aria-label": `Editing ${name}`, spellcheck: "false" }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) dirty = true;
            if (u.docChanged || u.selectionSet) {
              const head = u.state.selection.main.head;
              const line = u.state.doc.lineAt(head);
              cursor = { line: line.number, col: head - line.from + 1 };
              lines = u.state.doc.lines;
            }
          }),
        ],
      }),
    });
    lines = view.state.doc.lines;
    dirty = false;
    view.focus();
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      file = await fileops.readText(sessionId, path);
      loading = false;
      // What was on the host before any edit here: kept (if watched and different) while the editor draws.
      if (hostId && sessionId !== LOCAL_ID) {
        const perm = file.permissions === null ? "" : (file.permissions & 0o7777).toString(8).padStart(4, "0");
        void timeMachine.onEditorOpened(hostId, path, file.crlf ? file.text.replace(/\r?\n/g, "\r\n") : file.text, perm, file.owner && file.group ? `${file.owner}:${file.group}` : "");
      }
      await build(file);
    } catch (e) {
      loadError = isApiError(e) && (e.code === "not_text" || e.code === "too_large") ? `${e.message}. Open it in a local editor instead.` : errorMessage(e);
      loading = false;
    }
  }

  onMount(() => void load());
  onDestroy(() => {
    view?.destroy();
    clearTimeout(savedTimer);
    sudoPassword = "";
  });

  function saved(modified: number | null) {
    dirty = false;
    if (file) file = { ...file, modified, size: new TextEncoder().encode(text()).length };
    bar = { kind: "saved" };
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => {
      if (bar?.kind === "saved") bar = null;
    }, 2500);
    onsaved?.();
  }

  /** Save over the file. `force` skips the check that nobody else changed it meanwhile. */
  async function save(force = false) {
    if (!file || saving || !view) return;
    saving = true;
    bar = null;
    try {
      const r = await fileops.writeText(sessionId, path, text(), file.crlf, force ? null : file.modified);
      saved(r.modified);
    } catch (e) {
      if (isApiError(e) && e.code === "permission_denied") bar = { kind: "sudo" };
      else if (isApiError(e) && e.code === "changed_on_server") bar = { kind: "changed" };
      else bar = { kind: "error", text: errorMessage(e) };
    } finally {
      saving = false;
    }
  }

  /** Save through sudo. With no password, sudo is tried as it is (a host where it asks for none). */
  async function saveAsRoot(password: string | null) {
    if (!file || saving || !view) return;
    saving = true;
    try {
      await fileops.writeAsRoot(sessionId, path, text(), file.crlf, password);
      sudoPassword = "";
      // Read the file's new time (and owner), so a later save knows what it is saving over.
      const again = await fileops.readText(sessionId, path).catch(() => null);
      saved(again?.modified ?? null);
      if (again && file) file = { ...file, owner: again.owner, group: again.group, permissions: again.permissions };
    } catch (e) {
      sudoPassword = "";
      const code = isApiError(e) ? e.code : "";
      if (code === "sudo_password_required") bar = { kind: "sudo-password" };
      else if (code === "sudo_wrong_password") bar = { kind: "sudo-password", error: "sudo did not accept that password. Try again." };
      else bar = { kind: "error", text: errorMessage(e) };
    } finally {
      saving = false;
    }
  }

  async function reload() {
    if (dirty && !(await ask("Reload the file from the host and drop your unsaved changes?", { title: "Reload", confirm: "Reload", danger: true }))) return;
    bar = null;
    await load();
  }

  async function requestClose() {
    if (dirty && !(await ask(`Close without saving your changes to ${name}?`, { title: "Unsaved changes", confirm: "Discard changes", danger: true }))) return;
    onclose();
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); void requestClose(); } }} />

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" role="presentation">
  <div
    class="flex h-[85vh] w-[80vw] min-w-0 max-w-full flex-col overflow-hidden rounded-lg border border-line bg-base"
    role="dialog"
    aria-modal="true"
    aria-label="Editing {name}"
    data-testid="remote-editor"
  >
    <header class="flex items-center gap-3 border-b border-line bg-panel px-4 py-2">
      {#if env.value}<Badge tone={env.tone}>{env.short ?? ""}</Badge>{/if}
      <span class="shrink-0 text-xs text-fg-muted">{where}</span>
      <span class="min-w-0 flex-1 truncate font-mono text-xs tabular-nums" title={path} data-testid="editor-path">{path}</span>
      {#if file}
        <span class="shrink-0 font-mono text-xs tabular-nums text-fg-muted" title="Permissions" data-testid="editor-perms">{modeString(file.permissions)}</span>
        {#if file.owner}<span class="shrink-0 font-mono text-xs tabular-nums text-fg-muted" title="Owner and group" data-testid="editor-owner">{file.owner}:{file.group}</span>{/if}
      {/if}
      {#if dirty}<span class="h-2 w-2 shrink-0 rounded-full bg-warning" title="Unsaved changes" role="img" aria-label="Unsaved changes" data-testid="editor-dirty"></span>{/if}
      <button class="btn-primary px-3 py-1 text-xs" disabled={!file || saving || !dirty} onclick={() => void save()} title="Save (Ctrl+S)">
        {#if saving}<Loader2 size={12} class="animate-spin" />{/if} Save
      </button>
      <button class="icon-btn h-7 w-7" aria-label="Close" title="Close (Esc)" onclick={() => void requestClose()}><X size={14} /></button>
    </header>

    {#if bar?.kind === "sudo"}
      <div class="flex flex-wrap items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2 text-xs text-warning" role="alert" data-testid="sudo-bar">
        <ShieldAlert size={14} class="shrink-0" />
        <span class="min-w-0 flex-1">Permission denied{file?.owner ? ` (owned by ${file.owner})` : ""}. Save using <code class="font-mono">sudo tee</code>?</span>
        <button class="btn-secondary px-2 py-1 text-xs" onclick={() => (bar = null)}>Cancel</button>
        <button class="btn-primary px-2 py-1 text-xs" onclick={() => void saveAsRoot(null)}>Save with sudo</button>
      </div>
    {:else if bar?.kind === "sudo-password"}
      <form
        class="flex flex-wrap items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2 text-xs text-warning"
        data-testid="sudo-password-bar"
        onsubmit={(e) => {
          e.preventDefault();
          void saveAsRoot(sudoPassword);
        }}
      >
        <KeyRound size={14} class="shrink-0" />
        <label class="shrink-0" for="sudo-pw">{bar.error ?? "sudo needs your password on the host"}</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="sudo-pw" class="input max-w-56 py-1 text-xs" type="password" autocomplete="off" autofocus placeholder="Password for sudo" bind:value={sudoPassword} required />
        <button class="btn-secondary px-2 py-1 text-xs" type="button" onclick={() => { bar = null; sudoPassword = ""; }}>Cancel</button>
        <button class="btn-primary px-2 py-1 text-xs" type="submit" disabled={saving || !sudoPassword}>Save with sudo</button>
      </form>
    {:else if bar?.kind === "changed"}
      <div class="flex flex-wrap items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2 text-xs text-warning" role="alert" data-testid="changed-bar">
        <ShieldAlert size={14} class="shrink-0" />
        <span class="min-w-0 flex-1">This file was changed on the host after you opened it. Saving now would overwrite that change.</span>
        <button class="btn-secondary px-2 py-1 text-xs" onclick={() => void reload()}>Reload from host</button>
        <button class="btn-primary px-2 py-1 text-xs" onclick={() => void save(true)}>Overwrite anyway</button>
      </div>
    {:else if bar?.kind === "error"}
      <div class="flex items-center gap-3 border-b border-danger/30 bg-danger/10 px-4 py-2 text-xs text-danger" role="alert" data-testid="editor-error">
        <span class="min-w-0 flex-1 break-words">{bar.text}</span>
        <button class="icon-btn h-6 w-6" aria-label="Dismiss" onclick={() => (bar = null)}><X size={12} /></button>
      </div>
    {:else if bar?.kind === "saved"}
      <div class="border-b border-success/30 bg-success/10 px-4 py-1 text-xs text-success" role="status" data-testid="editor-saved">Saved.</div>
    {/if}

    <div class="relative min-h-0 flex-1">
      {#if loading}
        <div class="flex h-full items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading the file…</div>
      {:else if loadError}
        <div class="flex h-full items-center justify-center p-8 text-center text-sm text-danger" role="alert" data-testid="editor-load-error">{loadError}</div>
      {/if}
      <div bind:this={host_el} class="h-full {loading || loadError ? 'hidden' : ''}"></div>
    </div>

    <footer class="flex items-center gap-4 border-t border-line bg-panel px-4 py-1 font-mono text-[11px] tabular-nums text-fg-muted">
      <span>Ln {cursor.line}, Col {cursor.col}</span>
      <span>{lines} lines</span>
      {#if file}<span>{file.size} bytes</span>{/if}
      <span class="flex-1"></span>
      {#if file?.crlf}<span>CRLF</span>{/if}
      <span data-testid="editor-lang">{LANGUAGE_LABEL[lang]}</span>
      <span>UTF-8</span>
    </footer>
  </div>
</div>
