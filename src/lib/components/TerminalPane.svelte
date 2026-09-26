<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ChevronDown, ChevronUp, KeyRound, Loader2, RefreshCw, Search, ShieldAlert, ShieldX, Unplug, X } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import { ssh, type Credentials, type SessionStatus } from "$lib/ssh";
  import { closePane, resizePane, writeToPane } from "$lib/terminalio";
  import { hostContextFor } from "$lib/runsnippet";
  import { render } from "$lib/snippetvars";
  import { settings } from "$lib/stores/settings.svelte";
  import { adhocLabel, ui, type Pane } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { themeById } from "$lib/themes";
  import { errorMessage } from "$lib/types";
  import { LineTracker, matchDestructive, pastedLines, pasteNeedsConfirm } from "$lib/guard";

  let {
    pane,
    active = true,
    broadcast = null,
  }: {
    pane: Pane;
    active?: boolean;
    /** When set, input goes here (to every pane in the tab) instead of just this pane. */
    broadcast?: ((data: string | Uint8Array) => void) | null;
  } = $props();

  // Target and id never change for a mounted pane (panes are keyed by id).
  // svelte-ignore state_referenced_locally
  const target = pane.target;
  // svelte-ignore state_referenced_locally
  const paneId = pane.id;
  const host = $derived(target.kind === "host" ? vaultStore.hostById.get(target.hostId)?.data : undefined);
  const label = $derived(
    target.kind === "host" ? (host?.label ?? "host") : target.kind === "adhoc" ? adhocLabel(target.adhoc) : "Local",
  );
  const needsCredentials = $derived(target.kind === "host" && !!host && !vaultStore.effectiveIdentity(host));
  const production = $derived(vaultStore.effectiveEnv(host) === "production");
  const shared = $derived(vaultStore.settings?.settings);

  // -- input guards (a safety net, not a guarantee) -------------------------
  const tracker = new LineTracker();
  type Pending =
    | { kind: "paste"; text: string; lines: number; danger: string | null }
    | { kind: "command"; line: string; pattern: string };
  let pending = $state<Pending | null>(null);

  function send(d: string | Uint8Array) {
    if (broadcast) broadcast(d);
    else void writeToPane(pane, d);
  }

  /** Typed input: hold Enter on production when the line looks destructive. */
  function typed(d: string) {
    if (production && shared && (d === "\r" || d === "\n")) {
      const line = tracker.line;
      const pattern = line === null ? null : matchDestructive(line, shared.destructive_patterns);
      if (line !== null && pattern) {
        pending = { kind: "command", line, pattern };
        return;
      }
    }
    tracker.feed(d);
    send(d);
  }

  /** Every paste path (menu, shortcut, native Ctrl+V) goes through here. */
  function guardedPaste(text: string) {
    if (!text || status.kind !== "connected") return;
    const lines = pastedLines(text);
    const danger = production && shared ? text.split(/\r\n|\r|\n/).map((l) => matchDestructive(l, shared.destructive_patterns)).find(Boolean) ?? null : null;
    if (danger || pasteNeedsConfirm(text, shared?.paste_confirm_lines ?? 2, production)) {
      pending = { kind: "paste", text, lines, danger };
      return;
    }
    doPaste(text);
  }

  function doPaste(text: string) {
    // term.paste handles bracketed-paste mode and newline normalisation.
    tracker.reset();
    term.paste(text);
  }

  function resolvePending(go: boolean) {
    const p = pending;
    pending = null;
    if (go && p?.kind === "paste") doPaste(p.text);
    else if (go && p?.kind === "command") {
      tracker.reset();
      send("\r");
    }
    term?.focus();
  }

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
  let remember = $state(true);
  let credentialError = $state<string | null>(null);

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
      } else if (target.kind === "adhoc") {
        await ssh.connectAdhoc(paneId, target.adhoc, term.cols, term.rows, onData);
      } else {
        await api.localTerm.spawn(paneId, term.cols, term.rows, onData);
      }
    } catch (e) {
      status = { kind: "error", message: errorMessage(e) };
      setInfo("error");
    }
  }

  async function submitCredentials(e: SubmitEvent) {
    e.preventDefault();
    credentialError = null;
    const creds = { username, password };
    password = "";
    if (remember && target.kind === "host" && host) {
      // Store them as this host's own credentials, then connect normally.
      try {
        await vaultStore.saveHostWithCredentials(target.hostId, $state.snapshot(host), {
          mode: "inline",
          username: creds.username,
          auth: { type: "password", password: creds.password },
          save_to_keychain: null,
        });
        connect(null);
      } catch (err) {
        credentialError = errorMessage(err);
      }
      return;
    }
    connect(creds);
  }

  function reconnect() {
    if (needsCredentials) started = false;
    else connect(null);
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
      if (text) guardedPaste(text);
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

    // Native paste (Ctrl+V, middle click): intercept before xterm sends it.
    container.addEventListener(
      "paste",
      (e) => {
        const text = e.clipboardData?.getData("text/plain") ?? "";
        e.preventDefault();
        e.stopImmediatePropagation();
        guardedPaste(text);
      },
      { capture: true },
    );

    term.onData((d) => {
      if (status.kind !== "connected" || pending) return;
      typed(d);
    });
    term.onBinary((d) => {
      if (status.kind !== "connected") return;
      const bytes = Uint8Array.from(d, (c) => c.charCodeAt(0));
      if (broadcast) broadcast(bytes);
      else void writeToPane(pane, bytes);
    });
    term.onResize(({ cols, rows }) => {
      if (status.kind === "connected") void resizePane(pane, cols, rows);
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
        if (target.kind === "host") {
          settings.markRecent(target.hostId);
          const startup = host?.startup_command?.trim();
          // Give the remote shell a moment to print its prompt first.
          if (startup) setTimeout(() => void writeToPane(pane, render(startup, hostContextFor(target.hostId), {}) + "\r"), 300);
        }
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
    void closePane(pane);
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
  {#if production && target.kind === "host"}
    <div class="flex shrink-0 items-center gap-2 bg-danger px-3 py-0.5 text-[11px] font-semibold tracking-wide text-white" role="status">
      <ShieldAlert size={12} /> PRODUCTION · {label}
    </div>
  {/if}
  <!-- `isolate` keeps xterm's internal z-indexed layers (up to 11) inside
       this box, so the overlays below always sit on top and stay clickable. -->
  <div
    class="relative isolate z-0 min-h-0 flex-1 p-1"
    bind:this={container}
    role="presentation"
    oncontextmenu={(e) => {
      e.preventDefault();
      menu = { x: e.clientX, y: e.clientY };
    }}
  ></div>

  {#if findOpen}
    <div class="absolute right-3 top-2 z-20 flex items-center gap-1 rounded-md border border-line bg-panel px-2 py-1 shadow-lg">
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

  {#if pending}
    <div class="absolute inset-0 z-40 flex items-center justify-center bg-base/80 p-4" role="presentation">
      <div class="w-full max-w-lg rounded-xl border {pending.kind === 'command' || (pending.kind === 'paste' && pending.danger) ? 'border-danger/50' : 'border-line'} bg-panel p-5 shadow-2xl" role="alertdialog" aria-modal="true" aria-label="Confirm input">
        {#if pending.kind === "command"}
          <div class="mb-2 flex items-center gap-2 text-sm font-semibold text-danger"><ShieldAlert size={16} /> Run this on production?</div>
          <pre class="max-h-32 overflow-auto rounded-md bg-base px-3 py-2 font-mono text-xs whitespace-pre-wrap">{pending.line}</pre>
          <p class="mt-2 text-xs text-fg-muted">It matches a destructive-command pattern for <strong>{label}</strong>. This check only sees what you typed; it's a reminder, not a guarantee.</p>
        {:else}
          <div class="mb-2 flex items-center gap-2 text-sm font-semibold {pending.danger ? 'text-danger' : ''}">
            <ShieldAlert size={16} /> Paste {pending.lines} line{pending.lines === 1 ? "" : "s"} into {label}?
          </div>
          <pre class="max-h-48 overflow-auto rounded-md bg-base px-3 py-2 font-mono text-xs whitespace-pre-wrap">{pending.text.length > 4000 ? `${pending.text.slice(0, 4000)}
…` : pending.text}</pre>
          <p class="mt-2 text-xs text-fg-muted">
            {pending.danger ? "It contains a command that looks destructive, and this is a production host. " : ""}Each line runs as soon as it's pasted.
          </p>
        {/if}
        <div class="mt-4 flex justify-end gap-2">
          <!-- svelte-ignore a11y_autofocus -->
          <button class="btn-primary" autofocus onclick={() => resolvePending(false)}>Cancel</button>
          <button class="btn-ghost border {pending.kind === 'command' || (pending.kind === 'paste' && pending.danger) ? 'border-danger/50 text-danger' : 'border-line'}" onclick={() => resolvePending(true)}>
            {pending.kind === "command" ? "Run it" : "Paste"}
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if hostKeyNotices.length}
    <div class="absolute inset-x-2 top-2 z-10 flex items-start gap-2 rounded-md border border-line bg-panel/95 px-3 py-2 text-xs shadow-lg">
      <ShieldAlert size={14} class="mt-0.5 shrink-0 text-accent" />
      <div class="min-w-0 flex-1 space-y-1">
        {#each hostKeyNotices as k (k.host)}
          <div>
            <div class="font-medium">Host key trusted for {k.host}</div>
            <div class="truncate font-mono text-fg-muted">{k.fingerprint}</div>
          </div>
        {/each}
      </div>
      <button class="text-fg-muted hover:text-fg" aria-label="Dismiss" onclick={() => (hostKeyNotices = [])}>✕</button>
    </div>
  {/if}

  {#if !started && needsCredentials}
    <div class="absolute inset-0 z-30 flex items-center justify-center bg-base/95 p-4">
      <form onsubmit={submitCredentials} class="w-full max-w-sm space-y-4 rounded-xl border border-line bg-panel p-6 shadow-2xl">
        <div class="flex items-center gap-3">
          <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-accent"><KeyRound size={18} /></div>
          <div class="min-w-0">
            <div class="truncate text-sm font-semibold">Log in to {label}</div>
            <div class="truncate font-mono text-xs text-fg-muted">{host?.hostname}{host && host.port !== 22 ? `:${host.port}` : ""}</div>
          </div>
        </div>
        <div>
          <label class="label" for="c-user-{paneId}">Username</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="c-user-{paneId}" class="input font-mono" bind:value={username} required autocomplete="username" spellcheck="false" autofocus />
        </div>
        <div>
          <label class="label" for="c-pw-{paneId}">Password</label>
          <input id="c-pw-{paneId}" class="input" type="password" bind:value={password} required autocomplete="current-password" />
        </div>
        <label class="flex items-center gap-2 text-xs text-fg-muted">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={remember} />
          Remember for this host (encrypted in your vault)
        </label>
        {#if credentialError}
          <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{credentialError}</p>
        {/if}
        <div class="flex gap-2">
          <button class="btn-ghost flex-1 border border-line" type="button" onclick={() => target.kind === "host" && (ui.modal = { kind: "host", id: target.hostId })}>
            Edit host
          </button>
          <button class="btn-primary flex-1" type="submit">Connect</button>
        </div>
      </form>
    </div>
  {:else if status.kind === "connecting"}
    <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-base/70">
      <div class="flex items-center gap-2 rounded-md bg-panel px-4 py-2 text-sm text-fg-muted">
        <Loader2 size={16} class="animate-spin text-accent" /> Connecting to {label}…
      </div>
    </div>
  {:else if status.kind === "host_key_changed"}
    <div class="absolute inset-0 z-30 flex items-center justify-center bg-base/95 p-4">
      <div class="w-full max-w-md rounded-xl border border-danger/40 bg-panel p-5">
        <div class="mb-2 flex items-center gap-2 text-sm font-semibold text-danger">
          <ShieldX size={18} /> Host key changed for {status.host}{status.port !== 22 ? `:${status.port}` : ""}
        </div>
        <p class="text-xs text-fg-muted">
          The server presented a different key from the one trusted before, so the connection was stopped and nothing was
          sent. This happens when a server is reinstalled, but it is also exactly what a man-in-the-middle attack looks
          like. Reconnect to compare the old and new fingerprints and decide.
        </p>
        <div class="mt-3 rounded-md bg-base px-3 py-2 font-mono text-xs break-all">{status.fingerprint}</div>
        <div class="mt-4 flex justify-end gap-2">
          <button class="btn-ghost" onclick={() => (status = { kind: "disconnected", code: null })}>Close</button>
          <button class="btn border border-danger/40 text-danger hover:bg-danger/10" onclick={reconnect}>Review and reconnect</button>
        </div>
      </div>
    </div>
  {:else if status.kind === "error" || (status.kind === "disconnected" && started)}
    <div class="absolute inset-x-0 bottom-0 z-10 flex items-center gap-3 border-t border-line bg-panel px-4 py-2 text-xs">
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
