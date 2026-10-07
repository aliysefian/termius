<script lang="ts">
  import HighlightRules from "./HighlightRules.svelte";
  import specSource from "$lib/completion/specs/SOURCE.json";
  import { ChevronDown, ChevronUp, Copy, Download, ExternalLink, Loader2, Lock, Palette, RefreshCw, RotateCcw, Search, ShieldAlert, SquareTerminal, TerminalSquare, X } from "lucide-svelte";
  import { onMount } from "svelte";
    import { RELEASES_URL, formatSize, installHint, installUpdate, updates } from "$lib/stores/updates.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { SHELL_SNIPPETS } from "$lib/shellintegration";
  import { ui } from "$lib/stores/ui.svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { localShells } from "$lib/stores/localshells.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { allThemes } from "$lib/themes";
  import { LOCALES, t as tr, type Key } from "$lib/i18n/index.svelte";
  import { ALWAYS_SHOWN, RAIL_GROUPS, arrange as arrangeRail, move as moveRail } from "$lib/railitems";
  import { importTheme } from "$lib/themeimport";
  import { open as openFile } from "@tauri-apps/plugin-dialog";
  import * as api from "$lib/api";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type VaultSettings } from "$lib/types";
  import { ask } from "$lib/dialogs.svelte";
  import Badge from "./Badge.svelte";
  import TerminalPreview from "./TerminalPreview.svelte";

  async function resetAppearance() {
    if (await ask("Put the theme, font, cursor, scrollback and density back to their defaults? Shortcuts and imported themes stay.", { title: "Reset appearance", confirm: "Reset" })) settings.resetAppearance();
  }

  async function resetAll() {
    if (await ask("Reset every setting on this computer to its default? Your custom shortcuts and imported colour themes are removed too.", { title: "Reset all settings", confirm: "Reset everything", danger: true })) settings.reset();
  }

  const CATEGORIES = [
    { id: "appearance", label: "Appearance" },
    { id: "terminal", label: "Terminal" },
    { id: "connections", label: "Connections" },
    { id: "safety", label: "Safety" },
    { id: "integrations", label: "Integrations" },
    { id: "updates", label: "Updates" },
    { id: "advanced", label: "Advanced" },
  ] as const;
  type Category = (typeof CATEGORIES)[number]["id"];
  let activeCategory = $state<Category>("appearance");
  // svelte-ignore state_referenced_locally
  let searchQuery = $state(ui.settingsQuery);
  // From the command palette: open with a setting already searched for. Taken once, then cleared.
  $effect(() => {
    if (ui.settingsQuery) {
      searchQuery = ui.settingsQuery;
      ui.settingsQuery = "";
    }
  });

  /** While searching, every category's matching sections show at once. */
  function visible(category: Category, keywords: string): boolean {
    const q = searchQuery.trim().toLowerCase();
    if (q) return keywords.toLowerCase().includes(q);
    return category === activeCategory;
  }

  const railName = (view: string) => tr(`rail.${view}` as Key);
  const railView = $derived(arrangeRail(RAIL_GROUPS, settings.prefs.railOrder, settings.prefs.railHidden));
  const searching = $derived(!!searchQuery.trim());

  const SIDEBAR_KEYWORDS = "sidebar rail icons labels hide show reorder order pin manage layout";
  const OPS_KEYWORDS = "alerts alert notify notification host down cpu memory disk threshold quiet hours mute monitoring history charts keep week day operations";
  const SMART_COMPLETION_KEYWORDS = "smart completion autocomplete auto complete suggestions suggest ghost inline tab history snippets options paths";

  // Mirrors the keyword strings each section's `visible()` guard uses, just
  // to say "no matches" when a search comes up empty.
  const ALL_KEYWORDS = [
    "terminal appearance theme colour color dark light font family size letter spacing padding contrast bold bright cursor block bar underline blink scrollback copy select clipboard osc 52 remote session log raw preview screen reader accessibility word separator double-click",
    "auto-lock inactivity lock timeout minutes",
    "update check version release install download changelog whats new",
    "layout density compact comfortable focus mode shortcuts",
    "local terminal shell bash zsh fish powershell wsl start folder cwd",
    SMART_COMPLETION_KEYWORDS,
    OPS_KEYWORDS,
    SIDEBAR_KEYWORDS,
    "connections auto-reconnect notify background command history remember restore session reopen tabs last time production paste trailing newline",
    "command line cli scripting sshvault run list connect",
    "shell integration osc 133 7 prompt directory",
    "safety clipboard clear paste confirm destructive production backup retention pattern",
    "export ssh config openssh include scp ansible git",
    "reset default settings",
  ];
  const anyMatch = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return !q || ALL_KEYWORDS.some((k) => k.includes(q));
  });

  type Msg = { ok: boolean; text: string } | null;
  // Each section keeps its own message, shown right under its own controls
  // (a shared one used to surface far from whatever action set it).
  let themeMsg = $state<Msg>(null);
  let cliMsg = $state<Msg>(null);
  let exportMsg = $state<Msg>(null);
  let safetyMsg = $state<Msg>(null);

  // -- command line ------------------------------------------------------------
  const cli = $derived(vaultStore.cliStatus);
  onMount(() => void updates.loadInfo());
  const isWindows = navigator.userAgent.includes("Windows");
  const aliasLine = $derived(
    cli?.executable ? (isWindows ? `Set-Alias sshvault "${cli.executable}"` : `alias sshvault='${cli.executable.replace(/'/g, "'\\''")}'`) : "",
  );

  async function toggleCli() {
    try {
      await vaultStore.setCliEnabled(!cli?.enabled);
    } catch (e) {
      cliMsg = { ok: false, text: errorMessage(e) };
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
      themeMsg = { ok: true, text: `Imported "${t.name}".` };
    } catch (e) {
      themeMsg = { ok: false, text: errorMessage(e) };
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
    safetyMsg = null;
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
      safetyMsg = { ok: true, text: "Saved. Every device using this vault picks it up." };
    } catch (err) {
      safetyMsg = { ok: false, text: errorMessage(err) };
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
      exportMsg = { ok: true, text: `Exported to ${path}. Add "Include ${path}" to ~/.ssh/config.` };
    } catch (e) {
      exportMsg = { ok: false, text: errorMessage(e) };
    }
  }

  async function exportToClipboard() {
    try {
      await writeText(await api.exportSshConfig(null));
      exportMsg = { ok: true, text: "SSH config copied to the clipboard." };
    } catch (e) {
      exportMsg = { ok: false, text: errorMessage(e) };
    }
  }

  void localShells.ensure();
