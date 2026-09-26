<script lang="ts">
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  let name = $state("");
  let error = $state<string | null>(null);
  const tabs = ui.snapshotWorkspace();
  const existing = $derived(vaultStore.workspaces.find((w) => w.data?.name.toLowerCase() === name.trim().toLowerCase()));

  async function save(e: SubmitEvent) {
    e.preventDefault();
    try {
      await vaultStore.saveWorkspace(existing?.id ?? null, { name: name.trim(), tabs }, existing?.rev ?? null);
      ui.notify("info", `Workspace "${name.trim()}" saved. Open it from the command palette.`);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    }
  }
</script>

<Modal title="Save workspace" onclose={() => (ui.modal = null)}>
  <form id="ws-form" class="space-y-3" onsubmit={save}>
    <p class="text-sm text-fg-muted">
      Saves the {tabs.length} open tab{tabs.length === 1 ? "" : "s"} and which host each pane connects to. Nothing secret is
      stored; quick-connect passwords are left out.
    </p>
    <!-- svelte-ignore a11y_autofocus -->
    <input class="input" bind:value={name} required placeholder="Morning checks" autofocus aria-label="Workspace name" />
    {#if existing}<p class="text-xs text-warning">Replaces the saved workspace with this name.</p>{/if}
    {#if error}<p class="text-sm text-danger">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="ws-form" disabled={!name.trim() || tabs.length === 0}>Save</button>
  {/snippet}
</Modal>
