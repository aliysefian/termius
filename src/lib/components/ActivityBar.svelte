<script lang="ts">
  import {
    ArrowLeftRight,
    ArrowUpCircle,
    ChevronRight,
    ChevronsLeft,
    ChevronsRight,
    Code,
    Container,
    Database,
    FolderSync,
    FolderTree,
    KeyRound,
    LayoutGrid,
    Network,
    Wrench,
    Lock,
    Server,
    Settings,
    ShieldCheck,
    Star,
    UserRound,
    Vault,
  } from "lucide-svelte";
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
    { view: "topology", label: "Topology", icon: Network, hint: "How hosts, proxies, tunnels and databases connect" },
  ];

  /** A rail entry's name in the chosen language. */
  const name = (item: Item) => t(`rail.${item.view}` as Key) ?? item.label;
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
    if (open) {
      queueMicrotask(() => {
        fitFlyout();
        panel?.querySelector<HTMLElement>("button")?.focus();
      });
    }
  }

  /** On a short window the menu is moved up until it fits; what is still too tall scrolls (see max-h below). */
  function fitFlyout() {
    if (!panel) return;
    const room = window.innerHeight - 8;
    flyout = { top: Math.max(8, Math.min(flyout.top, room - panel.offsetHeight)) };
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

  // Wide: a name beside each icon. Narrow: the icon strip, with the name under the icon when "Show a name" is on, and as a tooltip.
  const wide = $derived(settings.prefs.railExpanded);
  const row = "group relative flex h-9 w-full shrink-0 items-center gap-3 rounded-lg px-3 text-[13px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";
  const rail = "group relative flex shrink-0 flex-col items-center justify-center gap-0.5 rounded-xl transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";
  // The current page is a tile with a violet outline, like a selected tab.
  const tone = (on: boolean) => (on ? "bg-accent/15 text-accent ring-1 ring-inset ring-accent/60" : "text-fg-muted hover:bg-panel-hover hover:text-fg");
</script>

<svelte:window onpointerdown={outside} />

<nav class="flex {wide ? 'w-[196px] items-stretch px-2' : settings.prefs.railLabels ? 'w-[68px] items-center' : 'w-14 items-center'} flex-col overflow-y-auto bg-base py-2" aria-label={t("rail.sections")}>
  {#each groups as group, g (g)}
    {#if g > 0}<div class="my-1.5 h-px {wide ? 'w-full' : 'w-8'} shrink-0 bg-line" role="separator"></div>{/if}
    {#each group as item (item.view)}
      <button
        class="{wide ? row : `${rail} ${settings.prefs.railLabels ? 'h-[46px] w-[56px]' : 'h-10 w-10'}`} mb-0.5 {tone(isCurrent(item))}"
        onclick={() => go(item.view)}
        title={name(item)}
        aria-label={name(item)}
        aria-current={isCurrent(item) ? "page" : undefined}
      >
        <item.icon size={wide ? 18 : settings.prefs.railLabels ? 19 : 21} class="shrink-0" />
        {#if wide}<span class="truncate">{name(item)}</span>{:else if settings.prefs.railLabels}<span class="text-[10px] font-medium leading-none">{name(item)}</span>{/if}
        {#if item.view === "ops" && alerts.unread > 0}
          <span class="absolute {wide ? 'right-3 top-3.5' : 'right-2 top-1.5'} h-2 w-2 rounded-full bg-danger" title="{alerts.unread} new alert{alerts.unread === 1 ? '' : 's'}"></span>
        {/if}
      </button>
    {/each}
  {/each}

  <div class="my-1.5 h-px {wide ? 'w-full' : 'w-6'} shrink-0 bg-line" role="separator"></div>
  <button
    bind:this={manageButton}
    class="{wide ? row : `${rail} w-10 ${settings.prefs.railLabels ? 'h-[46px] w-[56px]' : 'h-10'}`} mb-0.5 {tone(inManage || open)}"
    onclick={toggleManage}
    title="Groups, keys, credentials, known hosts and the vault"
    aria-label={t("rail.manage")}
    aria-haspopup="menu"
    aria-expanded={open}
  >
    <LayoutGrid size={wide ? 18 : settings.prefs.railLabels ? 19 : 21} class="shrink-0" />
    {#if wide}<span class="truncate">{t("rail.manage")}</span>{:else if settings.prefs.railLabels}<span class="text-[10px] font-medium leading-none">{t("rail.manage")}</span>{/if}
    {#if vaultStore.openConflicts > 0}
      <span class="absolute {wide ? 'right-3 top-3.5' : 'right-2 top-1.5'} h-2 w-2 rounded-full bg-warning" title="Sync conflicts need attention"></span>
    {/if}
  </button>

  <div class="flex-1"></div>

  {#if updates.available}
    <button
      class="{wide ? row : `${rail} h-10 w-10`} mb-1 text-accent hover:bg-accent/15"
      onclick={() => (updates.dismissed = null)}
      title={updates.installing ? "Installing the update…" : `Update to SSHVault ${updates.available.version}`}
      aria-label="Update available"
    >
      <ArrowUpCircle size={wide ? 18 : 20} class="shrink-0 {updates.installing ? 'animate-pulse' : ''}" />
      {#if wide}<span class="truncate">Update available</span>{/if}
      <span class="absolute {wide ? 'right-3 top-3.5' : 'right-1.5 top-1.5'} h-2 w-2 rounded-full bg-accent"></span>
    </button>
  {/if}

  <button class="{wide ? row : `${rail} h-10 w-10`} mb-1 {tone(false)}" onclick={() => vaultStore.lock()} title="{t("rail.lock")} (Ctrl+Shift+L)" aria-label={t("rail.lock")}>
    <Lock size={wide ? 18 : 20} class="shrink-0" />{#if wide}<span class="truncate">{t("rail.lock")}</span>{/if}
  </button>
  <button class="{wide ? row : `${rail} h-10 w-10`} mb-1 {tone(ui.view === 'settings')}" onclick={() => go("settings")} title={t("rail.settings")} aria-label={t("rail.settings")} aria-current={ui.view === "settings" ? "page" : undefined}>
    <Settings size={wide ? 18 : 20} class="shrink-0" />{#if wide}<span class="truncate">{t("rail.settings")}</span>{/if}
  </button>
  <button
    class="{wide ? row : `${rail} h-10 w-10`} text-fg-muted hover:bg-panel-hover hover:text-fg"
    onclick={() => (settings.prefs.railExpanded = !wide)}
    title={wide ? t("rail.collapse") : t("rail.expand")}
    aria-label={wide ? t("rail.collapse") : t("rail.expand")}
    aria-expanded={wide}
  >
    {#if wide}<ChevronsLeft size={18} class="shrink-0" /><span class="truncate">{t("rail.collapse")}</span>{:else}<ChevronsRight size={20} />{/if}
  </button>

{#if open}
    <!-- Beside the rail, level with the button; fixed so the rail's own scrolling can't clip it. -->
    <div
      bind:this={panel}
      class="anim-pop-left fixed {wide ? 'left-[204px]' : settings.prefs.railLabels ? 'left-[72px]' : 'left-[60px]'} z-40 max-h-[calc(100vh-1rem)] w-72 overflow-y-auto overscroll-contain rounded-xl border border-line bg-panel p-1.5 shadow-xl"
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
