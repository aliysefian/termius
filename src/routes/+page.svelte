<script lang="ts">
  import { onMount } from "svelte";
  import { Copy, X } from "lucide-svelte";
  import Spinner from "$lib/components/Spinner.svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ActivityBar from "$lib/components/ActivityBar.svelte";
  import ForwardForm from "$lib/components/ForwardForm.svelte";
  import ForwardingPanel from "$lib/components/ForwardingPanel.svelte";
  import SftpView from "$lib/components/SftpView.svelte";
  import HostForm from "$lib/components/HostForm.svelte";
  import HostTree from "$lib/components/HostTree.svelte";
  import ResizablePanel from "$lib/components/ResizablePanel.svelte";
  import IdentityForm from "$lib/components/IdentityForm.svelte";
  import KeychainPanel from "$lib/components/KeychainPanel.svelte";
  import KeysPanel from "$lib/components/KeysPanel.svelte";
  import GroupsPanel from "$lib/components/GroupsPanel.svelte";
  import KnownHostsPanel from "$lib/components/KnownHostsPanel.svelte";
  import VaultPanel from "$lib/components/VaultPanel.svelte";
  import SecurityReview from "$lib/components/SecurityReview.svelte";
  import FleetView from "$lib/components/FleetView.svelte";
  import ChangelogView from "$lib/components/ChangelogView.svelte";
  import HostKeyDialog from "$lib/components/HostKeyDialog.svelte";
  import AgentPromptDialog from "$lib/components/AgentPromptDialog.svelte";
  import CliPromptDialog from "$lib/components/CliPromptDialog.svelte";
  import UpdateBanner from "$lib/components/UpdateBanner.svelte";
  import { updates } from "$lib/stores/updates.svelte";
  import RecoveryKeyDialog from "$lib/components/RecoveryKeyDialog.svelte";
  import SaveWorkspace from "$lib/components/SaveWorkspace.svelte";
  import ShortcutsDialog from "$lib/components/ShortcutsDialog.svelte";
  import BulkEditForm from "$lib/components/BulkEditForm.svelte";
  import SerialDialog from "$lib/components/SerialDialog.svelte";
  import HostDetails from "$lib/components/HostDetails.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import SnippetForm from "$lib/components/SnippetForm.svelte";
  import SnippetsPanel from "$lib/components/SnippetsPanel.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import TerminalArea from "$lib/components/TerminalArea.svelte";
  import UnlockScreen from "$lib/components/UnlockScreen.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import ImportSshConfig from "$lib/components/ImportSshConfig.svelte";
  import QuickConnect from "$lib/components/QuickConnect.svelte";
  import MasterPasswordPrompt from "$lib/components/MasterPasswordPrompt.svelte";
  import { secrets } from "$lib/secrets.svelte";
  import RunOnHosts from "$lib/components/RunOnHosts.svelte";
  import SnippetVarsDialog from "$lib/components/SnippetVarsDialog.svelte";
  import { handleShortcut } from "$lib/shortcuts";
  import { settings } from "$lib/stores/settings.svelte";
  import { PAGE_VIEWS, paneLabel, ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let ready = $state(false);

  // -- keyboard shortcuts (capture phase: beat xterm to the key) ---------
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (handleShortcut(e)) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  // -- window title follows the active pane --------------------------------
  const windowTitle = $derived.by(() => {
    const tab = ui.activeTab;
    const pane = tab?.panes.find((p) => p.id === tab.activePaneId) ?? tab?.panes[0];
    if (!tab || !pane) return "SSHVault";
    return `${tab.customTitle ?? paneLabel(pane.target).split(" · ")[0]} · SSHVault`;
  });
  $effect(() => {
    void getCurrentWindow().setTitle(windowTitle).catch(() => {});
  });

  // -- quitting with live sessions asks first ------------------------------
  onMount(() => {
    let closing = false;
    const win = getCurrentWindow();
    const un = win.onCloseRequested(async (e) => {
      if (closing) return;
      // Save what was open so it can come back next time, unless the vault
      // was already locked (then there's nothing open worth saving over).
      if (vaultStore.unlocked) settings.saveLastSession(ui.snapshotWorkspace());
      const live = ui.liveSessions(ui.tabs);
      if (live.connected === 0 || !settings.prefs.confirmCloseSessions) return;
      e.preventDefault();
      if (await ui.confirmClose(live, "SSHVault")) {
        closing = true;
        await win.destroy();
      }
    });
    return () => void un.then((f) => f());
  });

  // -- auto-lock after inactivity -----------------------------------------
  let lastActivity = Date.now();
  onMount(() => {
    const bump = () => (lastActivity = Date.now());
    const events = ["keydown", "mousedown", "mousemove", "wheel", "touchstart"] as const;
    for (const ev of events) window.addEventListener(ev, bump, { capture: true, passive: true });
    const timer = setInterval(() => {
      const minutes = settings.prefs.autoLockMinutes;
      if (!minutes || !vaultStore.unlocked) return;
      if (Date.now() - lastActivity >= minutes * 60_000) {
        void vaultStore.lock().then(() => ui.notify("info", "Vault locked after inactivity."));
      }
    }, 15_000);
    return () => {
      clearInterval(timer);
      for (const ev of events) window.removeEventListener(ev, bump, { capture: true });
    };
  });
  // Unlocking counts as activity; locking abandons any pending reveal.
  $effect(() => {
    if (vaultStore.unlocked) lastActivity = Date.now();
    else secrets.cancel();
  });
  $effect(() => {
    if (ui.view === "sftp") ui.sftpVisited = true;
  });
  onMount(async () => {
    await vaultStore.init();
    ready = true;
    // After start-up settles, so it never slows the first screen.
    setTimeout(() => void updates.init(), 3000);
  });

  /** Once, after an update actually took effect (the running version changed since last launch), points at the changelog. */
  async function announceIfUpdated() {
    await updates.loadInfo();
    const current = updates.info?.version;
    if (!current) return;
    const last = settings.prefs.lastSeenVersion;
    if (last && last !== current) {
      ui.notify("info", `Updated to v${current}.`, { label: "View changelog", run: () => (ui.view = "changelog") }, 10000);
    }
    settings.prefs.lastSeenVersion = current;
  }

  // Wait until the vault is actually unlocked (and so the toast is visible
  // and its countdown means something) before checking, and only once per
  // run of the app.
  let announced = false;
  $effect(() => {
    if (vaultStore.unlocked && !announced) {
      announced = true;
      void announceIfUpdated();
    }
  });
</script>

{#if !ready}
  <div class="flex h-screen items-center justify-center"><Spinner label="Starting…" /></div>
{:else if !vaultStore.unlocked}
  <UnlockScreen />
{:else}
  <div class="flex h-screen flex-col overflow-hidden {settings.prefs.density === 'compact' ? 'density-compact' : ''} {settings.prefs.focusMode ? 'focus-mode' : ''}">
    {#if settings.prefs.focusMode}
      <button
        class="fixed right-2 top-1 z-40 rounded-md border border-line bg-panel/80 px-2 py-0.5 text-[11px] text-fg-muted opacity-40 hover:opacity-100"
        onclick={() => (settings.prefs.focusMode = false)}
        title="Leave focus mode (Ctrl+Shift+U)">Exit focus</button
      >
    {/if}
    <div class="flex min-h-0 flex-1 overflow-hidden">
    {#if !settings.prefs.focusMode}
      <ActivityBar />
    {/if}

    {#if !settings.prefs.sidebarHidden && !settings.prefs.focusMode && ["hosts", "favorites", "keychain", "snippets", "forwarding"].includes(ui.view)}
      <ResizablePanel>
        {#if ui.view === "hosts"}
          <HostTree />
        {:else if ui.view === "favorites"}
          <HostTree favoritesOnly />
        {:else if ui.view === "keychain"}
          <KeychainPanel />
        {:else if ui.view === "snippets"}
          <SnippetsPanel />
        {:else if ui.view === "forwarding"}
          <ForwardingPanel />
        {/if}
      </ResizablePanel>
    {/if}

    <!-- Terminals and SFTP stay mounted while hidden so their sessions survive view switches. -->
    <div class="min-w-0 flex-1 {PAGE_VIEWS.includes(ui.view) || ui.view === 'sftp' ? 'hidden' : 'flex'}">
      <TerminalArea />
    </div>
    {#if ui.sftpVisited}
      <div class="min-w-0 flex-1 {ui.view === 'sftp' ? 'flex' : 'hidden'}">
        <SftpView />
      </div>
    {/if}
    {#if ui.view === "settings"}
      <SettingsPanel />
    {:else if ui.view === "keys"}
      <KeysPanel />
    {:else if ui.view === "groups"}
      <GroupsPanel />
    {:else if ui.view === "knownhosts"}
      <KnownHostsPanel />
    {:else if ui.view === "vault"}
      <VaultPanel />
    {:else if ui.view === "security-review"}
      <SecurityReview />
    {:else if ui.view === "fleet"}
      <FleetView />
    {:else if ui.view === "changelog"}
      <ChangelogView />
    {/if}
    </div>
    <StatusBar />
  </div>

  {#if ui.modal?.kind === "host"}
    {#key ui.modal.id}
      <HostForm id={ui.modal.id} group={ui.modal.group} />
    {/key}
  {:else if ui.modal?.kind === "identity"}
    {#key ui.modal.id}
      <IdentityForm id={ui.modal.id} />
    {/key}
  {:else if ui.modal?.kind === "snippet"}
    {#key ui.modal.id}
      <SnippetForm id={ui.modal.id} />
    {/key}
  {:else if ui.modal?.kind === "forward"}
    {#key ui.modal.id}
      <ForwardForm id={ui.modal.id} />
    {/key}
  {:else if ui.modal?.kind === "quick-connect"}
    <QuickConnect initial={ui.modal.initial} />
  {:else if ui.modal?.kind === "import-ssh-config"}
    <ImportSshConfig />
  {:else if ui.modal?.kind === "snippet-vars"}
    <SnippetVarsDialog command={ui.modal.command} names={ui.modal.names} opts={ui.modal.opts} />
  {:else if ui.modal?.kind === "run-on-hosts"}
    <RunOnHosts command={ui.modal.command} />
  {:else if ui.modal?.kind === "save-workspace"}
    <SaveWorkspace />
  {:else if ui.modal?.kind === "shortcuts"}
    <ShortcutsDialog />
  {:else if ui.modal?.kind === "serial"}
    <SerialDialog />
  {:else if ui.modal?.kind === "bulk-edit"}
    <BulkEditForm />
  {:else if ui.modal?.kind === "host-details"}
    {#key ui.modal.id}
      <HostDetails id={ui.modal.id} />
    {/key}
  {:else if ui.modal?.kind === "onboarding"}
    <Onboarding />
  {/if}

  <HostKeyDialog />
  <AgentPromptDialog />
  <CliPromptDialog />
  <UpdateBanner />

  {#if ui.paletteOpen}
    <CommandPalette />
  {/if}

  <MasterPasswordPrompt />
  <RecoveryKeyDialog />

  {#if ui.toasts.length}
    <div class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex flex-col items-center gap-2 px-4">
      {#each ui.toasts as toast (toast.id)}
        <div
          class="pointer-events-auto flex w-full max-w-md items-start gap-3 rounded-md border px-4 py-2 text-sm shadow-xl
            {toast.kind === 'error' ? 'border-danger/30 bg-panel text-danger' : 'border-line bg-panel text-fg'}"
          role={toast.kind === "error" ? "alert" : "status"}
        >
          <span class="line-clamp-3 min-w-0 flex-1 break-words">{toast.text}</span>
          {#if toast.action}
            <button class="shrink-0 font-semibold text-accent hover:underline" onclick={() => ui.runToastAction(toast.id)}>{toast.action.label}</button>
          {/if}
          {#if toast.kind === "error" && toast.text.length > 120}
            <button class="shrink-0 text-fg-muted hover:text-fg" title="Copy the full message" aria-label="Copy the full message" onclick={() => void writeText(toast.text).catch(() => {})}><Copy size={14} /></button>
          {/if}
          <button class="-mr-1 shrink-0 text-fg-muted hover:text-fg" aria-label="Dismiss" onclick={() => ui.dismissToast(toast.id)}><X size={14} /></button>
        </div>
      {/each}
    </div>
  {/if}

  {#if vaultStore.error}
    <div class="fixed bottom-4 right-4 flex max-w-sm items-start gap-3 rounded-md border border-danger/30 bg-panel px-4 py-3 text-sm text-danger shadow-xl" role="alert">
      <span class="min-w-0 flex-1 break-words">{vaultStore.error}</span>
      <button class="-mr-1 shrink-0 text-fg-muted hover:text-fg" aria-label="Dismiss" onclick={() => (vaultStore.error = null)}><X size={14} /></button>
    </div>
  {/if}
{/if}
