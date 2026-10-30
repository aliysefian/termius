<script lang="ts">
  import { AlertTriangle, ArrowLeftRight, Bot, ShieldCheck, SquareTerminal } from "lucide-svelte";
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

  /** "Connected to claude (146.70.44.165)": the focused connection in one line. */
  const statusText = $derived.by(() => {
    if (!activePane || !info) return "";
    const t = activePane.target;
    const host = t.kind === "host" ? vaultStore.hostById.get(t.hostId)?.data : undefined;
    const name = activeTab?.customTitle ?? host?.label ?? activeTab?.title ?? "";
    const addr = host?.hostname ? ` (${host.hostname})` : "";
    const state = info.status === "connected" ? "Connected to" : info.status === "connecting" ? "Connecting to" : info.status === "error" ? "Connection failed:" : info.status === "disconnected" ? "Disconnected from" : "Waiting for host key of";
    return t.kind === "local" ? "Local shell" : `${state} ${name}${addr}`;
  });
  const chip = "flex h-5 items-center gap-1.5 rounded px-1.5 hover:bg-panel-hover hover:text-fg";
</script>

{#if !settings.prefs.focusMode}
  <footer class="flex h-7 shrink-0 items-center gap-px border-t border-line bg-panel px-2 text-[11px] text-fg-muted" aria-label="Status">
    {#if activePane && info}
      <span class="flex min-w-0 items-center gap-2 text-xs text-fg">
        <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[info.status ?? 'disconnected']}"></span>
        <span class="truncate" title={info.cwd ?? ""}>{statusText}</span>
      </span>
    {:else}
      <span class="text-xs">No open connection</span>
    {/if}

    {#if sync}
      <button class="{chip} ml-2 {sync.tone === 'danger' ? 'text-danger' : 'text-warning'}" title={sync.title} onclick={() => goTo("vault")} data-testid="sync-badge">
        <AlertTriangle size={12} /><span>{sync.text}</span>
      </button>
    {/if}

    <!-- A version of a config file was just kept: says which, and opens its history. -->
    {#if snapshotVault.note}
      {@const n = snapshotVault.note}
      <button class="{chip} ml-2 min-w-0 max-w-[48ch] tabular-nums" title="Open the Config Time-Machine" data-testid="snapshot-note" onclick={() => (ui.configHistory = { hostId: n.host_id, path: n.file_path })}>
        <ShieldCheck size={12} class="shrink-0 text-success" />
        <span class="truncate">Snapshot saved: {n.file_path} (v{n.version})</span>
      </button>
    {/if}

    <div class="min-w-0 flex-1"></div>

    {#if activePane && info}
      {#if facts}
        <span class="{chip} hidden cursor-default md:flex" title="Protocol · text encoding · TERM">
          {facts.protocol}{#if facts.encoding}<span class="text-fg-muted/60">·</span>{facts.encoding}{/if}{#if facts.term}<span class="hidden lg:inline text-fg-muted/60">·</span><span class="hidden lg:inline">{facts.term}</span>{/if}
        </span>
        {#if facts.latencyMs !== null}
          <span class="{chip} cursor-default tabular-nums text-success" title="Last reachability probe">{facts.latencyMs}ms</span>
        {/if}
      {/if}
      {#if info.cols && info.rows}
        <span class="{chip} cursor-default font-mono tabular-nums" title="Terminal size">{info.cols}×{info.rows}</span>
      {/if}
      <span class="mx-1 h-3.5 w-px bg-line" aria-hidden="true"></span>
    {/if}

    <button class="{chip} {ui.view === 'keys' ? 'text-accent' : ''}" title={agent?.enabled ? `SSH agent on · ${offeredKeys} key${offeredKeys === 1 ? "" : "s"} offered` : "SSH agent off"} onclick={() => goTo("keys")}>
      <Bot size={12} /><span class="hidden lg:inline">Agent</span><span>{agent?.enabled ? `on · ${offeredKeys}` : "off"}</span>
    </button>
    <button class="{chip} {ui.view === 'settings' ? 'text-accent' : ''}" title={cli?.enabled ? "Command line on" : "Command line off"} onclick={() => goTo("settings")}>
      <SquareTerminal size={12} /><span class="hidden lg:inline">CLI</span><span>{cli?.enabled ? "on" : "off"}</span>
    </button>
    <button class="{chip} {ui.view === 'forwarding' ? 'text-accent' : ''}" title="{activeTunnels} active tunnel{activeTunnels === 1 ? '' : 's'}" onclick={() => goTo("forwarding")}>
      <ArrowLeftRight size={12} /><span>{activeTunnels}<span class="hidden lg:inline">&nbsp;tunnel{activeTunnels === 1 ? "" : "s"}</span></span>
    </button>
  </footer>
{/if}
