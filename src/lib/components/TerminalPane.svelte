<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { Loader2, RefreshCw, ShieldAlert, Unplug } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { ssh, type Credentials, type SessionStatus } from "$lib/ssh";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { paneId, hostId, active = true }: { paneId: string; hostId: Uuid; active?: boolean } = $props();

  const host = $derived(vaultStore.hostById.get(hostId)?.data);
  const needsCredentials = $derived(!!host && !host.identity_id);

  let container: HTMLDivElement;
  let term: Terminal;
  let fit: FitAddon;
  let unlisten: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let status = $state<SessionStatus>({ kind: "disconnected", code: null });
  let hostKeyNotices = $state<{ host: string; fingerprint: string }[]>([]);
  let started = $state(false);
  let username = $state("");
  let password = $state("");

  const theme = {
    background: "#18191e",
    foreground: "#e6e6ea",
    cursor: "#7b61ff",
    cursorAccent: "#18191e",
    selectionBackground: "rgba(123, 97, 255, 0.35)",
    black: "#20222b",
    red: "#ff5c5c",
    green: "#3ddc84",
    yellow: "#ffb020",
    blue: "#5b8def",
    magenta: "#b678ff",
    cyan: "#38bdf8",
    white: "#e6e6ea",
    brightBlack: "#8b8d98",
    brightRed: "#ff7b7b",
    brightGreen: "#5ff29c",
    brightYellow: "#ffc855",
    brightBlue: "#7fa8ff",
    brightMagenta: "#cf9dff",
    brightCyan: "#6ad4ff",
    brightWhite: "#ffffff",
  };

  async function connect(credentials: Credentials | null) {
    if (!host) return;
    started = true;
    hostKeyNotices = [];
    status = { kind: "connecting" };
    term.clear();
    fit.fit();
    try {
      await ssh.connect(paneId, hostId, term.cols, term.rows, credentials, (bytes) => term.write(bytes));
    } catch (e) {
      status = { kind: "error", message: errorMessage(e) };
    }
  }

  function submitCredentials(e: SubmitEvent) {
    e.preventDefault();
    const creds = { username, password };
    password = "";
    connect(creds);
  }

  onMount(async () => {
    term = new Terminal({
      theme,
      fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, Menlo, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
      macOptionIsMeta: true,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(container);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // Canvas renderer fallback is automatic.
    }
    fit.fit();

    term.onData((d) => {
      if (status.kind === "connected") void ssh.write(paneId, d);
    });
    term.onBinary((d) => {
      if (status.kind === "connected") void ssh.write(paneId, Uint8Array.from(d, (c) => c.charCodeAt(0)));
    });
    term.onResize(({ cols, rows }) => {
      if (status.kind === "connected") void ssh.resize(paneId, cols, rows);
    });

    // Skip fitting while hidden (another view is showing); xterm would
    // otherwise shrink to 0x0 and resize the remote PTY to nothing.
    resizeObserver = new ResizeObserver(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) fit.fit();
    });
    resizeObserver.observe(container);

    unlisten = await ssh.onStatus((e) => {
      if (e.pane_id !== paneId) return;
      if (e.status.kind === "new_host_key") {
        hostKeyNotices = [...hostKeyNotices, { host: e.status.host, fingerprint: e.status.fingerprint }];
        return;
      }
      status = e.status;
      if (e.status.kind === "connected") {
        fit.fit();
        term.focus();
      } else if (e.status.kind === "disconnected") {
        term.write(`\r\n\x1b[90m[session closed${e.status.code != null ? `, exit ${e.status.code}` : ""}]\x1b[0m\r\n`);
      }
    });

    if (!needsCredentials) connect(null);
  });

  onDestroy(() => {
    unlisten?.();
    resizeObserver?.disconnect();
    void ssh.disconnect(paneId);
    term?.dispose();
  });

  $effect(() => {
    if (active && status.kind === "connected") term?.focus();
  });
</script>

<div class="relative flex min-h-0 flex-1 flex-col">
  <div class="min-h-0 flex-1 p-1" bind:this={container}></div>

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
        <div class="text-sm font-semibold">Credentials for {host?.label}</div>
        <p class="text-xs text-fg-muted">This host has no identity. Enter one-time credentials.</p>
        <input class="input font-mono" placeholder="username" bind:value={username} required autocomplete="username" />
        <input class="input" type="password" placeholder="password" bind:value={password} required autocomplete="current-password" />
        <button class="btn-primary w-full" type="submit">Connect</button>
      </form>
    </div>
  {:else if status.kind === "connecting"}
    <div class="pointer-events-none absolute inset-0 flex items-center justify-center bg-base/70">
      <div class="flex items-center gap-2 rounded-md bg-panel px-4 py-2 text-sm text-fg-muted">
        <Loader2 size={16} class="animate-spin text-accent" /> Connecting to {host?.hostname}…
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
      <button class="btn-ghost py-1" onclick={() => (needsCredentials ? (started = false) : connect(null))}>
        <RefreshCw size={12} /> Reconnect
      </button>
    </div>
  {/if}
</div>
