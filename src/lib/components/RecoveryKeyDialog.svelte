<script lang="ts">
  import { Copy, KeyRound, Printer } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { copySecret } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const key = $derived(vaultStore.pendingRecoveryKey);
  let saved = $state(false);

  // This key is shown exactly once. No bypass: Escape, the backdrop and the
  // X all route here, and none of them close the dialog until the checkbox
  // is ticked, so it can't be dismissed by a stray keypress.
  function close() {
    if (!saved) {
      ui.notify("info", "Tick the box once you've stored the recovery key, then close.");
      return;
    }
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
        <button class="btn-secondary" onclick={() => copySecret(key, "Recovery key")}><Copy size={14} /> Copy</button>
        <button class="btn-secondary" onclick={() => window.print()}><Printer size={14} /> Print</button>
      </div>
      <label class="flex items-center gap-2">
        <input type="checkbox" class="accent-input" bind:checked={saved} />
        I've stored the recovery key safely
      </label>
    </div>
    {#snippet footer()}
      <button class="btn-primary" disabled={!saved} onclick={close}>Done</button>
    {/snippet}
  </Modal>
{/if}
