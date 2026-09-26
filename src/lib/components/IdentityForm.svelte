<script lang="ts">
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptyIdentity, errorMessage, type AuthMethod, type Identity, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.identityById.get(id)?.data : undefined;
  let form = $state<Identity>(existing ? structuredClone($state.snapshot(existing)) : emptyIdentity());
  let authType = $state<AuthMethod["type"]>(form.auth.type);
  let password = $state(form.auth.type === "password" ? form.auth.password : "");
  let privateKey = $state(form.auth.type === "private_key" ? form.auth.private_key : "");
  let passphrase = $state(form.auth.type === "private_key" ? (form.auth.passphrase ?? "") : "");
  let error = $state<string | null>(null);
  let busy = $state(false);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      const auth: AuthMethod =
        authType === "password"
          ? { type: "password", password }
          : authType === "private_key"
            ? { type: "private_key", private_key: privateKey, passphrase: passphrase || undefined }
            : { type: "agent" };
      await vaultStore.saveIdentity(id, { ...$state.snapshot(form), auth });
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit identity" : "New identity"} onclose={() => (ui.modal = null)}>
  <form id="identity-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="i-label">Label</label>
        <input id="i-label" class="input" bind:value={form.label} required placeholder="deploy key" />
      </div>
      <div>
        <label class="label" for="i-user">Username</label>
        <input id="i-user" class="input font-mono" bind:value={form.username} required placeholder="root" />
      </div>
    </div>

    <div>
      <span class="label">Authentication</span>
      <div class="flex overflow-hidden rounded-md border border-line text-sm">
        {#each [["password", "Password"], ["private_key", "Private key"], ["agent", "SSH agent"]] as [value, label] (value)}
          <button
            type="button"
            class="flex-1 py-1.5 {authType === value ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}"
            onclick={() => (authType = value as AuthMethod["type"])}
          >
            {label}
          </button>
        {/each}
      </div>
    </div>

    {#if authType === "password"}
      <div>
        <label class="label" for="i-pw">Password</label>
        <input id="i-pw" class="input" type="password" bind:value={password} autocomplete="off" />
      </div>
    {:else if authType === "private_key"}
      <div>
        <label class="label" for="i-key">Private key (PEM / OpenSSH)</label>
        <textarea
          id="i-key"
          class="input font-mono text-xs"
          rows="7"
          bind:value={privateKey}
          placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
          spellcheck="false"
        ></textarea>
      </div>
      <div>
        <label class="label" for="i-pass">Key passphrase (optional)</label>
        <input id="i-pass" class="input" type="password" bind:value={passphrase} autocomplete="off" />
      </div>
    {:else}
      <p class="text-sm text-fg-muted">Keys are supplied by the local ssh-agent on each machine.</p>
    {/if}

    <div>
      <label class="label" for="i-notes">Notes</label>
      <textarea id="i-notes" class="input" rows="2" bind:value={form.notes}></textarea>
    </div>

    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="identity-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
