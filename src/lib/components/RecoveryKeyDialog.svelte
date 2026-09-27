<script lang="ts">
  import { Copy, KeyRound, Printer } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { copySecret } from "$lib/secrets.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const key = $derived(vaultStore.pendingRecoveryKey);
  let saved = $state(false);

  async function close() {
    if (!saved && !await ask("Close without saving the recovery key? It can't be shown again, but you can create a new one on the Vault screen.")) return;
    vaultStore.pendingRecoveryKey = null;
    saved = false;
  }
</script>

{#if key}
  <Modal title="Your recovery key" onclose={close}>
    <div class="space-y-3 text-sm">
      <p class="flex items-start gap-2">
        <KeyRound size={16} class="mt-0.5 shrink-0 text-accent" />
        This key can reset your master password if you forget it. It is shown <strong>only now</strong> and is never stored on
        this computer or in the vault folder.
      </p>
      <div class="rounded-md border border-accent/30 bg-base p-3 text-center font-mono text-sm tracking-wider break-all select-all">{key}</div>
      <p class="text-xs text-fg-muted">
        Print it or write it down and keep it somewhere safe and offline, e.g. with important documents. Don't keep it in the
        same synced folder as the vault. Anyone with this key and a copy of the vault can open it.
      </p>
      <div class="flex gap-2">
        <button class="btn-ghost border border-line" onclick={() => copySecret(key, "Recovery key")}><Copy size={14} /> Copy</button>
        <button class="btn-ghost border border-line" onclick={() => window.print()}><Printer size={14} /> Print</button>
      </div>
      <label class="flex items-center gap-2">
        <input type="checkbox" class="accent-[#7b61ff]" bind:checked={saved} />
        I've stored the recovery key safely
      </label>
    </div>
    {#snippet footer()}
      <button class="btn-primary" disabled={!saved} onclick={close}>Done</button>
    {/snippet}
  </Modal>
{/if}
