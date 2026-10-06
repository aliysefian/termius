<script lang="ts">
  // A remote desktop in a tab: connects, asks about the server's certificate,
  // and shows the screen. Today that means RDP.
  import { onDestroy, onMount } from "svelte";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ClipboardCopy, KeyRound, Loader2, Maximize, Minimize, MonitorOff, RefreshCw, ShieldAlert } from "lucide-svelte";
  import * as api from "$lib/api";
  import type { Credentials } from "$lib/ssh";
  import { ask } from "$lib/dialogs.svelte";
  import { decodeMessage, fingerprintLines, usableSize } from "$lib/rdp";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui, type Pane } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, isApiError, type RdpCertificate } from "$lib/types";
  import RemoteDisplay from "./RemoteDisplay.svelte";

  let { pane, active = true }: { pane: Pane; active?: boolean } = $props();

  // svelte-ignore state_referenced_locally
  const target = pane.target;
  // svelte-ignore state_referenced_locally
  const paneId = pane.id;
  const hostId = target.kind === "host" ? target.hostId : "";
  const host = $derived(vaultStore.hostById.get(hostId)?.data);
  const label = $derived(host?.label ?? "host");
  const needsCredentials = $derived(!!host && !vaultStore.effectiveIdentity(host));

  type Status = { kind: "idle" } | { kind: "connecting" } | { kind: "connected" } | { kind: "error"; message: string } | { kind: "disconnected" };
  let status = $state<Status>({ kind: "idle" });
  let display = $state<ReturnType<typeof RemoteDisplay>>();
  let area = $state<HTMLDivElement>();
  let started = $state(false);
  let actualSize = $state(false);
  let shareClipboard = $state(true);
  let lastLocalClipboard = "";
  let username = $state("");
  let password = $state("");
  let remember = $state(true);
  let credentialError = $state<string | null>(null);

  function setInfo(kind: "connecting" | "connected" | "error" | "disconnected") {
    ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? {}), status: kind };
  }

  const certificateText = (c: RdpCertificate) =>
    `Subject: ${c.subject}\nIssued by: ${c.issuer}\nValid: ${c.not_before} to ${c.not_after}\nSHA-256:\n${fingerprintLines(c.fingerprint)}`;

  /** Ask about a certificate the server presented. True to go ahead, with the fingerprint to accept. */
  async function askAboutCertificate(e: unknown): Promise<string | null> {
    if (!isApiError(e)) return null;
    if (e.code === "rdp_certificate_unknown") {
      const c = e.details as RdpCertificate;
      const ok = await ask(
        `${label} has not been seen before. Check that this is the certificate you expect before trusting it; it is remembered for this host, and a different one later is refused until you say otherwise.\n\n${certificateText(c)}`,
        { title: "Trust this server's certificate?", confirm: "Trust and connect" },
      );
      return ok ? c.fingerprint : null;
    }
    if (e.code === "rdp_certificate_changed") {
      const d = e.details as { expected: string; found: RdpCertificate };
      const ok = await ask(
        `The certificate ${label} presents is not the one trusted before. This happens when the server's certificate is renewed or the server is reinstalled, but it is also what someone intercepting the connection looks like. Nothing was sent to it.\n\nTrusted before:\n${fingerprintLines(d.expected)}\n\nNow presented:\n${certificateText(d.found)}`,
        { title: "The server's certificate changed", confirm: "Trust the new certificate", danger: true },
      );
      return ok ? d.found.fingerprint : null;
    }
    return null;
  }

  function onMessage(bytes: Uint8Array) {
    const m = decodeMessage(bytes);
    if (!m) return;
    switch (m.kind) {
      case "frame":
        display?.draw(m.x, m.y, m.width, m.height, m.rgba);
        break;
      case "size":
        display?.resize(m.width, m.height);
        break;
      case "cursor-default":
        display?.setCursor("default");
        break;
      case "cursor-hidden":
        display?.setCursor("hidden");
        break;
      case "cursor":
        display?.setCursor(m);
        break;
      case "clipboard":
        if (shareClipboard && m.text && m.text !== lastLocalClipboard) {
          lastLocalClipboard = m.text;
          void writeText(m.text).catch(() => {});
        }
        break;
      case "ended":
        if (m.error) {
          status = { kind: "error", message: m.error };
          setInfo("error");
        } else {
          status = { kind: "disconnected" };
          setInfo("disconnected");
        }
        break;
    }
  }

  async function connect(credentials: Credentials | null, accept: string | null = null) {
    if (!host || !area) return;
    started = true;
    status = { kind: "connecting" };
    setInfo("connecting");
    const { width, height } = usableSize(area.clientWidth, area.clientHeight);
    try {
      await api.rdp.connect(paneId, hostId, width, height, credentials, accept, onMessage);
      status = { kind: "connected" };
      setInfo("connected");
      settings.markRecent(hostId);
      display?.focus();
      void sendLocalClipboard();
    } catch (e) {
      const fingerprint = await askAboutCertificate(e);
      if (fingerprint) return connect(credentials, fingerprint);
      status = { kind: "error", message: errorMessage(e) };
      setInfo("error");
    }
  }

  async function submitCredentials(e: SubmitEvent) {
    e.preventDefault();
    credentialError = null;
    const creds = { username, password };
    password = "";
    if (remember && host) {
      try {
        await vaultStore.saveHostWithCredentials(hostId, $state.snapshot(host), {
          mode: "inline",
          username: creds.username,
          auth: { type: "password", password: creds.password },
          save_to_keychain: null,
        });
        void connect(null);
      } catch (err) {
        credentialError = errorMessage(err);
      }
      return;
    }
    void connect(creds);
  }

  const send = (input: api.RdpInput) => {
    if (status.kind === "connected") void api.rdp.input(paneId, input).catch(() => {});
  };

  /** Offer the local clipboard's text to the remote, when it has changed. */
  async function sendLocalClipboard() {
    if (!shareClipboard || status.kind !== "connected") return;
    try {
      const text = await readText();
      if (text && text !== lastLocalClipboard) {
        lastLocalClipboard = text;
        send({ type: "clipboard_text", text });
      }
    } catch {
      // Nothing readable on the clipboard.
    }
  }

  function reconnect() {
    if (needsCredentials) started = false;
    else void connect(null);
  }

  onMount(() => {
    setInfo("connecting");
    if (!needsCredentials) void connect(null);
  });

  onDestroy(() => {
    void api.rdp.close(paneId).catch(() => {});
  });

  // Coming back to this tab: the keyboard goes to the screen, and the clipboard is offered.
  $effect(() => {
    if (active && status.kind === "connected") {
      display?.focus();
      void sendLocalClipboard();
    }
  });
