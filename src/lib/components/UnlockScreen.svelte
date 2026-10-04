<script lang="ts">
  import { onMount } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";
  import { ArrowLeft, Eye, EyeOff, FileInput, FolderOpen, FolderPlus, FolderSearch, History, KeyRound, Lock, ShieldCheck, Smartphone, X } from "lucide-svelte";
  import { pickFolder } from "$lib/api";
  import StrengthMeter from "./StrengthMeter.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, isApiError } from "$lib/types";

  type Screen = "home" | "create" | "open" | "forgot";

  const status = $derived(vaultStore.status);
  const path = $derived("path" in status ? status.path : null);
  const remembered = $derived(status.state === "locked" && status.remembered);
  const needsUpgrade = $derived(status.state === "locked" && status.needs_upgrade);

  // svelte-ignore state_referenced_locally
  let screen = $state<Screen>(status.state === "locked" ? "open" : status.state === "needs_setup" ? "create" : "home");
  /** Open the ssh config importer once the vault is unlocked. */
  let importAfter = $state(false);

  let name = $state("SSHVault");
  let location = $state<string | null>(null);
  let password = $state("");
  let confirm = $state("");
  let recoveryKey = $state("");
  let withRecovery = $state(true);
  // svelte-ignore state_referenced_locally
  let remember = $state(remembered);
  let show = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  function go(s: Screen, opts: { importing?: boolean } = {}) {
    error = null;
    password = confirm = recoveryKey = "";
    screen = s;
    if (opts.importing !== undefined) importAfter = opts.importing;
  }

  function join(dir: string, leaf: string) {
    const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
    return dir.replace(/[\\/]+$/, "") + sep + leaf;
  }

  async function chooseLocation() {
    const dir = await pickFolder("Where should the vault folder be created? (e.g. inside Dropbox)");
    if (dir) location = dir;
  }

  async function chooseExisting() {
    error = null;
    const dir = await pickFolder("Choose your vault folder");
    if (!dir) return;
    try {
      await vaultStore.setPath(dir);
      if (vaultStore.status.state === "needs_setup") error = "There is no vault in that folder. Create one instead, or pick another folder.";
      else settings.markRecentVault(dir);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Just the folder's own name, for a short label; the full path is the title. */
  function folderName(p: string): string {
    return p.split(/[\\/]/).filter(Boolean).at(-1) ?? p;
  }

  async function openRecentVault(vaultPath: string) {
    error = null;
    busy = true;
    try {
      await vaultStore.setPath(vaultPath);
      if (vaultStore.status.state === "needs_setup") {
        error = "There is no vault in that folder anymore.";
        settings.forgetRecentVault(vaultPath);
        return;
      }
      go("open");
      if (remembered) await unlockWithDevice(true);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function finished(created = false) {
    password = confirm = recoveryKey = "";
    if (importAfter) ui.modal = { kind: "import-ssh-config" };
    else if (created) {
      // A brand-new, empty vault: offer a way in, and don't leave the
      // terminal area looking broken with nothing open.
      ui.modal = { kind: "onboarding" };
      ui.openLocal();
    }
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    if (password !== confirm) return void (error = "The passwords don't match");
    busy = true;
    try {
      if (status.state !== "needs_setup") {
        if (!location) throw new Error("Choose where to create the vault");
        if (!name.trim() || /[\\/:*?"<>|]/.test(name)) throw new Error("Give the vault a folder name without / \\ : * ? \" < > |");
        await vaultStore.setPath(join(location, name.trim()));
        if (vaultStore.status.state === "locked") {
          screen = "open";
          throw new Error("A vault already exists in that folder. Enter its password to open it.");
        }
      }
      await vaultStore.create(password, withRecovery, remember);
      finished(true);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function unlock(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      await vaultStore.unlock(password, remember);
      finished();
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function unlockWithDevice(quiet = false) {
    error = null;
    busy = true;
    try {
      await vaultStore.unlockWithDevice();
      finished();
    } catch (err) {
      // A missing or stale stored key just means "use the password".
      if (!quiet || !isApiError(err) || err.code !== "not_remembered") error = errorMessage(err);
      await vaultStore.refreshStatus();
    } finally {
      busy = false;
    }
  }

  async function recover(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    if (password !== confirm) return void (error = "The new passwords don't match");
    busy = true;
    try {
      await vaultStore.unlockWithRecovery(recoveryKey, password, remember);
      ui.notify("info", "Master password reset. Your recovery key still works; you can replace it on the Vault screen.");
      finished();
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  // "Remember on this device": try the OS keychain once, quietly.
  onMount(() => {
    if (remembered && !needsUpgrade) void unlockWithDevice(true);
  });
</script>

{#snippet pw(id: string, label: string, value: string, set: (v: string) => void, auto: HTMLInputAttributes["autocomplete"], meter = false)}
  <div>
    <label class="label" for={id}>{label}</label>
    <div class="relative">
      <input
        {id}
        class="input pr-10"
        type={show ? "text" : "password"}
        {value}
        oninput={(e) => set(e.currentTarget.value)}
        autocomplete={auto}
        required
        minlength={meter ? 8 : undefined}
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
    {#if meter}<StrengthMeter password={value} />{/if}
  </div>
{/snippet}

{#snippet rememberBox()}
  <label class="flex items-start gap-2 text-sm">
    <input type="checkbox" class="mt-0.5 accent-input" bind:checked={remember} />
    <span>
      Remember on this device
      <span class="block text-xs text-fg-muted">
        Keeps the vault key in this computer's credential store (Keychain, Credential Manager or Secret Service), not the
        password. Anyone who can use your account here can open the vault.
      </span>
    </span>
  </label>
{/snippet}

{#snippet problem()}
  {#if error}
    <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>
  {/if}
{/snippet}

<div class="flex min-h-screen items-center justify-center bg-base p-4">
  <div class="w-full max-w-md rounded-2xl border border-line bg-panel p-8 shadow-2xl">
    <div class="mb-6 flex items-center gap-3">
      <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-accent/15 text-accent">
        <ShieldCheck size={22} />
      </div>
      <div>
        <h1 class="text-lg font-semibold">SSHVault</h1>
        <p class="text-xs text-fg-muted">End-to-end encrypted. Synced by any folder you choose.</p>
      </div>
    </div>

    {#if screen === "home"}
      <div class="space-y-2">
        <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => go("create", { importing: false })}>
          <FolderPlus size={20} class="text-accent" />
          <span><span class="block text-sm font-medium">Create New Vault</span><span class="text-xs text-fg-muted">Start fresh in a folder you sync (Dropbox, OneDrive, Nextcloud, Syncthing…)</span></span>
        </button>
        <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => go("open", { importing: false })}>
          <FolderOpen size={20} class="text-accent" />
          <span><span class="block text-sm font-medium">Open Existing Vault</span><span class="text-xs text-fg-muted">Use a vault created on another computer</span></span>
        </button>
        <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => go("create", { importing: true })}>
          <FileInput size={20} class="text-accent" />
          <span><span class="block text-sm font-medium">Import SSH Config</span><span class="text-xs text-fg-muted">Create a vault, then bring in hosts from ~/.ssh/config</span></span>
        </button>
      </div>
      {#if settings.recentVaults.length}
        <div class="mt-5">
          <div class="mb-1.5 flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">
            <History size={11} /> Recent vaults
          </div>
          <div class="space-y-1">
            {#each settings.recentVaults as p (p)}
              <div class="group relative flex items-center rounded-md hover:bg-panel-hover">
                <button class="min-w-0 flex-1 truncate rounded-md px-2 py-1.5 text-left text-xs" title={p} disabled={busy} onclick={() => openRecentVault(p)}>
                  {folderName(p)}
                  <span class="block truncate text-fg-muted/70">{p}</span>
                </button>
                <button
                  class="icon-btn reveal mr-1 h-6 w-6 shrink-0"
                  title="Remove from this list"
                  aria-label="Remove {folderName(p)} from recent vaults"
                  onclick={(e) => { e.stopPropagation(); settings.forgetRecentVault(p); }}
                >
                  <X size={12} />
                </button>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      <div class="mt-3">{@render problem()}</div>
    {:else}
      <button class="btn-ghost -ml-2 mb-3 py-1 text-xs" onclick={() => go(screen === "forgot" ? "open" : "home")}>
        <ArrowLeft size={13} /> Back
      </button>
    {/if}

    {#if screen === "create"}
      <form onsubmit={create} class="space-y-4">
        <h2 class="text-sm font-semibold">{importAfter ? "Create a vault for your imported hosts" : "Create New Vault"}</h2>
        {#if status.state === "needs_setup"}
          <div>
            <span class="label">Vault folder</span>
            <div class="input truncate font-mono text-xs" title={path ?? ""}>{path}</div>
          </div>
        {:else}
          <div>
            <label class="label" for="v-name">Vault name</label>
            <input id="v-name" class="input" bind:value={name} required placeholder="MySSHVault" />
          </div>
          <div>
            <span class="label">Location</span>
            <div class="flex gap-2">
              <div class="input flex-1 truncate font-mono text-xs {location ? '' : 'text-fg-muted/60'}" title={location ?? ""}>
                {location ? join(location, name || "…") : "Choose a folder, e.g. ~/Dropbox"}
              </div>
              <button class="btn-secondary" type="button" onclick={chooseLocation}><FolderSearch size={16} /> Browse</button>
            </div>
            <p class="mt-1 text-xs text-fg-muted">Only encrypted files are written there. The sync service never sees your data or password.</p>
          </div>
        {/if}
        {@render pw("c-pw", "Master password", password, (v) => (password = v), "new-password", true)}
        {@render pw("c-pw2", "Confirm master password", confirm, (v) => (confirm = v), "new-password")}
        <label class="flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" bind:checked={withRecovery} />
          <span>
            Create a recovery key
            <span class="block text-xs text-fg-muted">
              A one-time code that can reset the master password. Without one, a forgotten password means the vault is lost
              for good.
            </span>
          </span>
        </label>
        {@render rememberBox()}
        {@render problem()}
        <button class="btn-primary w-full" type="submit" disabled={busy || !password}>
          <Lock size={16} /> {busy ? "Creating…" : "Create vault"}
        </button>
      </form>
    {:else if screen === "open"}
      <div class="space-y-4">
        <h2 class="text-sm font-semibold">Open Existing Vault</h2>
        <div>
          <span class="label">Vault folder</span>
          <div class="flex gap-2">
            <div class="input flex-1 truncate font-mono text-xs {path ? '' : 'text-fg-muted/60'}" title={path ?? ""}>{path ?? "Not chosen yet"}</div>
            <button class="btn-secondary" type="button" onclick={chooseExisting}><FolderSearch size={16} /> Browse</button>
          </div>
        </div>
        {#if needsUpgrade}
          <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
            This vault uses the previous format. It will be upgraded when you unlock it, after a backup of the original is
            kept in <code>backups/</code>. Update SSHVault on your other computers too; older versions can't open the new format.
          </p>
        {/if}
        {#if status.state === "locked"}
          <form onsubmit={unlock} class="space-y-4">
            {@render pw("o-pw", "Master password", password, (v) => (password = v), "current-password")}
            {@render rememberBox()}
            {@render problem()}
            <button class="btn-primary w-full" type="submit" disabled={busy || !password}>
              <Lock size={16} /> {busy ? "Unlocking…" : needsUpgrade ? "Unlock and upgrade" : "Unlock"}
            </button>
          </form>
          <div class="flex items-center justify-between text-xs">
            {#if remembered}
              <button class="btn-ghost py-1 text-xs" onclick={() => unlockWithDevice()} disabled={busy}><Smartphone size={13} /> Unlock with this device</button>
            {:else}<span></span>{/if}
            <button class="btn-ghost py-1 text-xs" onclick={() => go("forgot")}><KeyRound size={13} /> Forgot password?</button>
          </div>
        {:else}
          {@render problem()}
        {/if}
      </div>
    {:else if screen === "forgot"}
      <form onsubmit={recover} class="space-y-4">
        <h2 class="text-sm font-semibold">Reset the master password</h2>
        <p class="text-xs text-fg-muted">
          Enter the recovery key you saved when the vault was created. Your data stays as it is; only the password changes,
          on every device.
        </p>
        <div>
          <label class="label" for="r-key">Recovery key</label>
          <textarea
            id="r-key"
            class="input font-mono text-xs"
            rows="3"
            bind:value={recoveryKey}
            required
            spellcheck="false"
            autocomplete="off"
            placeholder="XXXX-XXXX-XXXX-…"
          ></textarea>
        </div>
        {@render pw("r-pw", "New master password", password, (v) => (password = v), "new-password", true)}
        {@render pw("r-pw2", "Confirm new password", confirm, (v) => (confirm = v), "new-password")}
        {@render rememberBox()}
        {@render problem()}
        <button class="btn-primary w-full" type="submit" disabled={busy || !recoveryKey || !password}>
          <KeyRound size={16} /> {busy ? "Checking…" : "Reset password and unlock"}
        </button>
      </form>
    {/if}
  </div>
</div>
