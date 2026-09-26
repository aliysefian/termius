<script lang="ts">
  import { Copy, Eye, KeyRound, Loader2, Sparkles } from "lucide-svelte";
  import { revealIdentity } from "$lib/secrets.svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptyIdentity, errorMessage, type AuthMethod, type Identity, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // Lists only carry redacted identities. Secrets stay empty here; leaving
  // them empty keeps the stored ones, and Reveal fetches them after the
  // master password is confirmed.
  // svelte-ignore state_referenced_locally
  const stored = id ? vaultStore.identityById.get(id)?.data : undefined;
  let form = $state<Identity>(stored ? structuredClone($state.snapshot(stored)) : emptyIdentity());
  let authType = $state<AuthMethod["type"]>(stored?.auth.type ?? "password");
  let password = $state("");
  let privateKey = $state("");
  let passphrase = $state("");
  let showPassword = $state(false);
  let publicKey = $state<string | null>(null);
  let generating = $state(false);
  let revealing = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const savedPassword = stored?.auth.type === "password";
  const savedKey = stored?.auth.type === "private_key";

  async function reveal() {
    if (!id) return;
    revealing = true;
    try {
      const s = await revealIdentity(id, `Show the secret for "${form.label}"`);
      if (!s) return;
      if (s.password !== null) {
        password = s.password;
        showPassword = true;
      }
      if (s.private_key !== null) privateKey = s.private_key;
      if (s.passphrase) passphrase = s.passphrase;
    } finally {
      revealing = false;
    }
  }

  async function generate() {
    if (privateKey && !confirm("Replace the current private key with a new one?")) return;
    generating = true;
    error = null;
    try {
      const comment = form.username ? `${form.username}@sshvault` : "sshvault";
      const k = await api.keys.generate(comment);
      authType = "private_key";
      privateKey = k.private_key;
      passphrase = "";
      publicKey = k.public_key;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      generating = false;
    }
  }

  async function copyPublic() {
    if (!publicKey) return;
    await writeText(publicKey);
    ui.notify("info", "Public key copied. Add it to ~/.ssh/authorized_keys on the server.");
  }

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
      password = privateKey = passphrase = "";
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
          {#each [["password", "Password"], ["private_key", "Private key"], ["agent", "SSH agent"]] as [value, text] (value)}
            <button
              type="button"
              class="flex-1 py-1.5 {authType === value ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}"
              onclick={() => (authType = value as AuthMethod["type"])}
            >
              {text}
            </button>
          {/each}
        </div>
      </div>

      {#if authType === "password"}
        <div>
          <div class="mb-1 flex items-end justify-between">
            <label class="label mb-0" for="i-pw">Password</label>
            {#if savedPassword && !password}
              <button type="button" class="btn-ghost py-0.5 text-xs" onclick={reveal} disabled={revealing}>
                {#if revealing}<Loader2 size={12} class="animate-spin" />{:else}<Eye size={12} />{/if} Reveal saved
              </button>
            {/if}
          </div>
          <input
            id="i-pw"
            class="input font-mono"
            type={showPassword ? "text" : "password"}
            bind:value={password}
            autocomplete="off"
            placeholder={savedPassword ? "Saved. Leave empty to keep it." : ""}
          />
        </div>
      {:else if authType === "private_key"}
        <div>
          <div class="mb-1 flex items-end justify-between">
            <label class="label mb-0" for="i-key">Private key (OpenSSH, PEM or PuTTY)</label>
            {#if savedKey && !privateKey}
              <button type="button" class="btn-ghost py-0.5 text-xs" onclick={reveal} disabled={revealing}>
                {#if revealing}<Loader2 size={12} class="animate-spin" />{:else}<Eye size={12} />{/if} Reveal saved
              </button>
            {/if}
            <button type="button" class="btn-ghost py-0.5 text-xs" onclick={generate} disabled={generating}>
              {#if generating}<Loader2 size={12} class="animate-spin" />{:else}<Sparkles size={12} />{/if}
              Generate Ed25519 key
            </button>
          </div>
          <textarea
            id="i-key"
            class="input font-mono text-xs"
            rows="7"
            bind:value={privateKey}
            placeholder={savedKey ? "Saved. Leave empty to keep it." : "-----BEGIN OPENSSH PRIVATE KEY-----"}
            spellcheck="false"
          ></textarea>
        </div>
        {#if publicKey}
          <div class="rounded-md border border-accent/30 bg-accent/5 p-3">
            <div class="mb-1 flex items-center justify-between">
              <span class="flex items-center gap-1.5 text-xs font-medium"><KeyRound size={12} class="text-accent" /> Public key</span>
              <button type="button" class="btn-ghost py-0.5 text-xs" onclick={copyPublic}><Copy size={12} /> Copy</button>
            </div>
            <p class="font-mono text-[11px] break-all text-fg-muted">{publicKey}</p>
            <p class="mt-2 text-xs text-fg-muted">Add this line to <code>~/.ssh/authorized_keys</code> on each server, then save.</p>
          </div>
        {/if}
        <div>
          <label class="label" for="i-pass">Key passphrase {savedKey ? "(leave empty to keep)" : "(optional)"}</label>
          <input id="i-pass" class="input" type="password" bind:value={passphrase} autocomplete="off" />
        </div>
      {:else}
        <p class="text-sm text-fg-muted">Keys are supplied by the local ssh-agent on each computer.</p>
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
