<script lang="ts">
  import { ArrowLeftRight, Bot, Lock, SquareTerminal } from "lucide-svelte";
  import { PAGE_VIEWS, STATUS_DOT, ui, type View } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  /** Same "go to view" behaviour as the activity bar's icons, minus the toggle-on-repeat-click. */
  function goTo(view: View) {
    ui.view = view;
    if (!PAGE_VIEWS.includes(view) && view !== "sftp") settings.prefs.sidebarHidden = false;
  }

  const agent = $derived(vaultStore.agentStatus);
  const offeredKeys = $derived(vaultStore.keys.filter((k) => k.data?.agent && k.data.agent !== "off").length);
  const cli = $derived(vaultStore.cliStatus);
  const activeTunnels = $derived(Object.values(vaultStore.forwardStatus).filter((s) => s.state === "active" || s.state === "starting").length);

  // The focused pane, kept visible across every view (not just the terminal
  // area) so a connection is never out of sight while editing Settings.
  const activeTab = $derived(ui.activeTab);
  const activePane = $derived(activeTab?.panes.find((p) => p.id === activeTab.activePaneId));
  const info = $derived(activePane ? ui.paneInfo[activePane.id] : undefined);
</script>

{#if !settings.prefs.focusMode}
  <div class="flex h-6 shrink-0 items-center gap-px border-t border-line bg-panel px-1 text-[11px] text-fg-muted" role="presentation">
    <button class="icon-btn h-5 w-5" title="Lock vault" aria-label="Lock vault" onclick={() => vaultStore.lock()}>
      <Lock size={12} />
    </button>

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

    <div class="min-w-0 flex-1"></div>

    {#if activePane && info}
      <span class="h-2 w-2 shrink-0 rounded-full {STATUS_DOT[info.status ?? 'disconnected']}"></span>
      <span class="truncate pl-1.5">{activeTab?.customTitle ?? activeTab?.title}</span>
      {#if info.cwd}
        <span class="truncate pl-1.5 text-fg-muted/70" title={info.cwd}>{info.cwd}</span>
      {/if}
      {#if info.cols && info.rows}
        <span class="shrink-0 pl-1.5 font-mono">{info.cols}×{info.rows}</span>
      {/if}
    {/if}
  </div>
{/if}
