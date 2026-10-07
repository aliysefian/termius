<script lang="ts">
  import {
    ArrowLeftRight,
    ArrowUpCircle,
    ChevronRight,
    Code,
    Container,
    Database,
    FolderSync,
    FolderTree,
    KeyRound,
    LayoutGrid,
    Wrench,
    Lock,
    PanelLeftClose,
    PanelLeftOpen,
    Server,
    Settings,
    ShieldCheck,
    Star,
    UserRound,
    Vault,
  } from "lucide-svelte";
  import Logo from "./Logo.svelte";
  import { RAIL_GROUPS, arrange } from "$lib/railitems";
  import { t, type Key } from "$lib/i18n/index.svelte";
  import { PAGE_VIEWS, ui, type View } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { updates } from "$lib/stores/updates.svelte";
  import { alerts } from "$lib/stores/alerts.svelte";

  type Item = { view: View; label: string; icon: typeof Server; hint?: string; /** Other pages that belong to the same entry. */ also?: View[] };

  // What people open every day stays on the rail, in groups: where to connect, what to do once connected,
  // and the data stores. Everything that is set up once and then left alone sits behind "Manage", and so does
  // anything the person put away in Settings.
  const arranged = $derived(arrange(RAIL_GROUPS, settings.prefs.railOrder, settings.prefs.railHidden));
  const groups = $derived(arranged.groups);

  const manage: Item[] = [
    { view: "groups", label: "Groups and proxies", icon: FolderTree, hint: "Shared logins, bastions and proxies" },
    { view: "keys", label: "Keys", icon: KeyRound, hint: "SSH keys and certificates" },
    { view: "keychain", label: "Credentials", icon: UserRound, hint: "Usernames and passwords" },
    { view: "knownhosts", label: "Known hosts", icon: ShieldCheck, hint: "Server fingerprints you trust" },
    { view: "vault", label: "Vault", icon: Vault, hint: "Sync, backups, conflicts and the connection log" },
  ];

  /** A rail entry's name in the chosen language. */
  const name = (item: Item) => t(`rail.${item.view}` as Key);
  const isCurrent = (item: Item) => ui.view === item.view || !!item.also?.includes(ui.view);
  const inManage = $derived(manage.some((m) => m.view === ui.view) || arranged.away.some((a) => isCurrent(a)));
  let open = $state(false);
  let flyout = $state<{ top: number }>({ top: 0 });
  let manageButton = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();

  const hasPanel = (view: View) => !PAGE_VIEWS.includes(view) && view !== "sftp";

  function go(view: View) {
    open = false;
    // Clicking the current view's icon toggles the panel, like VS Code.
    if (ui.view === view && hasPanel(view)) ui.toggleSidebar();
    else {
      ui.view = view;
      if (hasPanel(view)) settings.prefs.sidebarHidden = false;
    }
  }

  function toggleManage() {
    if (!open && manageButton) flyout = { top: Math.max(8, manageButton.getBoundingClientRect().top - 4) };
    open = !open;
    if (open) queueMicrotask(() => panel?.querySelector<HTMLElement>("button")?.focus());
  }

  function onPanelKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      open = false;
      manageButton?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const rows = [...(panel?.querySelectorAll<HTMLElement>("button") ?? [])];
    const at = rows.indexOf(document.activeElement as HTMLElement);
    rows[(at + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length]?.focus();
  }

  function outside(e: PointerEvent) {
    if (!open) return;
    const t = e.target as Node;
    if (!panel?.contains(t) && !manageButton?.contains(t)) open = false;
  }

  const rail = "group relative flex shrink-0 flex-col items-center justify-center gap-0.5 rounded-lg transition-colors";
  const tone = (on: boolean) => (on ? "bg-accent/15 text-accent" : "text-fg-muted hover:bg-panel-hover hover:text-fg");
</script>

<svelte:window onpointerdown={outside} />

