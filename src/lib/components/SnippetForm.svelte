<script lang="ts">
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptySnippet, errorMessage, type Snippet, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();
  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.snippets.find((s) => s.id === id)?.data : undefined;
  let form = $state<Snippet>(existing ? structuredClone($state.snapshot(existing)) : emptySnippet());
  let error = $state<string | null>(null);
  let busy = $state(false);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      await vaultStore.saveSnippet(id, $state.snapshot(form));
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit snippet" : "New snippet"} onclose={() => (ui.modal = null)}>
  <form id="snippet-form" onsubmit={save} class="space-y-4">
    <div>
      <label class="label" for="s-label">Label</label>
      <input id="s-label" class="input" bind:value={form.label} required placeholder="Disk usage" />
    </div>
    <div>
      <label class="label" for="s-cmd">Command</label>
      <textarea id="s-cmd" class="input font-mono text-xs" rows="5" bind:value={form.command} required spellcheck="false" placeholder="df -h"></textarea>
    </div>
    <div>
      <label class="label" for="s-desc">Description</label>
      <input id="s-desc" class="input" bind:value={form.description} />
    </div>
    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="snippet-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