</script>

<div class="relative flex h-full w-full flex-col bg-black">
  <div class="flex h-7 shrink-0 items-center gap-1 border-b border-line bg-panel px-2 text-xs text-fg-muted">
    <span class="mr-auto truncate">{host ? `${host.hostname}${host.port !== 3389 ? `:${host.port}` : ""}` : ""}</span>
    <button class="btn-ghost h-6 shrink-0 px-2 py-0 text-xs" title="Send Ctrl+Alt+Del" disabled={status.kind !== "connected"} onclick={() => send({ type: "ctrl_alt_del" })}>
      Ctrl+Alt+Del
    </button>
    <button
      class="icon-btn h-6 w-6 {shareClipboard ? 'text-accent' : ''}"
      title={shareClipboard ? "Clipboard text is shared with the remote computer (click to stop)" : "Clipboard sharing is off (click to share text)"}
      aria-pressed={shareClipboard}
      onclick={() => {
        shareClipboard = !shareClipboard;
        if (shareClipboard) void sendLocalClipboard();
      }}
    >
      <ClipboardCopy size={14} />
    </button>
    <button class="icon-btn h-6 w-6" title={actualSize ? "Shrink to fit the tab" : "Show at actual size"} onclick={() => (actualSize = !actualSize)}>
      {#if actualSize}<Minimize size={14} />{:else}<Maximize size={14} />{/if}
    </button>
  </div>

  <div bind:this={area} class="relative min-h-0 flex-1">
    <RemoteDisplay bind:this={display} onInput={send} {actualSize} disabled={status.kind !== "connected"} />

    {#if !started && needsCredentials}
      <div class="absolute inset-0 z-30 flex items-center justify-center bg-base/95 p-4">
        <form onsubmit={submitCredentials} class="w-full max-w-sm space-y-4 rounded-xl border border-line bg-panel p-6 shadow-2xl">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-accent"><KeyRound size={18} /></div>
            <div class="min-w-0">
              <div class="truncate text-sm font-semibold">Sign in to {label}</div>
              <div class="truncate font-mono text-xs text-fg-muted">{host?.hostname}{host && host.port !== 3389 ? `:${host.port}` : ""}</div>
            </div>
          </div>
          <div>
            <label class="label" for="r-user-{paneId}">User name</label>
            <!-- svelte-ignore a11y_autofocus -->
            <input id="r-user-{paneId}" class="input font-mono" bind:value={username} required autocomplete="username" spellcheck="false" autofocus />
          </div>
          <div>
            <label class="label" for="r-pw-{paneId}">Password</label>
            <input id="r-pw-{paneId}" class="input" type="password" bind:value={password} required autocomplete="current-password" />
          </div>
          <label class="flex items-center gap-2 text-xs text-fg-muted">
            <input type="checkbox" class="accent-input" bind:checked={remember} />
            Remember for this host (encrypted in your vault)
          </label>
          {#if credentialError}
            <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{credentialError}</p>
          {/if}
          <div class="flex gap-2">
            <button class="btn-ghost flex-1 border border-line" type="button" onclick={() => (ui.modal = { kind: "host", id: hostId })}>Edit host</button>
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
    {:else if status.kind === "error" || status.kind === "disconnected"}
      <div class="absolute inset-x-0 bottom-0 z-10 flex items-center gap-3 border-t border-line bg-panel px-4 py-2 text-xs">
        {#if status.kind === "error"}
          <ShieldAlert size={14} class="shrink-0 text-danger" />
          <span class="line-clamp-3 min-w-0 flex-1 break-words text-danger">{status.message}</span>
        {:else}
          <MonitorOff size={14} class="shrink-0 text-fg-muted" />
          <span class="flex-1 text-fg-muted">Disconnected</span>
        {/if}
        <button class="btn-ghost py-1" onclick={reconnect}><RefreshCw size={12} /> Reconnect</button>
      </div>
    {/if}
  </div>
</div>
