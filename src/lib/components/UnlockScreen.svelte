<script lang="ts">
  import { Eye, EyeOff, FolderSearch, Lock, ShieldCheck } from "lucide-svelte";
  import { pickFolder } from "$lib/api";
  import StrengthMeter from "./StrengthMeter.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  let password = $state("");
  let confirm = $state("");
  let show = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const status = $derived(vaultStore.status);
  const path = $derived("path" in status ? status.path : null);
  const creating = $derived(status.state === "needs_setup");

  async function choose() {
    error = null;
    const dir = await pickFolder("Choose the synced folder for your vault");
    if (!dir) return;
    try {
      await vaultStore.setPath(dir);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    if (creating && password !== confirm) {
      error = "Passwords do not match";
      return;
    }
    busy = true;
    try {
      if (creating) await vaultStore.create(password);
      else await vaultStore.unlock(password);
      password = confirm = "";
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex h-screen items-center justify-center bg-base">
  <div class="w-full max-w-md rounded-2xl border border-line bg-panel p-8 shadow-2xl">
    <div class="mb-6 flex items-center gap-3">
      <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-accent/15 text-accent">
        <ShieldCheck size={22} />
      </div>
      <div>
        <h1 class="text-lg font-semibold">SSHVault</h1>
        <p class="text-xs text-fg-muted">Zero-knowledge. Synced by your own folder.</p>
      </div>
    </div>

    <div class="mb-5">
      <span class="label">Vault folder</span>
      <div class="flex gap-2">
        <div
          class="input flex-1 truncate {path ? '' : 'text-fg-muted/60'}"
          title={path ?? ""}
        >
          {path ?? "Not chosen yet"}
        </div>
        <button class="btn-ghost border border-line" onclick={choose} type="button">
          <FolderSearch size={16} /> Browse
        </button>
      </div>
      <p class="mt-1.5 text-xs text-fg-muted">
        Pick a folder inside Dropbox, Nextcloud or any synced directory. Only encrypted files are written there.
      </p>
    </div>

    {#if path}
      <form onsubmit={submit} class="space-y-4">
        <div>
          <label class="label" for="pw">{creating ? "Choose a master password" : "Master password"}</label>
          <div class="relative">
            <input
              id="pw"
              class="input pr-10"
              type={show ? "text" : "password"}
              bind:value={password}
              autocomplete={creating ? "new-password" : "current-password"}
              required
              minlength={creating ? 8 : undefined}
            />
            <button
              type="button"
              class="absolute right-2 top-1/2 -translate-y-1/2 text-fg-muted hover:text-fg"
              onclick={() => (show = !show)}
              aria-label={show ? "Hide password" : "Show password"}
            >
              {#if show}<EyeOff size={16} />{:else}<Eye size={16} />{/if}
            </button>
          </div>
          {#if creating}<StrengthMeter {password} />{/if}
        </div>

        {#if creating}
          <div>
            <label class="label" for="pw2">Confirm password</label>
            <input id="pw2" class="input" type={show ? "text" : "password"} bind:value={confirm} required />
            <p class="mt-1.5 text-xs text-fg-muted">
              This password is never stored. If you forget it, the vault cannot be recovered.
            </p>
          </div>
        {/if}

        {#if error}
          <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
        {/if}

        <button class="btn-primary w-full" type="submit" disabled={busy || !password}>
          <Lock size={16} />
          {busy ? (creating ? "Creating…" : "Unlocking…") : creating ? "Create vault" : "Unlock"}
        </button>
      </form>
    {/if}
  </div>
</div>
