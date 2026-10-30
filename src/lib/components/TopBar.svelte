<script lang="ts">
  import { Bell, Columns2, Maximize2, Minimize2, PanelBottom, PanelLeft, PanelRight, Plus, Rows2, Search, Settings, SquareTerminal } from "lucide-svelte";
  import Logo from "./Logo.svelte";
  import Kbd from "./Kbd.svelte";
  import { bottomFits, rightFits } from "$lib/docklayout";
  import { ACTIONS, comboFor } from "$lib/shortcuts";
  import { alerts } from "$lib/stores/alerts.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let { windowWidth, windowHeight, leftUsed }: { windowWidth: number; windowHeight: number; leftUsed: number } = $props();

  const combo = (id: string) => {
    const a = ACTIONS.find((x) => x.id === id);
    return a ? comboFor(a) : "";
  };
  /** "Hide side panel (Ctrl+Alt+B)": what a click does, and the key for it. */
  const tip = (text: string, id: string) => (combo(id) ? `${text} (${combo(id)})` : text);

  const p = $derived(settings.prefs);
  const hasTab = $derived(ui.tabs.length > 0);
  const btn = "icon-btn h-8 w-8";
  const on = "bg-accent/15 text-accent";
</script>

<header class="flex h-11 shrink-0 items-center gap-3 border-b border-line bg-panel px-3" aria-label="Toolbar">
  <div class="flex w-[168px] shrink-0 items-center gap-2">
    <Logo size={24} />
    <span class="text-[15px] font-semibold tracking-tight">SSHVault</span>
  </div>

  <!-- The quick switcher: one place for hosts, commands and files. -->
  <button
    class="mx-auto flex h-8 w-full max-w-[520px] min-w-0 items-center gap-2 rounded-md border border-line bg-base px-3 text-left text-sm text-fg-muted hover:border-accent/50"
    title={tip("Search hosts, commands and files", "palette-alt")}
    aria-label="Search hosts, commands and files"
    onclick={() => (ui.paletteOpen = true)}
  >
    <Search size={14} class="shrink-0" />
    <span class="min-w-0 flex-1 truncate">Search hosts, commands, files…</span>
    <span class="hidden shrink-0 md:block"><Kbd keys="Ctrl+K" /></span>
  </button>

  <div class="flex shrink-0 items-center gap-0.5">
    <button class={btn} title={tip("New connection", "quick-connect")} aria-label="New connection" onclick={() => (ui.modal = { kind: "quick-connect" })}><Plus size={16} /></button>
    <button class={btn} title={tip("Local terminal", "local-terminal")} aria-label="Local terminal" onclick={() => ui.openLocal()}><SquareTerminal size={16} /></button>
    <button class="{btn} hidden md:inline-flex" title={tip("Split right", "split-right")} aria-label="Split right" disabled={!hasTab} onclick={() => ui.splitActive("vertical")}><Columns2 size={16} /></button>
    <button class="{btn} hidden md:inline-flex" title={tip("Split down", "split-down")} aria-label="Split down" disabled={!hasTab} onclick={() => ui.splitActive("horizontal")}><Rows2 size={16} /></button>

    <span class="mx-1.5 h-5 w-px bg-line" aria-hidden="true"></span>

    <!-- What is showing around the terminal. Each is its own switch; Shell focus hides all and restores them. -->
    <div class="flex items-center gap-0.5" role="group" aria-label="Layout">
      <button class="{btn} {!p.sidebarHidden ? on : ''}" title={tip(p.sidebarHidden ? "Show hosts panel" : "Hide hosts panel", "sidebar")} aria-label="Toggle the hosts panel" aria-pressed={!p.sidebarHidden} onclick={() => ui.toggleSidebar()}><PanelLeft size={16} /></button>
      <button class="{btn} {p.bottomPanelOpen ? on : ''}" title={bottomFits(windowHeight) ? tip(p.bottomPanelOpen ? "Hide bottom panel" : "Show bottom panel", "bottom-panel") : "The window is too short for the bottom panel"} aria-label="Toggle the bottom panel" aria-pressed={p.bottomPanelOpen} disabled={!bottomFits(windowHeight)} onclick={() => ui.toggleBottomPanel()}><PanelBottom size={16} /></button>
      <button class="{btn} {p.rightPanelOpen ? on : ''}" title={rightFits(windowWidth, leftUsed) ? tip(p.rightPanelOpen ? "Hide right panel" : "Show right panel", "right-panel") : "The window is too narrow for the right panel"} aria-label="Toggle the right panel" aria-pressed={p.rightPanelOpen} disabled={!rightFits(windowWidth, leftUsed)} onclick={() => ui.toggleRightPanel()}><PanelRight size={16} /></button>
      <button class="{btn} {p.shellFocus ? on : ''}" title={tip(p.shellFocus ? "Leave Shell focus" : "Shell focus: hide every panel", "shell-focus")} aria-label="Shell focus" aria-pressed={p.shellFocus} onclick={() => ui.toggleShellFocus()}>
        {#if p.shellFocus}<Minimize2 size={16} />{:else}<Maximize2 size={16} />{/if}
      </button>
    </div>

    <span class="mx-1.5 h-5 w-px bg-line" aria-hidden="true"></span>

    <button class="{btn} relative" title="Alerts" aria-label={alerts.unread ? `Alerts: ${alerts.unread} new` : "Alerts"} onclick={() => (ui.view = "ops")}>
      <Bell size={16} />
      {#if alerts.unread > 0}<span class="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-danger"></span>{/if}
    </button>
    <button class="{btn} {ui.view === 'settings' ? on : ''}" title="Settings" aria-label="Settings" onclick={() => (ui.view = "settings")}><Settings size={16} /></button>
  </div>
</header>
