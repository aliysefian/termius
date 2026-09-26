<script lang="ts">
  import { KeyRound, Pencil, Plus, Trash2 } from "lucide-svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const authLabel = { password: "Password", private_key: "Private key", agent: "SSH agent" } as const;

  function usage(id: string) {
    return vaultStore.hosts.filter((h) => h.data?.identity_id === id).length;
  }

  async function remove(id: string, label: string) {
    const n = usage(id);
    const msg = n ? `Delete "${label}"? ${n} host(s) reference it and will be detached.` : `Delete "${label}"?`;
    if (!confirm(msg)) return;
    await vaultStore.deleteIdentity(id);
  }
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Keychain</h2>
    <button class="icon-btn" title="New identity" onclick={() => (ui.modal = { kind: "identity", id: null })}>
      <Plus size={16} />
    </button>
  </div>
  <div class="flex-1 overflow-y-auto px-2 pb-4">
    {#if vaultStore.identities.length === 0}
      <div class="px-3 py-10 text-center">
        <KeyRound size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm text-fg-muted">No identities yet.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "identity", id: null })}>
          <Plus size={14} /> Add identity
        </button>
      </div>
    {:else}
      {#each vaultStore.identities as ident (ident.id)}
        {@const d = ident.data!}
        <div class="group flex items-center gap-2.5 rounded-md px-2 py-1.5 hover:bg-panel-hover">
          <KeyRound size={16} class="shrink-0 text-accent" />
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm">{d.label}</div>
            <div class="truncate text-xs text-fg-muted">{d.username} · {authLabel[d.auth.type]} · {usage(ident.id)} hosts</div>
          </div>
          <div class="flex opacity-0 group-hover:opacity-100">
            <button class="icon-btn h-6 w-6" title="Edit" onclick={() => (ui.modal = { kind: "identity", id: ident.id })}><Pencil size={12} /></button>
            <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" onclick={() => remove(ident.id, d.label)}><Trash2 size={12} /></button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</aside>
