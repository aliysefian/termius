<script lang="ts">
  import { FolderSearch, RefreshCw } from "lucide-svelte";
  import { pickFolder } from "$lib/api";
  import * as api from "$lib/api";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  const status = $derived(vaultStore.status);
  let current = $state("");
  let next = $state("");
  let confirmNext = $state("");
  let msg = $state<{ ok: boolean; text: string } | null>(null);
  let busy = $state(false);

  async function changeFolder() {
    const dir = await pickFolder("Choose a different vault folder");
    if (!dir) return;
    try {
      await vaultStore.setPath(dir);
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    }
  }

  async function changePassword(e: SubmitEvent) {
    e.preventDefault();
    msg = null;
    if (next !== confirmNext) {
      msg = { ok: false, text: "New passwords do not match" };
      return;
    }
    busy = true;
    try {
      await api.vault.changePassword(current, next);
      current = next = confirmNext = "";
      msg = { ok: true, text: "Master password changed. Every record was re-encrypted." };
    } catch (err) {
      msg = { ok: false, text: errorMessage(err) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-2xl space-y-8">
    <h1 class="text-lg font-semibold">Settings</h1>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Vault folder</h2>
      <p class="mb-3 text-xs text-fg-muted">Encrypted records are written here and synced by your file-sync client.</p>
      <div class="flex gap-2">
        <div class="input flex-1 truncate font-mono text-xs">{"path" in status ? status.path : "—"}</div>
        <button class="btn-ghost border border-line" onclick={changeFolder}><FolderSearch size={16} /> Change</button>
      </div>
      {#if status.state === "unlocked"}
        <p class="mt-2 font-mono text-xs text-fg-muted">vault id {status.vault_id}</p>
      {/if}
      <button class="btn-ghost mt-3" onclick={() => vaultStore.reloadAll()}><RefreshCw size={14} /> Reload from disk</button>
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Master password</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Rotating re-encrypts every record with a fresh salt. Other machines will need the new password next time they unlock.
      </p>
      <form onsubmit={changePassword} class="grid max-w-sm gap-3">
        <input class="input" type="password" placeholder="Current password" bind:value={current} required autocomplete="current-password" />
        <input class="input" type="password" placeholder="New password (min 8 chars)" bind:value={next} required minlength="8" autocomplete="new-password" />
        <input class="input" type="password" placeholder="Confirm new password" bind:value={confirmNext} required autocomplete="new-password" />
        <button class="btn-primary" type="submit" disabled={busy}>{busy ? "Re-encrypting…" : "Change password"}</button>
      </form>
    </section>

    {#if msg}
      <p class="rounded-md border px-3 py-2 text-sm {msg.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}">
        {msg.text}
      </p>
    {/if}
  </div>
</div>
