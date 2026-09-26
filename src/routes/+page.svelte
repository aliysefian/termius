<script lang="ts">
  import { onMount } from "svelte";
  import ActivityBar from "$lib/components/ActivityBar.svelte";
  import ForwardForm from "$lib/components/ForwardForm.svelte";
  import ForwardingPanel from "$lib/components/ForwardingPanel.svelte";
  import SftpView from "$lib/components/SftpView.svelte";
  import HostForm from "$lib/components/HostForm.svelte";
  import HostTree from "$lib/components/HostTree.svelte";
  import IdentityForm from "$lib/components/IdentityForm.svelte";
  import KeychainPanel from "$lib/components/KeychainPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import SnippetForm from "$lib/components/SnippetForm.svelte";
  import SnippetsPanel from "$lib/components/SnippetsPanel.svelte";
  import TerminalArea from "$lib/components/TerminalArea.svelte";
  import UnlockScreen from "$lib/components/UnlockScreen.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let ready = $state(false);
  $effect(() => {
    if (ui.view === "sftp") ui.sftpVisited = true;
  });
  onMount(async () => {
    await vaultStore.init();
    ready = true;
  });
</script>

{#if !ready}
  <div class="flex h-screen items-center justify-center text-sm text-fg-muted">Starting…</div>
{:else if !vaultStore.unlocked}
  <UnlockScreen />
{:else}
  <div class="flex h-screen overflow-hidden">
    <ActivityBar />

    {#if ui.view === "hosts"}
      <HostTree />
    {:else if ui.view === "keychain"}
      <KeychainPanel />
    {:else if ui.view === "snippets"}
      <SnippetsPanel />
    {:else if ui.view === "forwarding"}
      <ForwardingPanel />
    {/if}

    <!-- Terminals and SFTP stay mounted while hidden so their sessions survive view switches. -->
    <div class="min-w-0 flex-1 {ui.view === 'settings' || ui.view === 'sftp' ? 'hidden' : 'flex'}">
      <TerminalArea />
    </div>
    {#if ui.sftpVisited}
      <div class="min-w-0 flex-1 {ui.view === 'sftp' ? 'flex' : 'hidden'}">
        <SftpView />
      </div>
    {/if}
    {#if ui.view === "settings"}
      <SettingsPanel />
    {/if}
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
  {/if}

  {#if ui.toast}
    <div
      class="fixed bottom-4 left-1/2 z-50 max-w-md -translate-x-1/2 rounded-md border px-4 py-2 text-sm shadow-xl
        {ui.toast.kind === 'error' ? 'border-danger/30 bg-panel text-danger' : 'border-line bg-panel text-fg'}"
      role="status"
    >
      {ui.toast.text}
    </div>
  {/if}

  {#if vaultStore.error}
    <div class="fixed bottom-4 right-4 max-w-sm rounded-md border border-danger/30 bg-panel px-4 py-3 text-sm text-danger shadow-xl">
      {vaultStore.error}
    </div>
  {/if}
{/if}
