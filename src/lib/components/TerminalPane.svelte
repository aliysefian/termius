<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ChevronDown, ChevronUp, Loader2, RefreshCw, Search, ShieldAlert, ShieldX, Unplug, X } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import { ssh, type Credentials, type SessionStatus } from "$lib/ssh";
  import { settings } from "$lib/stores/settings.svelte";
  import { adhocLabel, ui, type Pane } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { themeById } from "$lib/themes";
  import { errorMessage } from "$lib/types";

  let { pane, active = true }: { pane: Pane; active?: boolean } = $props();

  // Target and id never change for a mounted pane (panes are keyed by id).
  // svelte-ignore state_referenced_locally
  const target = pane.target;
  // svelte-ignore state_referenced_locally
  const paneId = pane.id;
  const host = $derived(target.kind === "host" ? vaultStore.hostById.get(target.hostId)?.data : undefined);
  const label = $derived(target.kind === "host" ? (host?.label ?? "host") : adhocLabel(target.adhoc));
  const needsCredentials = $derived(target.kind === "host" && !!host && !host.identity_id);

  let container: HTMLDivElement;
  // Created once in onMount; the appearance effect re-reads it on each run.
  // svelte-ignore non_reactive_update
  let term: Terminal;
  let fit: FitAddon;
  let search: SearchAddon;
  let unlisten: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let status = $state<SessionStatus>({ kind: "disconnected", code: null });
  let hostKeyNotices = $state<{ host: string; fingerprint: string }[]>([]);
  let started = $state(false);
  let username = $state("");
  let password = $state("");

  let findOpen = $state(false);
  let findQuery = $state("");
  let findInput = $state<HTMLInputElement>();
  let menu = $state<{ x: number; y: number } | null>(null);

  function setInfo(s: SessionStatus["kind"]) {
    ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? {}), status: s };
  }

  function safeFit() {
    // Skip while hidden (another view is showing); xterm would otherwise
    // shrink to 0x0 and resize the remote PTY to nothing.
    if (container && container.clientWidth > 0 && container.clientHeight > 0) fit?.fit();
  }

  async function connect(credentials: Credentials | null) {
    if (target.kind === "host" && !host) return;
    started = true;
    hostKeyNotices = [];
    status = { kind: "connecting" };
    setInfo("connecting");
    term.clear();
    safeFit();
    const onData = (bytes: Uint8Array) => term.write(bytes);
    try {
      if (target.kind === "host") {
        await ssh.connect(paneId, target.hostId, term.cols, term.rows, credentials, onData);
      } else {
        await ssh.connectAdhoc(paneId, target.adhoc, term.cols, term.rows, onData);
      }
    } catch (e) {
      status = { kind: "error", message: errorMessage(e) };
      setInfo("error");
    }
  }

  function submitCredentials(e: SubmitEvent) {
    e.preventDefault();
    const creds = { username, password };
    password = "";
    connect(creds);
  }

  function reconnect() {
    if (needsCredentials) started = false;
    else connect(null);
  }

  async function trustNewKey(h: string, port: number) {
    try {
      await api.knownHosts.forget(h, port);
      reconnect();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // -- clipboard --------------------------------------------------------

  async function copySelection() {
    const sel = term.getSelection();
    if (!sel) return;
    try {
      await writeText(sel);
    } catch (e) {
      ui.notify("error", `Copy failed: ${errorMessage(e)}`);
    }
  }

  async function paste() {
    if (status.kind !== "connected") return;
    try {
      const text = await readText();
      // term.paste handles bracketed-paste mode and newline normalisation.
      if (text) term.paste(text);
    } catch (e) {
      ui.notify("error", `Paste failed: ${errorMessage(e)}`);
    }
  }

  // -- find ---------------------------------------------------------------

  const searchDecorations = {
    matchBackground: "#7b61ff55",
    activeMatchBackground: "#7b61ff",
    matchOverviewRuler: "#7b61ff",
    activeMatchColorOverviewRuler: "#ffffff",
  };

  function openFind() {
    findOpen = true;
    const sel = term?.getSelection();
    if (sel && !sel.includes("\n")) findQuery = sel;
    queueMicrotask(() => findInput?.select());
  }

  function closeFind() {
    findOpen = false;
    search?.clearDecorations();
    term?.focus();
  }

  function findNext(backwards = false) {
    if (!findQuery) return;
    const opts = { decorations: searchDecorations, incremental: false };
    if (backwards) search.findPrevious(findQuery, opts);
    else search.findNext(findQuery, opts);
  }

  // -- lifecycle ----------------------------------------------------------

  onMount(async () => {
    const p = settings.prefs;
    term = new Terminal({
      theme: themeById(p.themeId).theme,
      fontFamily: p.fontFamily,
      fontSize: p.fontSize,
      lineHeight: p.lineHeight,
      cursorStyle: p.cursorStyle,
      cursorBlink: p.cursorBlink,
      scrollback: p.scrollback,
      allowProposedApi: true,
      macOptionIsMeta: true,
      rightClickSelectsWord: false,
    });
    fit = new FitAddon();
    search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new WebLinksAddon());
    term.open(container);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // Falls back to the DOM renderer.
    }
    safeFit();

    // Terminal-local shortcuts. Returning false stops xterm from sending
    // the key to the remote shell.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
      switch (e.code) {
        case "KeyC":
          void copySelection();
          return false;
        case "KeyV":
          void paste();
          return false;
        case "KeyF":
          openFind();
          return false;
      }
      return true;
    });

    term.onData((d) => {
      if (status.kind === "connected") void ssh.write(paneId, d);
    });
    term.onBinary((d) => {
      if (status.kind === "connected") void ssh.write(paneId, Uint8Array.from(d, (c) => c.charCodeAt(0)));
    });
    term.onResize(({ cols, rows }) => {
      if (status.kind === "connected") void ssh.resize(paneId, cols, rows);
    });
    term.onSelectionChange(() => {
      if (settings.prefs.copyOnSelect && term.hasSelection()) void copySelection();
    });
    term.onTitleChange((title) => {
      ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), remoteTitle: title };
    });

    resizeObserver = new ResizeObserver(safeFit);
    resizeObserver.observe(container);

    unlisten = await ssh.onStatus((e) => {
      if (e.pane_id !== paneId) return;
      if (e.status.kind === "new_host_key") {
        hostKeyNotices = [...hostKeyNotices, { host: e.status.host, fingerprint: e.status.fingerprint }];
        return;
      }
      const wasConnected = status.kind === "connected";
      status = e.status;
      setInfo(e.status.kind);
      if (e.status.kind === "connected") {
        safeFit();
        term.focus();
        if (target.kind === "host") settings.markRecent(target.hostId);
      } else if (e.status.kind === "disconnected") {
        term.write(`\r\n\x1b[90m[session closed${e.status.code != null ? `, exit ${e.status.code}` : ""}]\x1b[0m\r\n`);
        if (wasConnected && !active) ui.notify("info", `Session to ${label} closed.`);
      }
    });

    setInfo("connecting");
    if (!needsCredentials) connect(null);
  });

  onDestroy(() => {
    unlisten?.();
    resizeObserver?.disconnect();
    void ssh.disconnect(paneId);
    term?.dispose();
  });

  // Apply appearance changes from Settings to the live terminal.
  $effect(() => {
    const p = settings.prefs;
    const theme = themeById(p.themeId).theme;
    const { fontFamily, fontSize, lineHeight, cursorStyle, cursorBlink, scrollback } = p;
    if (!term) return;
    term.options.theme = theme;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    term.options.lineHeight = lineHeight;
    term.options.cursorStyle = cursorStyle;
    term.options.cursorBlink = cursorBlink;
    term.options.scrollback = scrollback;
    safeFit();
  });

  $effect(() => {
    if (active && status.kind === "connected") term?.focus();
  });

  // Global "find" shortcut targets the active pane only.
  let lastFind = ui.findRequest;
  $effect(() => {
    const n = ui.findRequest;
    if (n !== lastFind) {
      lastFind = n;
      if (active) openFind();
    }
  });

  const background = $derived(themeById(settings.prefs.themeId).theme.background);