<nav class="flex {settings.prefs.railLabels ? 'w-[68px]' : 'w-14'} flex-col items-center overflow-y-auto border-r border-line bg-panel py-3" aria-label={t("rail.sections")}>
  <button class="mb-3 shrink-0 rounded-xl transition-transform hover:scale-105" onclick={() => go("hosts")} title="SSHVault" aria-label="SSHVault: go to hosts">
    <Logo size={34} />
  </button>

  {#each groups as group, g (g)}
    {#if g > 0}<div class="my-1.5 h-px w-8 shrink-0 bg-line" role="separator"></div>{/if}
    {#each group as item (item.view)}
      <button
        class="{rail} mb-0.5 w-[56px] {settings.prefs.railLabels ? 'h-[46px]' : 'h-10'} {tone(isCurrent(item))}"
        onclick={() => go(item.view)}
        title={name(item)}
        aria-label={name(item)}
        aria-current={isCurrent(item) ? "page" : undefined}
      >
        {#if isCurrent(item)}
          <span class="absolute -left-[6px] h-6 w-[3px] rounded-r bg-accent"></span>
        {/if}
        <item.icon size={19} />
        {#if settings.prefs.railLabels}<span class="text-[10px] font-medium leading-none">{name(item)}</span>{/if}
        {#if item.view === "ops" && alerts.unread > 0}
          <span class="absolute right-2 top-1.5 h-2 w-2 rounded-full bg-danger" title="{alerts.unread} new alert{alerts.unread === 1 ? '' : 's'}"></span>
        {/if}
      </button>
    {/each}
  {/each}

  <div class="my-1.5 h-px w-8 shrink-0 bg-line" role="separator"></div>
  <button
    bind:this={manageButton}
    class="{rail} h-[46px] w-[56px] {tone(inManage || open)}"
    onclick={toggleManage}
    title="Groups, keys, credentials, known hosts and the vault"
    aria-label={t("rail.manage")}
    aria-haspopup="menu"
    aria-expanded={open}
  >
    {#if inManage}<span class="absolute -left-[6px] h-6 w-[3px] rounded-r bg-accent"></span>{/if}
    <LayoutGrid size={19} />
    <span class="text-[10px] font-medium leading-none">{t("rail.manage")}</span>
    {#if vaultStore.openConflicts > 0}
      <span class="absolute right-2 top-1.5 h-2 w-2 rounded-full bg-warning" title="Sync conflicts need attention"></span>
    {/if}
  </button>

  <div class="flex-1"></div>

  {#if updates.available}
    <button
      class="{rail} mb-1 h-10 w-10 text-accent hover:bg-accent/15"
      onclick={() => (updates.dismissed = null)}
      title={updates.installing ? "Installing the update…" : `Update to SSHVault ${updates.available.version}`}
      aria-label="Update available"
    >
      <ArrowUpCircle size={20} class={updates.installing ? "animate-pulse" : ""} />
      <span class="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-accent"></span>
    </button>
  {/if}

  <button
    class="{rail} mb-1 h-10 w-10 {tone(false)}"
    onclick={() => ui.toggleSidebar()}
    title="{settings.prefs.sidebarHidden ? t("rail.showPanel") : t("rail.hidePanel")} (Ctrl+Shift+H)"
    aria-label={settings.prefs.sidebarHidden ? t("rail.showPanel") : t("rail.hidePanel")}
    aria-pressed={settings.prefs.sidebarHidden}
  >
    {#if settings.prefs.sidebarHidden}<PanelLeftOpen size={20} />{:else}<PanelLeftClose size={20} />{/if}
  </button>
  <button class="{rail} mb-1 h-10 w-10 {tone(false)}" onclick={() => vaultStore.lock()} title="{t("rail.lock")} (Ctrl+Shift+L)" aria-label={t("rail.lock")}>
    <Lock size={20} />
  </button>
  <button class="{rail} h-10 w-10 {tone(ui.view === 'settings')}" onclick={() => go("settings")} title={t("rail.settings")} aria-label={t("rail.settings")} aria-current={ui.view === "settings" ? "page" : undefined}>
    <Settings size={20} />
  </button>

{#if open}
    <!-- Beside the rail, level with the button; fixed so the rail's own scrolling can't clip it. -->
    <div
      bind:this={panel}
      class="anim-pop-left fixed {settings.prefs.railLabels ? 'left-[72px]' : 'left-[60px]'} z-40 w-72 rounded-xl border border-line bg-panel p-1.5 shadow-xl"
      style:top="{flyout.top}px"
      role="menu"
      aria-label={t("rail.manage")}
      tabindex="-1"
      onkeydown={onPanelKey}
    >
      {#each manage as item (item.view)}
        <button
          class="flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left transition-colors hover:bg-panel-hover focus:bg-panel-hover focus:outline-none {ui.view === item.view ? 'bg-accent/10' : ''}"
          role="menuitem"
          onclick={() => go(item.view)}
        >
          <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-base {ui.view === item.view ? 'text-accent' : 'text-fg-muted'}">
            <item.icon size={17} />
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-medium">{t(`manage.${item.view}` as Key)}</span>
            <span class="block truncate text-xs text-fg-muted">{t(`manage.${item.view}.hint` as Key)}</span>
          </span>
          {#if item.view === "vault" && vaultStore.openConflicts > 0}
            <span class="h-2 w-2 shrink-0 rounded-full bg-warning" title="Sync conflicts need attention"></span>
          {:else}
            <ChevronRight size={14} class="shrink-0 text-fg-muted" />
          {/if}
        </button>
      {/each}
      {#if arranged.away.length}
        <div class="mx-2 my-1.5 border-t border-line"></div>
        <div class="px-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wide text-fg-muted">{t("manage.away")}</div>
        {#each arranged.away as item (item.view)}
          <button
            class="flex w-full items-center gap-3 rounded-lg px-2.5 py-1.5 text-left transition-colors hover:bg-panel-hover focus:bg-panel-hover focus:outline-none {isCurrent(item) ? 'bg-accent/10' : ''}"
            role="menuitem"
            onclick={() => go(item.view)}
          >
            <item.icon size={16} class="shrink-0 {isCurrent(item) ? 'text-accent' : 'text-fg-muted'}" />
            <span class="text-sm">{name(item)}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</nav>
