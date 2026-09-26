<script lang="ts">
  import { FolderSearch, Lock, Palette, RefreshCw, RotateCcw } from "lucide-svelte";
  import KnownHostsSection from "./KnownHostsSection.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { themes } from "$lib/themes";
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
      <div class="mb-1 flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-sm font-semibold"><Palette size={15} class="text-accent" /> Terminal appearance</h2>
        <button class="btn-ghost py-1 text-xs" onclick={() => settings.reset()}><RotateCcw size={12} /> Reset</button>
      </div>
      <p class="mb-4 text-xs text-fg-muted">Saved on this computer only. Changes apply to open terminals immediately.</p>

      <label class="label" for="s-app-theme">App theme</label>
      <select id="s-app-theme" class="input mb-4 max-w-xs" bind:value={settings.prefs.appTheme}>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
        <option value="system">Match system</option>
      </select>

      <span class="label">Terminal colour theme</span>
      <div class="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
        {#each themes as t (t.id)}
          <button
            class="overflow-hidden rounded-md border text-left text-xs {settings.prefs.themeId === t.id ? 'border-accent ring-1 ring-accent' : 'border-line hover:border-fg-muted'}"
            onclick={() => (settings.prefs.themeId = t.id)}
          >
            <div class="flex h-10 flex-col justify-center gap-1 px-2 font-mono text-[10px]" style:background={t.theme.background} style:color={t.theme.foreground}>
              <span><span style:color={t.theme.green}>user@host</span>:<span style:color={t.theme.blue}>~</span>$ ls</span>
              <span class="flex gap-0.5">
                {#each [t.theme.red, t.theme.yellow, t.theme.green, t.theme.cyan, t.theme.blue, t.theme.magenta] as c, i (i)}
                  <span class="h-1.5 w-2.5 rounded-sm" style:background={c}></span>
                {/each}
              </span>
            </div>
            <div class="bg-base px-2 py-1">{t.name}</div>
          </button>
        {/each}
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2">
          <label class="label" for="s-font">Font family</label>
          <input id="s-font" class="input font-mono text-xs" bind:value={settings.prefs.fontFamily} spellcheck="false" />
        </div>
        <div>
          <label class="label" for="s-size">Font size: {settings.prefs.fontSize}px</label>
          <input id="s-size" type="range" min="8" max="28" class="w-full accent-[#7b61ff]" bind:value={settings.prefs.fontSize} />
        </div>
        <div>
          <label class="label" for="s-lh">Line height: {settings.prefs.lineHeight.toFixed(1)}</label>
          <input id="s-lh" type="range" min="1" max="2" step="0.1" class="w-full accent-[#7b61ff]" bind:value={settings.prefs.lineHeight} />
        </div>
        <div>
          <label class="label" for="s-cursor">Cursor</label>
          <select id="s-cursor" class="input" bind:value={settings.prefs.cursorStyle}>
            <option value="block">Block</option>
            <option value="bar">Bar</option>
            <option value="underline">Underline</option>
          </select>
        </div>
        <div>
          <label class="label" for="s-sb">Scrollback lines</label>
          <input id="s-sb" class="input" type="number" min="100" max="100000" step="500" bind:value={settings.prefs.scrollback} />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.cursorBlink} /> Blinking cursor
        </label>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.copyOnSelect} /> Copy on select
        </label>
        <label class="col-span-2 flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.logRaw} /> Session logs keep colours and
          control codes (raw) instead of plain text
        </label>
      </div>
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><Lock size={15} class="text-accent" /> Auto-lock</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Lock the vault after a period without keyboard or mouse activity. Locking closes every terminal, SFTP session and
        forwarding rule.
      </p>
      <select class="input max-w-xs" bind:value={settings.prefs.autoLockMinutes}>
        <option value={0}>Never</option>
        {#each [5, 15, 30, 60, 240] as m (m)}
          <option value={m}>After {m < 60 ? `${m} minutes` : `${m / 60} hour${m > 60 ? "s" : ""}`}</option>
        {/each}
      </select>
    </section>

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
        <div>
          <input class="input" type="password" placeholder="New password (min 8 chars)" bind:value={next} required minlength="8" autocomplete="new-password" />
          <StrengthMeter password={next} />
        </div>
        <input class="input" type="password" placeholder="Confirm new password" bind:value={confirmNext} required autocomplete="new-password" />
        <button class="btn-primary" type="submit" disabled={busy}>{busy ? "Re-encrypting…" : "Change password"}</button>
      </form>
    </section>

    <KnownHostsSection />

    {#if msg}
      <p class="rounded-md border px-3 py-2 text-sm {msg.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}">
        {msg.text}
      </p>
    {/if}
  </div>
</div>