</script>

<div class="relative flex min-h-0 flex-1 flex-col" style:background>
  <div
    class="min-h-0 flex-1 p-1"
    bind:this={container}
    role="presentation"
    oncontextmenu={(e) => {
      e.preventDefault();
      menu = { x: e.clientX, y: e.clientY };
    }}
  ></div>

  {#if findOpen}
    <div class="absolute right-3 top-2 z-10 flex items-center gap-1 rounded-md border border-line bg-panel px-2 py-1 shadow-lg">
      <Search size={13} class="text-fg-muted" />
      <input
        bind:this={findInput}
        class="w-48 bg-transparent px-1 py-0.5 text-xs outline-none"
        placeholder="Find"
        bind:value={findQuery}
        oninput={() => findQuery && search.findNext(findQuery, { decorations: searchDecorations, incremental: true })}
        onkeydown={(e) => {
          if (e.key === "Enter") findNext(e.shiftKey);
          else if (e.key === "Escape") closeFind();
        }}
      />
      <button class="icon-btn h-6 w-6" title="Previous (Shift+Enter)" onclick={() => findNext(true)}><ChevronUp size={13} /></button>
      <button class="icon-btn h-6 w-6" title="Next (Enter)" onclick={() => findNext()}><ChevronDown size={13} /></button>
      <button class="icon-btn h-6 w-6" title="Close (Esc)" onclick={closeFind}><X size={13} /></button>
    </div>
  {/if}

  {#if menu}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
    <div class="fixed z-50 w-52 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
      {#each [
        { label: "Copy", keys: "Ctrl+Shift+C", run: copySelection, disabled: !term?.hasSelection() },
        { label: "Paste", keys: "Ctrl+Shift+V", run: paste, disabled: status.kind !== "connected" },
        { label: "Select all", keys: "", run: () => term.selectAll(), disabled: false },
        { label: "Find…", keys: "Ctrl+Shift+F", run: openFind, disabled: false },
        { label: "Clear scrollback", keys: "", run: () => term.clear(), disabled: false },
      ] as item (item.label)}
        <button
          class="flex w-full items-center justify-between px-3 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 disabled:hover:bg-transparent"
          role="menuitem"
          disabled={item.disabled}
          onclick={() => {
            menu = null;
            void item.run();
            term.focus();
          }}
        >
          <span>{item.label}</span>
          <span class="text-xs text-fg-muted">{item.keys}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if hostKeyNotices.length}
    <div class="absolute inset-x-2 top-2 flex items-start gap-2 rounded-md border border-line bg-panel/95 px-3 py-2 text-xs shadow-lg">
      <ShieldAlert size={14} class="mt-0.5 shrink-0 text-accent" />
      <div class="min-w-0 flex-1 space-y-1">
        {#each hostKeyNotices as k (k.host)}
          <div>
            <div class="font-medium">New host key recorded for {k.host}</div>
            <div class="truncate font-mono text-fg-muted">{k.fingerprint}</div>
          </div>
        {/each}
      </div>
      <button class="text-fg-muted hover:text-fg" aria-label="Dismiss" onclick={() => (hostKeyNotices = [])}>✕</button>
    </div>
  {/if}

  {#if !started && needsCredentials}
    <div class="absolute inset-0 flex items-center justify-center bg-base/90">
      <form onsubmit={submitCredentials} class="w-72 space-y-3 rounded-xl border border-line bg-panel p-5">
        <div class="text-sm font-semibold">Credentials for {label}</div>
        <p class="text-xs text-fg-muted">This host has no identity. Enter one-time credentials.</p>
        <input class="input font-mono" placeholder="username" bind:value={username} required autocomplete="username" />
        <input class="input" type="password" placeholder="password" bind:value={password} required autocomplete="current-password" />
        <button class="btn-primary w-full" type="submit">Connect</button>
      </form>
    </div>
  {:else if status.kind === "connecting"}
    <div class="pointer-events-none absolute inset-0 flex items-center justify-center bg-base/70">
      <div class="flex items-center gap-2 rounded-md bg-panel px-4 py-2 text-sm text-fg-muted">
        <Loader2 size={16} class="animate-spin text-accent" /> Connecting to {label}…
      </div>
    </div>
  {:else if status.kind === "host_key_changed"}
    <div class="absolute inset-0 flex items-center justify-center bg-base/90 p-4">
      <div class="w-full max-w-md rounded-xl border border-danger/40 bg-panel p-5">
        <div class="mb-2 flex items-center gap-2 text-sm font-semibold text-danger">
          <ShieldX size={18} /> Host key changed for {status.host}{status.port !== 22 ? `:${status.port}` : ""}
        </div>
        <p class="text-xs text-fg-muted">
          The server presented a different key from the one pinned earlier. This happens when a server is
          reinstalled, but it is also exactly what a man-in-the-middle attack looks like. Only trust the new key
          if you know why it changed.
        </p>
        <div class="mt-3 rounded-md bg-base px-3 py-2 font-mono text-xs break-all">{status.fingerprint}</div>
        <div class="mt-4 flex justify-end gap-2">
          <button class="btn-ghost" onclick={() => (status = { kind: "disconnected", code: null })}>Cancel</button>
          <button class="btn border border-danger/40 text-danger hover:bg-danger/10" onclick={() => status.kind === "host_key_changed" && trustNewKey(status.host, status.port)}>
            Trust new key and reconnect
          </button>
        </div>
      </div>
    </div>
  {:else if status.kind === "error" || (status.kind === "disconnected" && started)}
    <div class="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-line bg-panel px-4 py-2 text-xs">
      {#if status.kind === "error"}
        <ShieldAlert size={14} class="shrink-0 text-danger" />
        <span class="min-w-0 flex-1 truncate text-danger" title={status.message}>{status.message}</span>
      {:else}
        <Unplug size={14} class="shrink-0 text-fg-muted" />
        <span class="flex-1 text-fg-muted">Disconnected</span>
      {/if}
      <button class="btn-ghost py-1" onclick={reconnect}>
        <RefreshCw size={12} /> Reconnect
      </button>
    </div>
  {/if}
</div>
