<script lang="ts">
  import { AlertTriangle, ArrowLeftRight, Bot, ShieldCheck, SignalHigh, SquareTerminal, TriangleAlert } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import { envInfo } from "$lib/types";
  import { snapshotVault } from "$lib/confighistory/snapshotVaultStore.svelte";
  import { syncBadge } from "$lib/syncstatus";
  import { PAGE_VIEWS, STATUS_DOT, ui, type View } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { sessionFacts } from "$lib/sessioninfo";
  import { vaultStore } from "$lib/stores/vault.svelte";

  /** Same "go to view" behaviour as the activity bar's icons, minus the toggle-on-repeat-click. */
  function goTo(view: View) {
    ui.view = view;
    if (!PAGE_VIEWS.includes(view) && view !== "sftp") settings.prefs.sidebarHidden = false;
  }

  const agent = $derived(vaultStore.agentStatus);
  const offeredKeys = $derived(vaultStore.keys.filter((k) => k.data?.agent && k.data.agent !== "off").length);
  const cli = $derived(vaultStore.cliStatus);
  const sync = $derived(syncBadge({ conflicts: vaultStore.openConflicts, rollbacks: vaultStore.rollbacks }));
  const activeTunnels = $derived(Object.values(vaultStore.forwardStatus).filter((s) => s.state === "active" || s.state === "starting").length);

  // The focused pane, kept visible across every view (not just the terminal
  // area) so a connection is never out of sight while editing Settings.
  const activeTab = $derived(ui.activeTab);
  const activePane = $derived(activeTab?.panes.find((p) => p.id === activeTab.activePaneId));
  const info = $derived(activePane ? ui.paneInfo[activePane.id] : undefined);
  const facts = $derived.by(() => {
    if (!activePane) return undefined;
    const t = activePane.target;
    const hostId = t.kind === "host" ? t.hostId : undefined;
    return sessionFacts(t, hostId ? vaultStore.hostById.get(hostId)?.data : undefined, hostId ? vaultStore.health[hostId] : undefined);
  });

  const host = $derived(activePane?.target.kind === "host" ? vaultStore.hostById.get(activePane.target.hostId)?.data : undefined);
  const env = $derived(envInfo(host?.environment));
  /** The focused connection's name: the host's label, else what the tab is called. */
  const name = $derived(host?.label ?? activeTab?.customTitle ?? activeTab?.title ?? "");
  const telnet = $derived(activePane?.target.kind === "telnet" || host?.protocol === "telnet");
  /** Words only for a state that needs them; a healthy connection is the green dot. */
  const stateWord = $derived(
    info?.status === "connecting" ? "Connecting…" : info?.status === "error" ? "Connection failed" : info?.status === "disconnected" ? "Disconnected" : info?.status === "new_host_key" ? "Verify host key" : info?.status === "host_key_changed" ? "Host key changed" : "",
  );
  const chip = "flex h-6 items-center gap-1.5 rounded-md px-1.5 hover:bg-panel-hover hover:text-fg";
</script>

{#if !settings.prefs.focusMode}
  <footer class="flex h-7 shrink-0 items-center gap-0.5 border-t border-line bg-base px-2 text-[11px] text-fg-muted" aria-label="Status">
    {#if activePane && info}
      {#if env.value}<Badge tone={"tone" in env ? env.tone : undefined}>{"short" in env ? env.short : ""}</Badge>{/if}
      <span class="ml-1.5 flex min-w-0 items-center gap-2 text-xs text-fg" title={info.cwd ?? name}>
        <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[info.status ?? 'disconnected']}"></span>
        <span class="truncate font-medium">{name}</span>
        {#if host?.hostname}<span class="hidden shrink-0 font-mono text-fg-muted md:inline">{host.hostname}</span>{/if}
        {#if stateWord}<span class="shrink-0 {info.status === 'error' || info.status === 'host_key_changed' ? 'text-danger' : 'text-warning'}">{stateWord}</span>{/if}
      </span>
      {#if facts}
        {#if facts.latencyMs !== null}
          <span class="{chip} ml-2 cursor-default tabular-nums text-success" title="Last reachability probe"><SignalHigh size={14} />{facts.latencyMs}ms</span>
        {/if}
        <span class="{chip} cursor-default" title="Protocol · text encoding · TERM: {facts.encoding ?? ''} {facts.term ?? ''}">
          {#if telnet}<TriangleAlert size={13} class="text-warning" /><span class="text-warning">Telnet, unencrypted</span>{:else}<ShieldCheck size={14} />{facts.protocol}{/if}
        </span>
      {/if}
    {:else}
      <span class="flex items-center gap-2 text-xs"><span class="h-2 w-2 rounded-full bg-fg-muted/40"></span>No open connection</span>
    {/if}

    {#if sync}
      <button class="{chip} ml-2 {sync.tone === 'danger' ? 'text-danger' : 'text-warning'}" title={sync.title} onclick={() => goTo("vault")} data-testid="sync-badge">
        <AlertTriangle size={13} /><span>{sync.text}</span>
      </button>
    {/if}

    <!-- A version of a config file was just kept: says which, and opens its history. -->
    {#if snapshotVault.note}
      {@const n = snapshotVault.note}
      <button class="{chip} ml-2 min-w-0 max-w-[48ch] tabular-nums" title="Open the Config Time-Machine" data-testid="snapshot-note" onclick={() => (ui.configHistory = { hostId: n.host_id, path: n.file_path })}>
        <ShieldCheck size={13} class="shrink-0 text-success" />
        <span class="truncate">Snapshot saved: {n.file_path} (v{n.version})</span>
      </button>
    {/if}

    <div class="min-w-0 flex-1"></div>

    {#if activePane && info?.cols && info.rows}
      <span class="{chip} hidden cursor-default font-mono tabular-nums lg:flex" title="Terminal size, columns × rows">{info.cols}×{info.rows}</span>
    {/if}
    <!-- What runs in the background: icons, with a count only where there is one. -->
    <button class="{chip} {agent?.enabled ? 'text-accent' : ''} {ui.view === 'keys' ? 'bg-panel-hover' : ''}" title={agent?.enabled ? `SSH agent on · ${offeredKeys} key${offeredKeys === 1 ? "" : "s"} offered` : "SSH agent off"} aria-label={agent?.enabled ? `SSH agent on, ${offeredKeys} keys offered` : "SSH agent off"} onclick={() => goTo("keys")}>
      <Bot size={14} />{#if agent?.enabled}<span class="tabular-nums">{offeredKeys}</span>{/if}
    </button>
    <button class="{chip} {cli?.enabled ? 'text-accent' : ''} {ui.view === 'settings' ? 'bg-panel-hover' : ''}" title={cli?.enabled ? "Command line on" : "Command line off"} aria-label={cli?.enabled ? "Command line on" : "Command line off"} onclick={() => goTo("settings")}>
      <SquareTerminal size={14} />
    </button>
    <button class="{chip} {activeTunnels ? 'text-accent' : ''} {ui.view === 'forwarding' ? 'bg-panel-hover' : ''}" title="{activeTunnels} active tunnel{activeTunnels === 1 ? '' : 's'}" aria-label="{activeTunnels} active tunnels" onclick={() => goTo("forwarding")}>
      <ArrowLeftRight size={14} />{#if activeTunnels}<span class="tabular-nums">{activeTunnels}</span>{/if}
    </button>
  </footer>
{/if}
