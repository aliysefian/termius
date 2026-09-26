<script lang="ts">
  import { Lock, Palette, RotateCcw, ShieldAlert } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { settings } from "$lib/stores/settings.svelte";
  import { themes } from "$lib/themes";
  import * as api from "$lib/api";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type VaultSettings } from "$lib/types";

  let msg = $state<{ ok: boolean; text: string } | null>(null);

  // Vault-wide safety settings, edited as a draft and saved together.
  let draft = $state<VaultSettings | null>(null);
  let patternsText = $state("");
  $effect(() => {
    const s = vaultStore.settings?.settings;
    if (s && !draft) {
      draft = structuredClone($state.snapshot(s));
      patternsText = s.destructive_patterns.join("\n");
    }
  });
  let savingShared = $state(false);

  async function saveShared(e: SubmitEvent) {
    e.preventDefault();
    if (!draft) return;
    savingShared = true;
    msg = null;
    try {
      const patterns = patternsText.split("\n").map((p) => p.trim()).filter(Boolean);
      for (const p of patterns) {
        try {
          new RegExp(p.startsWith("(?i)") ? p.slice(4) : p);
        } catch {
          throw new Error(`Not a valid pattern: ${p}`);
        }
      }
      await vaultStore.saveSettings({ ...$state.snapshot(draft), destructive_patterns: patterns });
      msg = { ok: true, text: "Saved. Every device using this vault picks it up." };
    } catch (err) {
      msg = { ok: false, text: errorMessage(err) };
      draft = null; // reload what's stored
    } finally {
      savingShared = false;
    }
  }

  async function exportToFile() {
    const path = await save({ title: "Export hosts as SSH config", defaultPath: "sshvault.config" });
    if (!path) return;
    try {
      await api.exportSshConfig(path);
      msg = { ok: true, text: `Exported to ${path}. Add "Include ${path}" to ~/.ssh/config.` };
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    }
  }

  async function exportToClipboard() {
    try {
      await writeText(await api.exportSshConfig(null));
      msg = { ok: true, text: "SSH config copied to the clipboard." };
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
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
        {#each [5, 15, 30, 60] as m (m)}
          <option value={m}>After {m < 60 ? `${m} minutes` : "1 hour"}</option>
        {/each}
      </select>
      <p class="mt-2 text-xs text-fg-muted">Lock now, change the master password or manage backups on the <strong>Vault</strong> screen.</p>
    </section>

    {#if draft}
      <form class="rounded-xl border border-line bg-panel p-5" onsubmit={saveShared}>
        <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><ShieldAlert size={15} class="text-accent" /> Safety</h2>
        <p class="mb-4 text-xs text-fg-muted">Stored in the vault and shared by every device that opens it.</p>
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="label" for="s-clip">Clear copied secrets after</label>
            <select id="s-clip" class="input" bind:value={draft.clipboard_clear_secs}>
              {#each [10, 20, 30, 45, 60, 120] as n (n)}<option value={n}>{n} seconds</option>{/each}
            </select>
          </div>
          <div>
            <label class="label" for="s-paste">Confirm pastes of</label>
            <select id="s-paste" class="input" bind:value={draft.paste_confirm_lines}>
              <option value={0}>Never ask</option>
              {#each [2, 3, 5, 10] as n (n)}<option value={n}>{n} or more lines</option>{/each}
            </select>
          </div>
          <div>
            <label class="label" for="s-ret">Keep backups</label>
            <input id="s-ret" class="input" type="number" min="1" max="1000" bind:value={draft.backup_retention} />
          </div>
          <div class="col-span-2">
            <label class="label" for="s-pat">Ask before running these on production hosts <span class="font-normal text-fg-muted">(one regular expression per line)</span></label>
            <textarea id="s-pat" class="input font-mono text-xs" rows="6" bind:value={patternsText} spellcheck="false"></textarea>
            <p class="mt-1 text-xs text-fg-muted">
              A safety net, not a guarantee: typed commands are matched as best the app can see them, so aliases, scripts and
              shell history can bypass it.
            </p>
          </div>
        </div>
        <button class="btn-primary mt-4" type="submit" disabled={savingShared}>{savingShared ? "Saving…" : "Save"}</button>
      </form>
    {/if}

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Use your hosts from the command line</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Export every host as an OpenSSH config, then add <code>Include /path/to/file</code> to <code>~/.ssh/config</code>
        so <code>ssh</code>, <code>scp</code>, Ansible and git use the same names. Keys stay in the vault; the file only has
        names, addresses, users, ports and jump hosts.
      </p>
      <div class="flex gap-2">
        <button class="btn-ghost border border-line" onclick={exportToFile}>Save to file…</button>
        <button class="btn-ghost border border-line" onclick={exportToClipboard}>Copy to clipboard</button>
      </div>
    </section>

    {#if msg}
      <p class="rounded-md border px-3 py-2 text-sm {msg.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}">
        {msg.text}
      </p>
    {/if}
  </div>
</div>
