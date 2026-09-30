<script lang="ts">
  import { Copy, Download, ExternalLink, Loader2, Lock, Palette, RefreshCw, RotateCcw, ShieldAlert, SquareTerminal, TerminalSquare } from "lucide-svelte";
  import { onMount } from "svelte";
  import type { CliStatus } from "$lib/types";
  import { RELEASES_URL, formatSize, installHint, installUpdate, updates } from "$lib/stores/updates.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { SHELL_SNIPPETS } from "$lib/shellintegration";
  import { ui } from "$lib/stores/ui.svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { settings } from "$lib/stores/settings.svelte";
  import { allThemes } from "$lib/themes";
  import { importTheme } from "$lib/themeimport";
  import { open as openFile } from "@tauri-apps/plugin-dialog";
  import * as api from "$lib/api";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type VaultSettings } from "$lib/types";

  let msg = $state<{ ok: boolean; text: string } | null>(null);

  // -- command line ------------------------------------------------------------
  let cli = $state<CliStatus | null>(null);
  onMount(() => void api.cli.status().then((s) => (cli = s)));
  onMount(() => void updates.loadInfo());
  const isWindows = navigator.userAgent.includes("Windows");
  const aliasLine = $derived(
    cli?.executable ? (isWindows ? `Set-Alias sshvault "${cli.executable}"` : `alias sshvault='${cli.executable.replace(/'/g, "'\\''")}'`) : "",
  );

  async function toggleCli() {
    try {
      cli = await api.cli.setEnabled(!cli?.enabled);
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    }
  }

  async function importThemeFile() {
    const f = await openFile({
      multiple: false,
      directory: false,
      title: "Import a colour scheme (VS Code .json, Windows Terminal .json, iTerm2 .itermcolors)",
      filters: [{ name: "Colour schemes", extensions: ["json", "itermcolors"] }],
    });
    if (typeof f !== "string") return;
    try {
      const t = importTheme(await api.readTextFile(f), f.split(/[\\/]/).pop());
      if (!t) throw new Error("That file isn't a VS Code, Windows Terminal or iTerm2 colour scheme.");
      settings.prefs.customThemes = [...settings.prefs.customThemes.filter((x) => x.id !== t.id), t];
      settings.prefs.themeId = t.id;
      msg = { ok: true, text: `Imported "${t.name}".` };
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    }
  }

  function removeTheme(id: string) {
    settings.prefs.customThemes = settings.prefs.customThemes.filter((t) => t.id !== id);
    if (settings.prefs.themeId === id) settings.prefs.themeId = "sshvault";
  }

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
        {#each allThemes(settings.prefs.customThemes) as t (t.id)}
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
            <div class="flex items-center justify-between bg-base px-2 py-1">
              <span class="truncate">{t.name}</span>
              {#if t.id.startsWith("custom-")}
                <span role="button" tabindex="0" class="text-fg-muted hover:text-danger" title="Remove" onclick={(e) => { e.stopPropagation(); removeTheme(t.id); }} onkeydown={(e) => e.key === "Enter" && removeTheme(t.id)}>✕</span>
              {/if}
            </div>
          </button>
        {/each}
      </div>
      <div class="-mt-2 mb-4 flex flex-wrap items-center gap-3 text-xs">
        <button class="btn-ghost border border-line py-1 text-xs" onclick={importThemeFile}>Import colour scheme…</button>
        <label class="flex items-center gap-2 text-fg-muted">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.prodTint} /> Tint production terminals red
        </label>
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

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Updates</h2>
      <p class="mb-3 text-xs text-fg-muted">
        You have version <strong class="text-fg">{updates.info?.version ?? "…"}</strong>. New versions come from the
        <button class="text-accent hover:underline" onclick={() => openUrl(RELEASES_URL)}>SSHVault releases on GitHub</button>
        and are only installed if they match the checksum GitHub publishes{updates.info?.enabled ? " or the signing key built into this app" : ""}.
      </p>
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.autoUpdateCheck} />
        Check for updates when SSHVault starts
      </label>
      <div class="mt-3 flex flex-wrap items-center gap-3">
        <button class="btn-ghost border border-line py-1 text-xs" disabled={updates.checking || updates.installing} onclick={() => updates.check()}>
          {#if updates.checking}<Loader2 size={12} class="animate-spin" /> Checking…{:else}<RefreshCw size={12} /> Check for updates{/if}
        </button>
        {#if updates.available && !updates.installing}
          <button class="btn-primary py-1 text-xs" onclick={() => void installUpdate()}>
            {#if updates.canInstall}<Download size={12} /> Update to {updates.available.version}{:else}<ExternalLink size={12} /> Download {updates.available.version}{/if}
          </button>
        {/if}
        <span class="text-xs {updates.error ? 'text-danger' : 'text-fg-muted'}">
          {updates.error ?? updates.lastResult ?? (updates.lastChecked ? `Last checked ${new Date(updates.lastChecked).toLocaleString()}` : "")}
        </span>
      </div>
      {#if updates.installing}
        <div class="mt-3 max-w-sm">
          <div class="h-1.5 overflow-hidden rounded bg-base">
            <div
              class="h-full bg-accent transition-[width] {updates.progress === null ? 'w-1/3 animate-pulse' : ''}"
              style:width={updates.progress === null ? undefined : `${Math.round(updates.progress * 100)}%`}
            ></div>
          </div>
          <p class="mt-1 text-xs text-fg-muted">{updates.stage} Don't close the app.</p>
        </div>
      {:else if updates.available}
        <p class="mt-2 text-xs text-fg-muted">
          {installHint(updates.available.install)}{updates.available.size ? ` Download size ${formatSize(updates.available.size)}.` : ""}
          <button class="text-accent hover:underline" onclick={() => updates.available && openUrl(updates.available.url)}>What's new</button>
        </p>
      {/if}
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Layout</h2>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="s-density">Density</label>
          <select id="s-density" class="input" bind:value={settings.prefs.density}>
            <option value="comfortable">Comfortable</option>
            <option value="compact">Compact</option>
          </select>
        </div>
        <label class="mt-5 flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={settings.prefs.focusMode} />
          Focus mode <span class="text-xs text-fg-muted">(Ctrl+Shift+U)</span>
        </label>
      </div>
      <p class="mt-2 text-xs text-fg-muted">
        Focus mode hides everything but the terminal. <button class="text-accent hover:underline" onclick={() => (ui.modal = { kind: "shortcuts" })}>Change keyboard shortcuts…</button>
      </p>
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Local terminal</h2>
      <p class="mb-3 text-xs text-fg-muted">Which program local tabs run, and where they start. Empty uses your system's default shell in your home folder.</p>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="s-shell">Shell</label>
          <input id="s-shell" class="input font-mono text-xs" list="s-shells" bind:value={settings.prefs.localShell} placeholder="default" spellcheck="false" />
          <datalist id="s-shells">
            {#each ["bash", "zsh", "fish", "pwsh -NoLogo", "powershell -NoLogo", "cmd", "wsl"] as sh (sh)}<option value={sh}></option>{/each}
          </datalist>
        </div>
        <div>
          <label class="label" for="s-cwd">Start in</label>
          <input id="s-cwd" class="input font-mono text-xs" bind:value={settings.prefs.localCwd} placeholder="~" spellcheck="false" />
        </div>
      </div>
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Connections</h2>
      <label class="flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={settings.prefs.autoReconnect} />
        <span>
          Reconnect automatically when a connection drops
          <span class="block text-xs text-fg-muted">Up to three attempts, a few seconds apart. Typing <code>exit</code> or closing the tab never reconnects.</span>
        </span>
      </label>
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={settings.prefs.notifyBackground} />
        <span>
          Notify me when a long command finishes in a background tab
          <span class="block text-xs text-fg-muted">Needs shell integration (below) to know when commands end; the terminal bell always notifies.</span>
        </span>
      </label>
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={settings.prefs.rememberCommands} />
        <span>
          Remember the commands I run on each host
          <span class="block text-xs text-fg-muted">
            Off by default. Kept as plain text on this computer only, never in the vault, and offered in the command palette. Commands can contain passwords and tokens; turning this off deletes the saved history.
          </span>
        </span>
      </label>
      <button class="btn-ghost mt-2 border border-line py-1 text-xs" onclick={() => { settings.clearHistory(); ui.notify("info", "Command history cleared."); }}>Clear command history</button>
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <div class="flex items-start justify-between gap-3">
        <div>
          <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><SquareTerminal size={15} class="text-accent" /> Command line</h2>
          <p class="text-xs text-fg-muted">
            Use your hosts from scripts and your own terminal while the vault is unlocked. Off by default; only programs
            running as you on this computer can connect, and <code>run</code> asks you here before touching any server.
          </p>
        </div>
        <button class="btn-{cli?.enabled ? 'ghost border border-line' : 'primary'} shrink-0" onclick={toggleCli}>{cli?.enabled ? "Turn off" : "Turn on"}</button>
      </div>
      {#if cli?.enabled}
        <pre class="mt-3 overflow-x-auto rounded-md border border-line bg-base p-2 font-mono text-[11px] leading-snug">sshvault status
sshvault list --group Production
sshvault connect web-01
sshvault run --tag frontend -- uptime
sshvault run web-01 db-01 --json -- df -h /</pre>
        {#if aliasLine}
          <p class="mt-2 text-xs text-fg-muted">If <code>sshvault</code> isn't on your PATH, add this to your shell profile:</p>
          <div class="mt-1 flex items-center gap-2">
            <code class="min-w-0 flex-1 truncate rounded-md border border-line bg-base px-2 py-1.5 font-mono text-xs" title={aliasLine}>{aliasLine}</code>
            <button class="btn-ghost border border-line py-1 text-xs" onclick={() => { void writeText(aliasLine); ui.notify("info", "Copied."); }}><Copy size={12} /> Copy</button>
          </div>
        {/if}
      {/if}
    </section>

    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><TerminalSquare size={15} class="text-accent" /> Shell integration</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Add this to the shell startup file <em>on the servers you connect to</em>. SSHVault then shows the current
        directory in the pane header (with "browse in SFTP" and "new tab here"), records command history (if enabled above) with exit
        codes, jumps between prompts (Ctrl+Shift+↑/↓), copies the last command's output, and can notify you when a
        long command finishes. It's the same OSC 133 / OSC 7 convention VS Code, WezTerm and Kitty use.
      </p>
      <div class="space-y-3">
        {#each SHELL_SNIPPETS as s (s.shell)}
          <div>
            <div class="mb-1 flex items-center justify-between text-xs">
              <span><strong>{s.shell}</strong> <span class="text-fg-muted">· {s.file}</span></span>
              <button class="btn-ghost py-0.5 text-xs" onclick={() => { void writeText(s.text); ui.notify("info", `${s.shell} snippet copied.`); }}><Copy size={12} /> Copy</button>
            </div>
            <pre class="overflow-x-auto rounded-md border border-line bg-base p-2 font-mono text-[11px] leading-snug">{s.text}</pre>
          </div>
        {/each}
      </div>
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
