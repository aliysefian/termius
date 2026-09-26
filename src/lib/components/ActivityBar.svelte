<script lang="ts">
  import { ArrowLeftRight, Code, FolderSync, KeyRound, Lock, Server, Settings } from "lucide-svelte";
  import { ui, type View } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const items: { view: View; label: string; icon: typeof Server }[] = [
    { view: "hosts", label: "Hosts", icon: Server },
    { view: "keychain", label: "Keychain", icon: KeyRound },
    { view: "sftp", label: "SFTP", icon: FolderSync },
    { view: "forwarding", label: "Port Forwarding", icon: ArrowLeftRight },
    { view: "snippets", label: "Snippets", icon: Code },
  ];
</script>

<nav class="flex w-14 flex-col items-center border-r border-line bg-panel py-3">
  <div class="mb-4 flex h-8 w-8 items-center justify-center rounded-lg bg-accent text-xs font-bold text-white">
    SV
  </div>

  {#each items as item (item.view)}
    <button
      class="group relative mb-1 flex h-10 w-10 items-center justify-center rounded-lg transition-colors
        {ui.view === item.view ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
      onclick={() => (ui.view = item.view)}
      title={item.label}
      aria-label={item.label}
      aria-current={ui.view === item.view ? "page" : undefined}
    >
      {#if ui.view === item.view}
        <span class="absolute left-0 h-5 w-0.5 rounded-r bg-accent"></span>
      {/if}
      <item.icon size={20} />
    </button>
  {/each}

  <div class="flex-1"></div>

  <button
    class="mb-1 flex h-10 w-10 items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-panel-hover hover:text-fg"
    onclick={() => vaultStore.lock()}
    title="Lock vault"
    aria-label="Lock vault"
  >
    <Lock size={20} />
  </button>
  <button
    class="flex h-10 w-10 items-center justify-center rounded-lg transition-colors
      {ui.view === 'settings' ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
    onclick={() => (ui.view = "settings")}
    title="Settings"
    aria-label="Settings"
  >
    <Settings size={20} />
  </button>
</nav>
