<script lang="ts">
  import { Copy, Eye, EyeOff, KeyRound, Pencil, Plus, Trash2 } from "lucide-svelte";
  import { copySecret, revealIdentity } from "$lib/secrets.svelte";
  import type { Revealed } from "$lib/types";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { errorMessage } from "$lib/types";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const authLabel = {
    password: "Password",
    private_key: "Private key",
    key: "Key Manager key",
    key_file: "Key file",
    agent: "SSH agent",
  } as const;

  // Credentials entered in a host's own form are listed apart from the
  // shared Keychain so the list doesn't fill up with one entry per host.
  const shared = $derived(vaultStore.identities.filter((i) => !i.data?.for_host));
  const owned = $derived(vaultStore.identities.filter((i) => !!i.data?.for_host));

  // Secrets currently on screen, hidden again automatically.
  const REVEAL_SECS = 60;
  let shown = $state<Record<string, Revealed>>({});
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  function hide(id: string) {
    delete shown[id];
    clearTimeout(timers.get(id));
    timers.delete(id);
  }

  async function reveal(id: string, label: string) {
    if (shown[id]) return hide(id);
    const secret = await revealIdentity(id, `Show the secret for "${label}"`);
    if (!secret) return;
    shown[id] = secret;
    timers.set(id, setTimeout(() => hide(id), REVEAL_SECS * 1000));
  }

  async function copyStored(id: string, label: string, part: "password" | "private_key" | "passphrase") {
    const secret = shown[id] ?? (await revealIdentity(id, `Copy the secret for "${label}"`));
    const value = secret?.[part];
    if (value) await copySecret(value, part === "password" ? "Password" : part === "private_key" ? "Private key" : "Passphrase");
  }

  $effect(() => () => timers.forEach(clearTimeout));

  function usage(id: string) {
    return vaultStore.hosts.filter((h) => h.data?.identity_id === id).length;
  }

  async function copyPublicKey(id: string) {
    try {
      const info = await api.identities.publicKey(id);
      await writeText(info.public_key);
      ui.notify("info", `Public key copied (${info.fingerprint}).`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function copyManagedPublic(keyId: string) {
    const k = vaultStore.keyById.get(keyId)?.data;
    if (!k) return ui.notify("error", "That key was deleted from the Key Manager.");
    await writeText(k.public_key);
    ui.notify("info", `Public key copied (${k.fingerprint}).`);
  }

  async function remove(id: string, label: string) {
    const n = usage(id);
    const msg = n ? `Delete "${label}"? ${n} host(s) reference it and will be detached.` : `Delete "${label}"?`;
    if (!await ask(msg)) return;
    await vaultStore.deleteIdentity(id);
  }
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Credentials</h2>
    <button class="icon-btn" title="New credential" onclick={() => (ui.modal = { kind: "identity", id: null })}>
      <Plus size={16} />
    </button>
  </div>
  <div class="flex-1 overflow-y-auto px-2 pb-4">
    {#if vaultStore.identities.length === 0}
      <div class="px-3 py-10 text-center">
        <KeyRound size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm text-fg-muted">No credentials yet.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "identity", id: null })}>
          <Plus size={14} /> Add credential
        </button>
      </div>
    {:else}
      {#snippet row(ident: (typeof vaultStore.identities)[number], ownerLabel?: string)}
        {@const d = ident.data!}
        <div class="group flex items-center gap-2.5 rounded-md px-2 py-1.5 hover:bg-panel-hover">
          <KeyRound size={16} class="shrink-0 {ownerLabel ? 'text-fg-muted' : 'text-accent'}" />
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm">{ownerLabel ?? d.label}</div>
            <div class="truncate text-xs text-fg-muted">
              {d.username} · {authLabel[d.auth.type]}{ownerLabel ? "" : ` · ${usage(ident.id)} hosts`}
            </div>
          </div>
          <div class="flex {shown[ident.id] ? '' : 'opacity-0'} group-hover:opacity-100">
            {#if d.auth.type === "password" || d.auth.type === "private_key" || d.auth.type === "key_file"}
              <button
                class="icon-btn h-6 w-6 {shown[ident.id] ? 'text-accent' : ''}"
                title={shown[ident.id] ? "Hide" : d.auth.type === "password" ? "Show password" : "Show private key"}
                onclick={() => reveal(ident.id, ownerLabel ?? d.label)}
              >
                {#if shown[ident.id]}<EyeOff size={12} />{:else}<Eye size={12} />{/if}
              </button>
            {/if}
            {#if d.auth.type === "password"}
              <button class="icon-btn h-6 w-6" title="Copy password" onclick={() => copyStored(ident.id, ownerLabel ?? d.label, "password")}><Copy size={12} /></button>
            {:else if d.auth.type === "private_key"}
              <button class="icon-btn h-6 w-6" title="Copy public key" onclick={() => copyPublicKey(ident.id)}><KeyRound size={12} /></button>
            {:else if d.auth.type === "key"}
              {@const kid = d.auth.key_id}
              <button class="icon-btn h-6 w-6" title="Copy public key" onclick={() => copyManagedPublic(kid)}><KeyRound size={12} /></button>
            {/if}
            <button class="icon-btn h-6 w-6" title="Edit" onclick={() => (ui.modal = { kind: "identity", id: ident.id })}><Pencil size={12} /></button>
            <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" onclick={() => remove(ident.id, d.label)}><Trash2 size={12} /></button>
          </div>
        </div>
        {#if shown[ident.id]}
          {@const s = shown[ident.id]}
          <div class="mx-2 mb-2 space-y-1.5 rounded-md border border-accent/30 bg-base p-2 text-xs">
            {#if s.password !== null}
              <div class="flex items-center gap-2">
                <span class="w-16 shrink-0 text-fg-muted">Password</span>
                <code class="min-w-0 flex-1 break-all font-mono select-all">{s.password}</code>
                <button class="icon-btn h-6 w-6" title="Copy password" onclick={() => copySecret(s.password!, "Password")}><Copy size={12} /></button>
              </div>
            {/if}
            {#if s.private_key !== null}
              <div class="flex items-start gap-2">
                <span class="w-16 shrink-0 text-fg-muted">Private key</span>
                <pre class="max-h-28 min-w-0 flex-1 overflow-auto font-mono text-[10px] leading-tight select-all">{s.private_key}</pre>
                <button class="icon-btn h-6 w-6" title="Copy private key" onclick={() => copySecret(s.private_key!, "Private key")}><Copy size={12} /></button>
              </div>
            {/if}
            {#if s.passphrase}
              <div class="flex items-center gap-2">
                <span class="w-16 shrink-0 text-fg-muted">Passphrase</span>
                <code class="min-w-0 flex-1 break-all font-mono select-all">{s.passphrase}</code>
                <button class="icon-btn h-6 w-6" title="Copy passphrase" onclick={() => copySecret(s.passphrase!, "Passphrase")}><Copy size={12} /></button>
              </div>
            {/if}
            <p class="text-[10px] text-fg-muted">Hides automatically in {REVEAL_SECS} seconds.</p>
          </div>
        {/if}
      {/snippet}

      {#each shared as ident (ident.id)}
        {@render row(ident)}
      {/each}
      {#if shared.length === 0}
        <p class="px-2 py-3 text-xs text-fg-muted">No shared credentials yet. Create one to reuse it across hosts.</p>
      {/if}

      {#if owned.length}
        <div class="mt-4 px-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Saved with hosts</div>
        {#each owned as ident (ident.id)}
          {@render row(ident, vaultStore.hostById.get(ident.data!.for_host!)?.data?.label ?? "Removed host")}
        {/each}
      {/if}
    {/if}
  </div>
</aside>
