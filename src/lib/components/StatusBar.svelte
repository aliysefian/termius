<script lang="ts">
  import { AlertTriangle, ArrowLeftRight, Bot, Lock, ShieldCheck, SquareTerminal } from "lucide-svelte";
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
</script>

{#if !settings.prefs.focusMode}
  <footer class="flex h-6 shrink-0 items-center gap-px border-t border-line bg-panel px-1 font-mono text-[11px] text-fg-muted" aria-label="Status">
    <button class="icon-btn h-5 w-5" title="Lock vault" aria-label="Lock vault" onclick={() => vaultStore.lock()}>
      <Lock size={12} />
    </button>

    {#if sync}
      <button
        class="flex h-5 items-center gap-1.5 rounded px-1.5 hover:bg-panel-hover {sync.tone === 'danger' ? 'text-danger' : 'text-warning'}"
        title={sync.title}
        onclick={() => goTo("vault")}
        data-testid="sync-badge"
      >
        <AlertTriangle size={12} />
        <span>{sync.text}</span>
      </button>
    {/if}

    <button
      class="flex h-5 items-center gap-1.5 rounded px-1.5 hover:bg-panel-hover hover:text-fg {ui.view === 'keys' ? 'text-accent' : ''}"
      title={agent?.enabled ? `SSH agent on · ${offeredKeys} key${offeredKeys === 1 ? "" : "s"} offered` : "SSH agent off"}
      onclick={() => goTo("keys")}
    >
      <Bot size={12} />
      <span>Agent {agent?.enabled ? `on · ${offeredKeys}` : "off"}</span>
    </button>

    <button
      class="flex h-5 items-center gap-1.5 rounded px-1.5 hover:bg-panel-hover hover:text-fg {ui.view === 'settings' ? 'text-accent' : ''}"
      title={cli?.enabled ? "Command line on" : "Command line off"}
      onclick={() => goTo("settings")}
    >
      <SquareTerminal size={12} />
      <span>CLI {cli?.enabled ? "on" : "off"}</span>
    </button>

    <button
      class="flex h-5 items-center gap-1.5 rounded px-1.5 hover:bg-panel-hover hover:text-fg {ui.view === 'forwarding' ? 'text-accent' : ''}"
      title="{activeTunnels} active tunnel{activeTunnels === 1 ? '' : 's'}"
      onclick={() => goTo("forwarding")}
    >
      <ArrowLeftRight size={12} />
      <span>{activeTunnels} tunnel{activeTunnels === 1 ? "" : "s"}</span>
    </button>

    <!-- A version of a config file was just kept: says which, and opens its history. -->
    {#if snapshotVault.note}
      {@const n = snapshotVault.note}
      <button
        class="flex h-6 min-w-0 max-w-[48ch] items-center gap-1.5 rounded px-1.5 font-mono text-[11px] tabular-nums text-fg-muted hover:bg-panel-hover hover:text-fg"
        title="Open the Config Time-Machine"
        data-testid="snapshot-note"
        onclick={() => (ui.configHistory = { hostId: n.host_id, path: n.file_path })}
      >
        <ShieldCheck size={12} class="shrink-0 text-success" />
        <span class="truncate">Snapshot saved: {n.file_path} (v{n.version})</span>
      </button>
    {/if}

    <div class="min-w-0 flex-1"></div>

    {#if activePane && info}
      <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[info.status ?? 'disconnected']}"></span>
      <span class="truncate pl-1.5 font-medium">{activeTab?.customTitle ?? activeTab?.title}</span>
      {#if info.cwd}
        <span class="truncate pl-1.5 text-fg-muted" title={info.cwd}>{info.cwd}</span>
      {/if}
      {#if facts}
        <!-- One run of facts, dot-separated; TERM drops out first when the window is narrow. -->
        <span class="shrink-0 pl-2 tabular-nums" title="Protocol · last probe latency · text encoding · TERM">
          {facts.protocol}{#if facts.latencyMs !== null}<span class="text-fg-muted/60"> · </span>{facts.latencyMs}ms{/if}{#if facts.encoding}<span class="text-fg-muted/60"> · </span>{facts.encoding}{/if}{#if facts.term}<span class="hidden lg:inline"><span class="text-fg-muted/60"> · </span>{facts.term}</span>{/if}
        </span>
      {/if}
      {#if info.cols && info.rows}
        <span class="shrink-0 pl-1.5 font-mono tabular-nums">{info.cols}×{info.rows}</span>
      {/if}
    {/if}
  </footer>
{/if}
