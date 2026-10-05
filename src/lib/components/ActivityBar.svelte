<script lang="ts">
  import {
    ArrowLeftRight,
    ArrowUpCircle,
    Code,
    Container,
    Database,
    FolderSync,
    FolderTree,
    KeyRound,
    Lock,
    PanelLeftClose,
    PanelLeftOpen,
    Server,
    Settings,
    ShieldCheck,
    Star,
    UserRound,
    Vault,
  } from "lucide-svelte";
  import { PAGE_VIEWS, ui, type View } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { updates } from "$lib/stores/updates.svelte";

  const items: { view: View; label: string; icon: typeof Server }[] = [
    { view: "hosts", label: "Hosts", icon: Server },
    { view: "favorites", label: "Favorites", icon: Star },
    { view: "groups", label: "Groups and proxies", icon: FolderTree },
    { view: "keys", label: "Keys", icon: KeyRound },
    { view: "keychain", label: "Credentials", icon: UserRound },
    { view: "forwarding", label: "Tunnels", icon: ArrowLeftRight },
    { view: "snippets", label: "Snippets", icon: Code },
    { view: "sftp", label: "SFTP", icon: FolderSync },
    { view: "databases", label: "Databases", icon: Database },
    { view: "containers", label: "Containers", icon: Container },
    { view: "knownhosts", label: "Known hosts", icon: ShieldCheck },
    { view: "vault", label: "Vault", icon: Vault },
  ];
</script>

<nav class="flex w-14 flex-col items-center overflow-y-auto border-r border-line bg-panel py-3">
  <div class="mb-4 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent text-xs font-bold text-white">
    SV
  </div>

  {#each items as item (item.view)}
    <button
      class="group relative mb-1 flex h-10 w-10 shrink-0 items-center justify-center rounded-lg transition-colors
        {ui.view === item.view ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
      onclick={() => {
        // Clicking the current view's icon toggles the panel, like VS Code.
        if (ui.view === item.view && !PAGE_VIEWS.includes(item.view) && item.view !== "sftp") ui.toggleSidebar();
        else {
          ui.view = item.view;
          if (!PAGE_VIEWS.includes(item.view) && item.view !== "sftp") settings.prefs.sidebarHidden = false;
        }
      }}
      title={item.label}
      aria-label={item.label}
      aria-current={ui.view === item.view ? "page" : undefined}
    >
      {#if ui.view === item.view}
        <span class="absolute left-0 h-5 w-0.5 rounded-r bg-accent"></span>
      {/if}
      <item.icon size={20} />
      {#if item.view === "vault" && vaultStore.openConflicts > 0}
        <span class="absolute right-1 top-1 h-2 w-2 rounded-full bg-warning" title="Sync conflicts need attention"></span>
      {/if}
    </button>
  {/each}

  <div class="flex-1"></div>

  {#if updates.available}
    <button
      class="relative mb-1 flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-accent transition-colors hover:bg-accent/15"
      onclick={() => (updates.dismissed = null)}
      title={updates.installing ? "Installing the update…" : `Update to SSHVault ${updates.available.version}`}
      aria-label="Update available"
    >
      <ArrowUpCircle size={20} class={updates.installing ? "animate-pulse" : ""} />
      <span class="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-accent"></span>
    </button>
  {/if}

  <button
    class="mb-1 flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-panel-hover hover:text-fg"
    onclick={() => ui.toggleSidebar()}
    title={settings.prefs.sidebarHidden ? "Show the list panel (Ctrl+Shift+H)" : "Hide the list panel (Ctrl+Shift+H)"}
    aria-label={settings.prefs.sidebarHidden ? "Show the list panel" : "Hide the list panel"}
    aria-pressed={settings.prefs.sidebarHidden}
  >
    {#if settings.prefs.sidebarHidden}<PanelLeftOpen size={20} />{:else}<PanelLeftClose size={20} />{/if}
  </button>
  <button
    class="mb-1 flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-panel-hover hover:text-fg"
    onclick={() => vaultStore.lock()}
    title="Lock vault"
    aria-label="Lock vault"
  >
    <Lock size={20} />
  </button>
  <button
    class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg transition-colors
      {ui.view === 'settings' ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
    onclick={() => (ui.view = "settings")}
    title="Settings"
    aria-label="Settings"
  >
    <Settings size={20} />
  </button>
</nav>
