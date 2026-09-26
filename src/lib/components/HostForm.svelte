<script lang="ts">
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptyHost, errorMessage, type Host, type Uuid } from "$lib/types";

  let { id, group }: { id: Uuid | null; group?: string } = $props();

  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.hostById.get(id)?.data : undefined;
  // svelte-ignore state_referenced_locally
  let form = $state<Host>(existing ? structuredClone($state.snapshot(existing)) : { ...emptyHost(), group: group ?? "" });
  let tags = $state(form.tags.join(", "));
  let error = $state<string | null>(null);
  let busy = $state(false);

  const colors = ["#7B61FF", "#3DDC84", "#FFB020", "#FF5C5C", "#38BDF8", "#F472B6"];

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      form.tags = tags.split(",").map((t) => t.trim()).filter(Boolean);
      form.identity_id = form.identity_id || undefined;
      await vaultStore.saveHost(id, $state.snapshot(form));
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit host" : "New host"} onclose={() => (ui.modal = null)}>
  <form id="host-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div class="col-span-2">
        <label class="label" for="h-label">Label</label>
        <input id="h-label" class="input" bind:value={form.label} required placeholder="prod-db-01" />
      </div>
      <div>
        <label class="label" for="h-host">Hostname / IP</label>
        <input id="h-host" class="input font-mono" bind:value={form.hostname} required placeholder="10.0.0.5" />
      </div>
      <div>
        <label class="label" for="h-port">Port</label>
        <input id="h-port" class="input font-mono" type="number" min="1" max="65535" bind:value={form.port} required />
      </div>
      <div class="col-span-2">
        <label class="label" for="h-identity">Identity</label>
        <select id="h-identity" class="input" bind:value={form.identity_id}>
          <option value={undefined}>Ask when connecting</option>
          {#each vaultStore.identities as ident (ident.id)}
            <option value={ident.id}>{ident.data?.label} ({ident.data?.username})</option>
          {/each}
        </select>
        {#if vaultStore.identities.length === 0}
          <p class="mt-1 text-xs text-fg-muted">No identities yet. Add one under Keychain.</p>
        {/if}
      </div>
      <div>
        <label class="label" for="h-group">Group</label>
        <input id="h-group" class="input" bind:value={form.group} placeholder="Production/Databases" />
      </div>
      <div>
        <label class="label" for="h-tags">Tags</label>
        <input id="h-tags" class="input" bind:value={tags} placeholder="db, eu-west" />
      </div>
      <div class="col-span-2">
        <span class="label">Color</span>
        <div class="flex gap-2">
          {#each colors as c (c)}
            <button
              type="button"
              class="h-6 w-6 rounded-full ring-offset-2 ring-offset-panel {form.color === c ? 'ring-2 ring-fg' : ''}"
              style:background={c}
              onclick={() => (form.color = c)}
              aria-label="Color {c}"
            ></button>
          {/each}
        </div>
      </div>
      <div class="col-span-2">
        <label class="label" for="h-notes">Notes</label>
        <textarea id="h-notes" class="input" rows="2" bind:value={form.notes}></textarea>
      </div>
    </div>
    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="host-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
