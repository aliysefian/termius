<script lang="ts">
  import Modal, { DISCARD } from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptySnippet, errorMessage, type Snippet, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();
  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existingRec = id ? vaultStore.snippets.find((s) => s.id === id) : undefined;
  const existing = existingRec?.data;
  const baseRev = existingRec?.rev ?? null;
  let form = $state<Snippet>(existing ? structuredClone($state.snapshot(existing)) : emptySnippet());
  let tags = $state((form.tags ?? []).join(", "));
  const folders = $derived([...new Set(vaultStore.snippets.map((s) => s.data?.folder).filter(Boolean))].sort());
  let error = $state<string | null>(null);
  let busy = $state(false);

  const fingerprint = () => JSON.stringify([$state.snapshot(form), tags]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      form.tags = tags.split(",").map((t) => t.trim()).filter(Boolean);
      form.folder = form.folder?.split("/").map((p) => p.trim()).filter(Boolean).join("/") || undefined;
      await vaultStore.saveSnippet(id, $state.snapshot(form), baseRev);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit snippet" : "New snippet"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null}>
  <form id="snippet-form" onsubmit={save} class="space-y-4">
    <div>
      <label class="label" for="s-label">Label</label>
      <input id="s-label" class="input" bind:value={form.label} required placeholder="Disk usage" />
    </div>
    <div>
      <label class="label" for="s-cmd">Command</label>
      <textarea id="s-cmd" class="input font-mono text-xs" rows="5" bind:value={form.command} required spellcheck="false" placeholder="df -h"></textarea>
    </div>
    <p class="text-xs text-fg-muted">Use <code>{"{{name}}"}</code> for values you're asked for when running it.</p>
    <div>
      <label class="label" for="s-desc">Description</label>
      <input id="s-desc" class="input" bind:value={form.description} />
    </div>
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="s-folder">Folder</label>
        <input id="s-folder" class="input" list="s-folders" bind:value={form.folder} placeholder="Kubernetes/Debug" />
        <datalist id="s-folders">{#each folders as f (f)}<option value={f}></option>{/each}</datalist>
      </div>
      <div>
        <label class="label" for="s-tags">Tags</label>
        <input id="s-tags" class="input" bind:value={tags} placeholder="k8s, logs" />
      </div>
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