</script>

{#snippet msgBox(m: Msg)}
  {#if m}
    <p class="mt-2 rounded-md border px-3 py-2 text-sm {m.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}">
      {m.text}
    </p>
  {/if}
{/snippet}

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto flex max-w-5xl gap-8">
    <nav class="w-40 shrink-0 space-y-4">
      <h1 class="text-lg font-semibold">Settings</h1>
      <div class="relative">
        <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input py-1.5 pl-8 text-sm" placeholder="Search…" bind:value={searchQuery} />
      </div>
      <div class="flex flex-col gap-0.5">
        {#each CATEGORIES as c (c.id)}
          <button
            class="rounded-md px-2.5 py-1.5 text-left text-sm {!searching && activeCategory === c.id ? 'bg-accent/15 text-accent' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'} {searching ? 'opacity-50' : ''}"
            disabled={searching}
            onclick={() => (activeCategory = c.id)}
          >
            {c.label}
          </button>
        {/each}
      </div>
    </nav>

    <div class="min-w-0 max-w-3xl flex-1 space-y-8 pb-8">
    {#if searching}
      <h1 class="text-lg font-semibold">Search results</h1>
      {#if !anyMatch}
        <p class="text-sm text-fg-muted">No settings match "{searchQuery.trim()}".</p>
      {/if}
    {/if}

    {#if visible("appearance", "terminal appearance theme colour color dark light font family size letter spacing padding contrast bold bright cursor block bar underline blink scrollback copy select clipboard osc 52 remote session log raw preview screen reader accessibility word separator double-click highlight words rules notify trigger command blocks margin bar images pictures sixel")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <div class="mb-1 flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-sm font-semibold"><Palette size={15} class="text-accent" /> Terminal appearance</h2>
        <button class="btn-ghost py-1 text-xs" onclick={resetAppearance}><RotateCcw size={12} /> Reset</button>
      </div>
      <p class="mb-4 text-xs text-fg-muted">Saved on this computer only. Changes apply to open terminals immediately.</p>

      <label class="label" for="s-app-theme">App theme</label>
      <select id="s-app-theme" class="input mb-4 max-w-xs" bind:value={settings.prefs.appTheme}>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
        <option value="system">Match system</option>
        <option value="contrast">High contrast (dark)</option>
      </select>

      <label class="label" for="s-language">{tr("settings.language")} / Language</label>
      <select id="s-language" class="input max-w-xs" bind:value={settings.prefs.language}>
        <option value="auto">{tr("settings.language.auto")}</option>
        {#each LOCALES as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
      </select>
      <p class="mb-4 mt-1 text-xs text-fg-muted">{tr("settings.language.note")}</p>

      <span class="label">Terminal colour theme</span>
      <div class="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
        {#each allThemes(settings.prefs.customThemes) as t (t.id)}
          <!-- The card is not itself a button: Remove is a second control, so the card is a box with the
               choosing button filling it and Remove laid over its corner. -->
          <div class="relative overflow-hidden rounded-md border text-left text-xs {settings.prefs.themeId === t.id ? 'border-accent ring-1 ring-accent' : 'border-line hover:border-fg-muted'}">
            <button type="button" class="block w-full text-left" aria-pressed={settings.prefs.themeId === t.id} onclick={() => (settings.prefs.themeId = t.id)}>
              <span class="flex h-10 flex-col justify-center gap-1 px-2 font-mono text-[10px]" style:background={t.theme.background} style:color={t.theme.foreground}>
                <span>user@host:~$ ls</span>
                <span class="flex gap-0.5">
                  {#each [t.theme.red, t.theme.yellow, t.theme.green, t.theme.cyan, t.theme.blue, t.theme.magenta] as c, i (i)}
                    <span class="h-1.5 w-2.5 rounded-sm" style:background={c}></span>
                  {/each}
                </span>
              </span>
              <span class="block truncate bg-base px-2 py-1.5 {t.id.startsWith('custom-') ? 'pr-7' : ''}">{t.name}</span>
            </button>
            {#if t.id.startsWith("custom-")}
              <button type="button" class="icon-btn absolute bottom-0.5 right-0.5 h-5 w-5 text-fg-muted hover:text-danger" aria-label="Remove {t.name}" title="Remove" onclick={() => removeTheme(t.id)}>
                <X size={11} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
      <div class="-mt-2 mb-4 flex flex-wrap items-center gap-3 text-xs">
        <button class="btn-secondary py-1 text-xs" onclick={importThemeFile}>Import colour scheme…</button>
        <label class="flex items-center gap-2 text-fg-muted">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.prodTint} /> Tint production terminals red
        </label>
      </div>
      {@render msgBox(themeMsg)}

      <div class="mb-4">
        <span class="label">Preview</span>
        <TerminalPreview />
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2">
          <label class="label" for="s-font">Font family</label>
          <input id="s-font" class="input font-mono text-xs" bind:value={settings.prefs.fontFamily} spellcheck="false" />
        </div>
        <div>
          <label class="label" for="s-size">Font size: {settings.prefs.fontSize}px</label>
          <input id="s-size" type="range" min="8" max="28" class="w-full accent-input" bind:value={settings.prefs.fontSize} />
        </div>
        <div>
          <label class="label" for="s-lh">Line height: {settings.prefs.lineHeight.toFixed(1)}</label>
          <input id="s-lh" type="range" min="1" max="2" step="0.1" class="w-full accent-input" bind:value={settings.prefs.lineHeight} />
        </div>
        <div>
          <label class="label" for="s-ls">Letter spacing: {settings.prefs.letterSpacing}px</label>
          <input id="s-ls" type="range" min="-1" max="5" step="0.5" class="w-full accent-input" bind:value={settings.prefs.letterSpacing} />
        </div>
        <div>
          <label class="label" for="s-pad">Padding: {settings.prefs.terminalPadding}px</label>
          <input id="s-pad" type="range" min="0" max="24" class="w-full accent-input" bind:value={settings.prefs.terminalPadding} />
        </div>
        <div>
          <label class="label" for="s-contrast">Minimum contrast ratio: {settings.prefs.minimumContrastRatio}</label>
          <input id="s-contrast" type="range" min="1" max="21" class="w-full accent-input" bind:value={settings.prefs.minimumContrastRatio} />
          <p class="mt-0.5 text-[11px] text-fg-muted">Brightens low-contrast text (dim colours on a dark background) for readability. 1 leaves colours as the theme sets them.</p>
        </div>
        <div>
          <label class="label" for="s-cursor-color">Cursor colour</label>
          <div class="flex items-center gap-2">
            <input id="s-cursor-color" type="color" class="h-9 w-10 shrink-0 cursor-pointer rounded-md border border-line bg-base p-0.5" value={settings.prefs.cursorColor || "#ffffff"} oninput={(e) => (settings.prefs.cursorColor = e.currentTarget.value)} />
            <button class="btn-secondary py-1 text-xs" disabled={!settings.prefs.cursorColor} onclick={() => (settings.prefs.cursorColor = "")}>Use theme's</button>
          </div>
        </div>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.boldAsBright} /> Bold text uses the bright colour
        </label>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.screenReaderMode} /> Screen reader mode
        </label>
        <div class="col-span-2">
          <label class="label" for="s-wordsep">Double-click word boundaries</label>
          <input id="s-wordsep" class="input font-mono text-xs" bind:value={settings.prefs.wordSeparator} placeholder="Default (space and common punctuation)" spellcheck="false" />
          <p class="mt-0.5 text-[11px] text-fg-muted">Extra characters that end a double-click selection, e.g. add <code>/</code> to stop at path separators. Empty keeps the default.</p>
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
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.cursorBlink} /> Blinking cursor
        </label>
        <label class="flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.copyOnSelect} /> Copy on select
        </label>
        <label class="col-span-2 flex items-center gap-2 text-sm" title="A thin bar beside each command: green when it worked, red when it failed. Click it to copy the output or pin it. Needs shell integration.">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.commandBlocks} /> Mark each command's output in the margin (shell integration)
        </label>
        <HighlightRules />
        <label class="col-span-2 flex items-center gap-2 text-sm" title="Sixel and the iTerm2 protocol (imgcat, chafa, timg). Applies to terminals opened after the change.">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.terminalImages} /> Show pictures a program draws in the terminal
        </label>
        <label class="col-span-2 flex items-center gap-2 text-sm" title="OSC 52. tmux (with set-clipboard on), Claude Code and Neovim copy this way.">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.remoteClipboard} /> Let programs in the terminal copy to the clipboard
        </label>
        <label class="col-span-2 flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.logRaw} /> Session logs keep colours and
          control codes (raw) instead of plain text
        </label>
      </div>
    </section>
    {/if}

    {#if visible("connections", "auto-lock inactivity lock timeout minutes")}
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
    {/if}

    {#if visible("updates", "update check version release install download changelog whats new")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Updates</h2>
      <p class="mb-3 text-xs text-fg-muted">
        You have version <strong class="text-fg">{updates.info?.version ?? "…"}</strong>. New versions come from the
        <button class="text-accent hover:underline" onclick={() => openUrl(RELEASES_URL)}>SSHVault releases on GitHub</button>
        and are only installed if they match the checksum GitHub publishes{updates.info?.enabled ? " or the signing key built into this app" : ""}.
        <button class="text-accent hover:underline" onclick={() => (ui.view = "changelog")}>View changelog</button>
      </p>
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" class="accent-input" bind:checked={settings.prefs.autoUpdateCheck} />
        Check for updates when SSHVault starts
      </label>
      <div class="mt-3 flex flex-wrap items-center gap-3">
        <button class="btn-secondary py-1 text-xs" disabled={updates.checking || updates.installing} onclick={() => updates.check()}>
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
    {/if}

    {#if visible("appearance", SIDEBAR_KEYWORDS)}
    <section class="rounded-xl border border-line bg-panel p-5">
      <div class="mb-1 flex items-center justify-between">
        <h2 class="text-sm font-semibold">Sidebar</h2>
        <button class="btn-ghost py-1 text-xs" onclick={() => { settings.prefs.railOrder = []; settings.prefs.railHidden = []; settings.prefs.railLabels = true; }}>Reset</button>
      </div>
      <p class="mb-3 text-xs text-fg-muted">Choose what the icons on the left show and in what order. An entry you hide moves into the <strong>Manage</strong> menu, so it is still one click away. Hosts always stays.</p>
      <label class="mb-3 flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={settings.prefs.railLabels} /> Show a name under each icon</label>
      <ul class="space-y-3" aria-label="Sidebar entries">
        {#each railView.groups as g, gi (gi)}
          <li>
            <ul class="divide-y divide-line/60 rounded-md border border-line">
              {#each g as item (item.view)}
                <li class="flex items-center gap-3 px-3 py-1.5 text-sm">
                  <item.icon size={16} class="shrink-0 text-fg-muted" />
                  <span class="min-w-0 flex-1 truncate">{railName(item.view)}</span>
                  <label class="flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" checked disabled={ALWAYS_SHOWN.includes(item.view)} aria-label="Show {railName(item.view)} in the sidebar" onchange={() => (settings.prefs.railHidden = [...settings.prefs.railHidden, item.view])} /> shown</label>
                  <button class="icon-btn h-6 w-6" aria-label="Move {railName(item.view)} up" disabled={g[0] === item} onclick={() => (settings.prefs.railOrder = moveRail(RAIL_GROUPS, settings.prefs.railOrder, item.view, -1))}><ChevronUp size={14} /></button>
                  <button class="icon-btn h-6 w-6" aria-label="Move {railName(item.view)} down" disabled={g[g.length - 1] === item} onclick={() => (settings.prefs.railOrder = moveRail(RAIL_GROUPS, settings.prefs.railOrder, item.view, 1))}><ChevronDown size={14} /></button>
                </li>
              {/each}
            </ul>
          </li>
        {/each}
        {#if railView.away.length}
          <li>
            <div class="mb-1 text-xs font-medium text-fg-muted">In the Manage menu</div>
            <ul class="divide-y divide-line/60 rounded-md border border-line">
              {#each railView.away as item (item.view)}
                <li class="flex items-center gap-3 px-3 py-1.5 text-sm">
                  <item.icon size={16} class="shrink-0 text-fg-muted" />
                  <span class="min-w-0 flex-1 truncate">{railName(item.view)}</span>
                  <label class="flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" aria-label="Show {railName(item.view)} in the sidebar" onchange={() => (settings.prefs.railHidden = settings.prefs.railHidden.filter((v) => v !== item.view))} /> shown</label>
                </li>
              {/each}
            </ul>
          </li>
        {/if}
      </ul>
    </section>
    {/if}

    {#if visible("appearance", "layout density compact comfortable focus mode shortcuts")}
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
          <input type="checkbox" class="accent-input" bind:checked={settings.prefs.focusMode} />
          Focus mode <span class="text-xs text-fg-muted">(Ctrl+Shift+U)</span>
        </label>
      </div>
      <p class="mt-2 text-xs text-fg-muted">
        Focus mode hides everything but the terminal. <button class="text-accent hover:underline" onclick={() => (ui.modal = { kind: "shortcuts" })}>Change keyboard shortcuts…</button>
      </p>
    </section>
    {/if}

    {#if visible("terminal", "local terminal shell bash zsh fish powershell wsl start folder cwd")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Local terminal</h2>
      <p class="mb-3 text-xs text-fg-muted">Which program local tabs run, and where they start. Empty uses your system's default shell in your home folder.</p>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="s-shell-id">Default shell</label>
          <select id="s-shell-id" class="input text-xs" bind:value={settings.prefs.localShellId} onfocus={() => void localShells.ensure()}>
            <option value="">{settings.prefs.localShell.trim() ? "Custom command" : "System default"}</option>
            {#each localShells.list as sh (sh.id)}<option value={sh.id}>{sh.label}</option>{/each}
            {#if settings.prefs.localShellId && !localShells.list.some((s) => s.id === settings.prefs.localShellId)}
              <option value={settings.prefs.localShellId}>{settings.prefs.localShellId} (not installed)</option>
            {/if}
          </select>
        </div>
        <div>
          <label class="label" for="s-cwd">Start in</label>
          <input id="s-cwd" class="input font-mono text-xs" bind:value={settings.prefs.localCwd} placeholder="~" spellcheck="false" />
        </div>
        <div class="col-span-2">
          <label class="label" for="s-shell">Custom command</label>
          <input id="s-shell" class="input font-mono text-xs" bind:value={settings.prefs.localShell} placeholder="used when no default shell is chosen, e.g. pwsh -NoLogo" spellcheck="false" />
        </div>
      </div>
    </section>
    {/if}

    {#if visible("connections", "connections auto-reconnect notify background command history remember restore session reopen tabs last time production paste trailing newline")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Connections</h2>
      <label class="flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.autoReconnect} />
        <span>
          Reconnect automatically when a connection drops
          <span class="block text-xs text-fg-muted">Up to three attempts, a few seconds apart. Typing <code>exit</code> or closing the tab never reconnects.</span>
        </span>
      </label>
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.restoreLastSession} />
        <span>
          Reopen the tabs that were open last time, after unlocking
          <span class="block text-xs text-fg-muted">Only when nothing's open yet. The targets are remembered on this computer, never in the vault; saved passwords and keys apply as usual.</span>
        </span>
      </label>
      {#if settings.prefs.restoreLastSession}
        <label class="ml-6 mt-2 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.restoreSkipProduction} />
          <span>
            Leave production hosts closed; reconnect them by hand
          </span>
        </label>
      {/if}
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.notifyBackground} />
        <span>
          Notify me when a long command finishes in a background tab
          <span class="block text-xs text-fg-muted">Needs shell integration (below) to know when commands end; the terminal bell always notifies.</span>
        </span>
      </label>
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.rememberCommands} />
        <span>
          Remember the commands I run on each host
          <span class="block text-xs text-fg-muted">
            Off by default. Kept as plain text on this computer only, never in the vault, and offered in the command palette. Commands can contain passwords and tokens; turning this off deletes the saved history.
          </span>
        </span>
      </label>
      <button class="btn-ghost mt-2 border border-line py-1 text-xs" onclick={() => { settings.clearHistory(); ui.notify("info", "Command history cleared."); }}>Clear command history</button>
      <label class="mt-3 flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.trimPasteNewline} />
        <span>
          Drop one trailing newline from a paste
          <span class="block text-xs text-fg-muted">Copying a line from a file or editor often adds a trailing newline; without this, pasting it submits an extra blank line.</span>
        </span>
      </label>
    </section>
    {/if}

    {#if visible("integrations", "command line cli scripting sshvault run list connect")}
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
      {@render msgBox(cliMsg)}
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
            <button class="btn-secondary py-1 text-xs" onclick={() => { void writeText(aliasLine); ui.notify("info", "Copied."); }}><Copy size={12} /> Copy</button>
          </div>
        {/if}
      {/if}
    </section>
    {/if}

    {#if visible("terminal", SMART_COMPLETION_KEYWORDS)}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Smart completion</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Suggests the rest of a command while you type at a shell prompt. Nothing is typed for you: a suggestion is only shown, and you accept it
        with a key. It stays out of full-screen programs like vim and tmux, and out of password prompts.
      </p>
      <label class="flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.smartCompletion} />
        <span>
          Smart completion
          <span class="block text-xs text-fg-muted">The master switch: off turns every part below off, in every tab, at once. Each host can override it in its own settings.</span>
        </span>
      </label>
      <div class="ml-6 mt-3 space-y-2 {settings.prefs.smartCompletion ? '' : 'opacity-50'}">
        <label class="flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion} bind:checked={settings.prefs.acInline} />
          <span>
            Suggest as I type
            <span class="block text-xs text-fg-muted">A faint suggestion after the cursor, from commands typed before. → or End accepts it.</span>
          </span>
        </label>
        <label class="flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion} bind:checked={settings.prefs.acMenu} />
          <span>
            Show a list of matches
            <span class="block text-xs text-fg-muted">Ctrl+Space opens it. Tab stays the shell's own; you can give it to the list under Keyboard shortcuts.</span>
          </span>
        </label>
        <label class="ml-6 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion || !settings.prefs.acMenu} bind:checked={settings.prefs.acSnippets} />
          <span>Include my snippets</span>
        </label>
        <label class="ml-6 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion || !settings.prefs.acMenu} bind:checked={settings.prefs.acOptions} />
          <span>
            Include command options and subcommands (git, docker, systemctl, …)
            <span class="block text-xs text-fg-muted" data-testid="spec-source">{specSource.commands} commands, from {specSource.package} {specSource.version} (MIT){specSource.handwritten?.length ? `, plus ${specSource.handwritten.length} written for this app` : ""}.</span>
          </span>
        </label>
        <label class="ml-6 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion || !settings.prefs.acMenu} bind:checked={settings.prefs.acRemotePaths} />
          <span>
            Look up file and folder names on the host
            <span class="block text-xs text-fg-muted">Opens an extra SSH channel to the host you are already connected to. Nothing else leaves this computer.</span>
          </span>
        </label>
        <label class="ml-12 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" disabled={!settings.prefs.smartCompletion || !settings.prefs.acMenu || !settings.prefs.acRemotePaths} bind:checked={settings.prefs.acSeedHistory} />
          <span>
            Learn from the host's own shell history
            <span class="block text-xs text-fg-muted">Once per host, reads the last few hundred commands from its <code>.bash_history</code>, <code>.zsh_history</code> or fish history, so a new computer is not empty. Commands that look like they hold a password or token are skipped. Off until you turn it on; never on production hosts.</span>
          </span>
        </label>
      </div>
      <p class="mt-3 text-xs text-fg-muted">
        Suggestions come from the commands you have typed in this session. They are kept for next time only if
        <strong>Remember the commands I run on each host</strong> (above) is on. Commands that look like they contain a password or token, and
        commands you start with a space, are never suggested. Production hosts use history only unless the host says otherwise.
      </p>
    </section>
    {/if}

    {#if visible("integrations", OPS_KEYWORDS)}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Alerts and monitoring history</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Alerts watch the hosts you turned monitoring on for (Fleet), and only while this app is open. They show a notice, an operating-system
        notification when the window is behind others, and a list under Operations. Nothing leaves this computer.
      </p>
      <label class="flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={settings.prefs.alerts} />
        <span>Tell me when a monitored host has a problem<span class="block text-xs text-fg-muted">Off until you switch it on.</span></span>
      </label>
      <div class="ml-6 mt-3 space-y-3 {settings.prefs.alerts ? '' : 'opacity-50'}">
        <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" disabled={!settings.prefs.alerts} bind:checked={settings.prefs.alertDown} /> A host stops answering (two checks in a row, a minute apart)</label>
        <div class="grid grid-cols-3 gap-3">
          {#each [["alertCpu", "CPU over"], ["alertMem", "Memory over"], ["alertDisk", "Disk over"]] as [key, label] (key)}
            <div>
              <label class="label" for="al-{key}">{label} (%)</label>
              <input id="al-{key}" class="input" type="number" min="0" max="100" disabled={!settings.prefs.alerts} bind:value={settings.prefs[key as "alertCpu" | "alertMem" | "alertDisk"]} />
            </div>
          {/each}
        </div>
        <p class="-mt-1 text-xs text-fg-muted">0 turns a limit off. A reading must stay over it for</p>
        <div class="flex items-center gap-2 text-sm">
          <input class="input w-20" type="number" min="1" max="20" disabled={!settings.prefs.alerts} bind:value={settings.prefs.alertSamples} aria-label="Readings in a row" />
          <span class="text-xs text-fg-muted">readings in a row (30 seconds apart) before it counts.</span>
        </div>
        <div class="flex flex-wrap items-center gap-2 text-sm">
          <label class="flex items-center gap-2"><input type="checkbox" class="accent-input" disabled={!settings.prefs.alerts} bind:checked={settings.prefs.alertQuiet} /> Quiet hours, from</label>
          <input class="input w-28" type="time" disabled={!settings.prefs.alerts || !settings.prefs.alertQuiet} bind:value={settings.prefs.alertQuietFrom} aria-label="Quiet from" />
          <span>to</span>
          <input class="input w-28" type="time" disabled={!settings.prefs.alerts || !settings.prefs.alertQuiet} bind:value={settings.prefs.alertQuietTo} aria-label="Quiet until" />
          <span class="text-xs text-fg-muted">Alerts still reach the list, but nothing pops up.</span>
        </div>
      </div>

      <div class="mt-5 border-t border-line pt-4">
        <label class="label" for="keep">Keep the monitoring charts for</label>
        <select id="keep" class="input w-64" bind:value={settings.prefs.metricsKeep}>
          <option value="off">15 minutes (in memory only)</option>
          <option value="day">A day, on this computer</option>
          <option value="week">A week, on this computer</option>
        </select>
        <p class="mt-1 text-xs text-fg-muted">
          Kept as one-minute averages of CPU, memory and network, in this app's storage on this computer; never synced and not encrypted.
          Choosing 15 minutes deletes what was kept.
        </p>
      </div>
    </section>
    {/if}

    {#if visible("terminal", "shell integration osc 133 7 prompt directory")}
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
    {/if}

    {#if visible("safety", "safety clipboard clear paste confirm destructive production backup retention pattern")}
    {#if draft}
      <form class="rounded-xl border border-line bg-panel p-5" onsubmit={saveShared}>
        <div class="mb-1 flex items-center gap-2">
          <h2 class="flex items-center gap-2 text-sm font-semibold"><ShieldAlert size={15} class="text-accent" /> Safety</h2>
          <Badge tone="accent">Synced</Badge>
        </div>
        <p class="mb-4 text-xs text-fg-muted">
          Stored in the vault and shared by every device that opens it. Unlike the rest of this page, changes here need
          <strong>Save</strong>, so a slip of the keyboard never lowers a safety check on every computer that opens this
          vault.
        </p>
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
        {@render msgBox(safetyMsg)}
      </form>
    {/if}
    {/if}

    {#if visible("integrations", "export ssh config openssh include scp ansible git")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 text-sm font-semibold">Use your hosts from the command line</h2>
      <p class="mb-3 text-xs text-fg-muted">
        Export every host as an OpenSSH config, then add <code>Include /path/to/file</code> to <code>~/.ssh/config</code>
        so <code>ssh</code>, <code>scp</code>, Ansible and git use the same names. Keys stay in the vault; the file only has
        names, addresses, users, ports and jump hosts.
      </p>
      <div class="flex gap-2">
        <button class="btn-secondary" onclick={exportToFile}>Save to file…</button>
        <button class="btn-secondary" onclick={exportToClipboard}>Copy to clipboard</button>
      </div>
      {@render msgBox(exportMsg)}
    </section>
    {/if}

    {#if visible("advanced", "reset default settings")}
    <section class="rounded-xl border border-line bg-panel p-5">
      <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><RotateCcw size={15} class="text-accent" /> Reset</h2>
      <p class="mb-4 text-xs text-fg-muted">
        Puts every per-computer setting back to its default: appearance, layout, shortcuts, imported themes, auto-lock,
        local shell and update checks. Vault settings and your hosts are not touched.
      </p>
      <button class="btn-ghost border border-danger/40 text-danger hover:bg-danger/10" onclick={resetAll}>Reset all settings…</button>
    </section>
    {/if}
    </div>
  </div>
</div>
