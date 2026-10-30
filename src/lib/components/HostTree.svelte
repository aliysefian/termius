<script lang="ts">
  import Illustration from "./Illustration.svelte";
  import { Activity, ArrowDownUp, Check, ChevronDown, ChevronRight, Clock, Cloud, Ellipsis, FileInput, FoldVertical, Plus, Search, Server, Star, UnfoldVertical } from "lucide-svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import HostRow from "./HostRow.svelte";
  import Spinner from "./Spinner.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { buildTree, flattenTree, groupPaths, type HostSort, type TreeRow } from "$lib/tree";
  import { ENVIRONMENTS } from "$lib/types";
  import { acceptsHost, dropHostInto } from "$lib/hostdrag.svelte";
  import HostTreeNode from "./HostTreeNode.svelte";
  import GroupRow from "./GroupRow.svelte";
  import { offsets, windowOf } from "$lib/virtuallist";
  import { tick } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { GroupNode } from "$lib/tree";

  let { favoritesOnly = false }: { favoritesOnly?: boolean } = $props();
  let envFilter = $state("");
  let tagFilter = $state("");
  let sortBy = $state<HostSort>("name");
  const allTags = $derived([...new Set(vaultStore.hosts.flatMap((h) => h.data?.tags ?? []))].sort());

  const filtered = $derived.by(() => {
    const q = ui.search.trim().toLowerCase();
    return vaultStore.hosts.filter((h) => {
      const d = h.data;
      if (!d) return false;
      if (favoritesOnly && !d.favorite) return false;
      if (envFilter && (d.environment ?? "") !== envFilter) return false;
      if (tagFilter && !d.tags.includes(tagFilter)) return false;
      if (!q) return true;
      const fields = [d.label, d.hostname, d.group, d.notes, ...d.tags, ...Object.entries(d.custom ?? {}).flat()];
      return fields.some((s) => s?.toLowerCase().includes(q));
    });
  });
  const envChoices = $derived([
    ...new Set([...ENVIRONMENTS.map((e) => e.value), ...vaultStore.hosts.map((h) => h.data?.environment ?? "")]),
  ].filter(Boolean));
  const byLastUsed = (a: (typeof filtered)[number], b: (typeof filtered)[number]) =>
    (settings.usage[b.id]?.last ?? 0) - (settings.usage[a.id]?.last ?? 0) || (a.data?.label ?? "").localeCompare(b.data?.label ?? "");
  const tree = $derived(buildTree(filtered, sortBy === "lastused" ? byLastUsed : sortBy));
  const paths = $derived(groupPaths(tree));
  const anyExpanded = $derived(paths.some((p) => !ui.collapsedGroups.has(p)));
  let rootDrop = $state(false);
  const recent = $derived(
    settings.recent
      .map((id) => vaultStore.hostById.get(id))
      .filter((h): h is NonNullable<typeof h> => !!h?.data)
      .slice(0, 5),
  );

  // -- keyboard navigation -------------------------------------------------
  // One flat, visibility-aware order (Recent first, then the real tree) so
  // arrow keys, Home/End and type-ahead move through exactly what's drawn.
  const recentRows = $derived<TreeRow[]>(recent.map((h) => ({ kind: "host", key: `r:${h.id}`, id: h.id, depth: 0, parentKey: null })));
  // Favorites sit at the top of the list (collapsible), like the Favorites view in brief.
  const FAV_KEY = "★favorites";
  const favorites = $derived(favoritesOnly || ui.search.trim() ? [] : filtered.filter((h) => h.data?.favorite).sort((a, b) => a.data!.label.localeCompare(b.data!.label)));
  const favoritesFolded = $derived(ui.collapsedGroups.has(FAV_KEY));
  const favRows = $derived<TreeRow[]>(favoritesFolded ? [] : favorites.map((h) => ({ kind: "host", key: `f:${h.id}`, id: h.id, depth: 0, parentKey: null })));
  // A favorite is already at the top, so Recent lists only the rest.
  const favIds = $derived(new Set(favorites.map((h) => h.id)));
  const recentShown = $derived(recent.filter((h) => !favIds.has(h.id)));
  // The header's overflow menu: the less-used actions, so the row stays two buttons wide.
  let menuOpen = $state(false);
  let menuBtn = $state<HTMLButtonElement>();
  let menuEl = $state<HTMLDivElement>();
  let menuTop = $state(0);
  function toggleMenu() {
    if (!menuOpen && menuBtn) menuTop = menuBtn.getBoundingClientRect().bottom + 4;
    menuOpen = !menuOpen;
    if (menuOpen) queueMicrotask(() => menuEl?.querySelector<HTMLElement>("button")?.focus());
  }
  function outsideMenu(e: PointerEvent) {
    if (menuOpen && !menuEl?.contains(e.target as Node) && !menuBtn?.contains(e.target as Node)) menuOpen = false;
  }
  const SORTS: [HostSort, string][] = [["name", "Name"], ["hostname", "Address"], ["updated", "Recent edits"], ["lastused", "Last used"]];
  // While searching, every group opens: a match inside a folded group would otherwise be hidden.
  const folded = $derived(ui.search.trim() ? new Set<string>() : ui.collapsedGroups);
  const treeRows = $derived(flattenTree(tree, folded));
  const navRows = $derived([...favRows, ...(!ui.search.trim() && !favoritesOnly ? recentRows.filter((r) => r.kind === "host" && !favIds.has(r.id)) : []), ...treeRows]);

  // -- a very long list is drawn a window at a time ----------------------------------------------------
  // Thousands of hosts as thousands of rows is slow to build and to scroll (measured: 5,000 hosts took 5,800
  // elements and a second to search), so past VIRTUAL_AFTER rows only the ones in view are drawn. Below it the
  // nested list is kept as it was.
  const VIRTUAL_AFTER = 400;
  const virtual = $derived(treeRows.length > VIRTUAL_AFTER);
  let hostH = $state(46);
  let groupH = $state(28);
  let scrollTop = $state(0);
  let viewport = $state(600);
  let listEl = $state<HTMLDivElement>();
  const starts = $derived(offsets(treeRows.map((r) => (r.kind === "group" ? groupH : hostH))));
  const groupMap = $derived.by(() => {
    const m = new Map<string, GroupNode>();
    const walk = (n: GroupNode) => n.children.forEach((c) => (m.set(c.path, c), walk(c)));
    walk(tree);
    return m;
  });
  // The list starts below the Recent block; the window is measured from there.
  const listTop = $derived((void treeRows, listEl?.offsetTop ?? 0));
  const win = $derived(windowOf(starts, Math.max(0, scrollTop - listTop), viewport));
  const shownRows = $derived(treeRows.slice(win.start, win.end));
  let frame = 0;
  function onScroll() {
    if (!virtual || frame) return;
    // Once a frame at most, however fast the wheel turns.
    frame = requestAnimationFrame(() => {
      frame = 0;
      scrollTop = treeEl?.scrollTop ?? 0;
    });
  }
  // The rows' real heights (they change with the density setting), taken from the first of each kind drawn.
  $effect(() => {
    void shownRows;
    void settings.prefs.density;
    if (!virtual || !listEl) return;
    const h = listEl.querySelector<HTMLElement>(".host-row")?.offsetHeight;
    const g = listEl.querySelector<HTMLElement>("[role=treeitem][aria-expanded]")?.offsetHeight;
    if (h && Math.abs(h - hostH) > 0.5) hostH = h;
    if (g && Math.abs(g - groupH) > 0.5) groupH = g;
  });
  let focusedKey = $state<string | null>(null);
  const effectiveFocusedKey = $derived(focusedKey ?? navRows[0]?.key ?? null);
  let treeEl = $state<HTMLDivElement>();

  function rowName(row: TreeRow): string {
    return row.kind === "group" ? row.name : (vaultStore.hostById.get(row.id)?.data?.label ?? "");
  }

  function focusRow(key: string | undefined) {
    if (!key || !treeEl) return;
    const find = () => treeEl?.querySelector<HTMLElement>(`[data-tree-key="${CSS.escape(key)}"]`);
    const there = find();
    if (there) {
      there.focus();
      return;
    }
    // A row that isn't drawn (it is outside the window): scroll to it first, then focus it.
    const at = treeRows.findIndex((r) => r.key === key);
    if (!virtual || at < 0 || !treeEl) return;
    treeEl.scrollTop = Math.max(0, listTop + starts[at] - viewport / 2);
    scrollTop = treeEl.scrollTop;
    void tick().then(() => find()?.focus());
  }

  let typeAhead = "";
  let typeAheadTimer: ReturnType<typeof setTimeout> | undefined;

  function onTreeKeydown(e: KeyboardEvent) {
    const idx = navRows.findIndex((r) => r.key === effectiveFocusedKey);
    const row = navRows[idx];
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusRow(navRows[Math.min(idx + 1, navRows.length - 1)]?.key);
        return;
      case "ArrowUp":
        e.preventDefault();
        focusRow(navRows[Math.max(idx - 1, 0)]?.key);
        return;
      case "Home":
        e.preventDefault();
        focusRow(navRows[0]?.key);
        return;
      case "End":
        e.preventDefault();
        focusRow(navRows.at(-1)?.key);
        return;
      case "*":
        e.preventDefault();
        ui.expandAllGroups();
        return;
      case "ArrowRight":
        if (!row || row.kind !== "group") return;
        e.preventDefault();
        if (ui.collapsedGroups.has(row.path)) ui.toggleGroup(row.path);
        else focusRow(navRows[idx + 1]?.key);
        return;
      case "ArrowLeft":
        if (!row) return;
        e.preventDefault();
        if (row.kind === "group" && !ui.collapsedGroups.has(row.path)) ui.toggleGroup(row.path);
        else if (row.parentKey) focusRow(row.parentKey);
        return;
      case "Enter":
        if (!row) return;
        e.preventDefault();
        if (row.kind === "host") ui.openTerminal(row.id, vaultStore.hostById.get(row.id)?.data?.label ?? "");
        else ui.toggleGroup(row.path);
        return;
      case " ":
        if (!row) return;
        e.preventDefault();
        if (row.kind === "host") ui.modal = { kind: "host-details", id: row.id };
        else ui.toggleGroup(row.path);
        return;
      case "e":
      case "E":
        // Ctrl/Cmd+E: this host's files. Only with a host row focused, so it never shadows an editor's own binding.
        if (row?.kind === "host" && (e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
          e.preventDefault();
          ui.openSftpAt(row.id, "");
        }
        break;
      case "F2":
        if (row?.kind === "host") {
          e.preventDefault();
          ui.modal = { kind: "host", id: row.id };
        }
        return;
      case "Delete":
        if (row?.kind === "host") {
          e.preventDefault();
          void vaultStore.requestDeleteHost(row.id);
        }
        return;
    }
    // Type-ahead: letters/digits jump to the next row starting with what
    // was typed, buffering consecutive keystrokes like a native listbox.
    if (e.key.length === 1 && /[a-z0-9]/i.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      clearTimeout(typeAheadTimer);
      typeAhead += e.key.toLowerCase();
      typeAheadTimer = setTimeout(() => (typeAhead = ""), 600);
      const query = typeAhead;
      const n = navRows.length;
      for (let step = 1; step <= n; step++) {
        const row = navRows[(idx + step) % n];
        if (rowName(row).toLowerCase().startsWith(query)) {
          focusRow(row.key);
          return;
        }
      }
    }
  }
</script>

<svelte:window onpointerdown={outsideMenu} />

<aside class="flex min-w-0 flex-1 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between gap-1 px-4 pt-3 pb-2 @max-[16rem]:px-3">
    <h2 class="min-w-0 truncate text-sm font-semibold">{favoritesOnly ? t("hosts.favorites") : t("hosts.title")}</h2>
    <div class="flex shrink-0 items-center gap-0.5 [&>.icon-btn]:@max-[16rem]:h-7 [&>.icon-btn]:@max-[16rem]:w-7">
      <button class="icon-btn" title="Check which hosts are reachable" disabled={vaultStore.checking} onclick={() => vaultStore.checkHealth()}>
        <Activity size={16} class={vaultStore.checking ? "animate-pulse text-accent" : ""} />
      </button>
      <button bind:this={menuBtn} class="icon-btn" title="Sort, import and export" aria-label="More host list actions" aria-haspopup="menu" aria-expanded={menuOpen} onclick={toggleMenu}>
        <Ellipsis size={16} />
      </button>
      <button class="ml-0.5 flex h-7 w-7 items-center justify-center rounded-md bg-accent text-white hover:bg-accent-hover" title="New host" aria-label="New host" onclick={() => (ui.modal = { kind: "host", id: null })}>
        <Plus size={15} />
      </button>
    </div>
  </div>
  {#if menuOpen}
    <div bind:this={menuEl} class="anim-pop fixed left-20 z-40 w-56 rounded-xl border border-line bg-panel p-1 text-sm shadow-xl" style:top="{menuTop}px" role="menu" aria-label="Host list actions" tabindex="-1" onkeydown={(e) => e.key === "Escape" && (menuOpen = false, menuBtn?.focus())}>
      <p class="px-2 pb-1 pt-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Sort by</p>
      {#each SORTS as [v, label] (v)}
        <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitemradio" aria-checked={sortBy === v} onclick={() => { sortBy = v; menuOpen = false; }}>
          <span class="w-3.5">{#if sortBy === v}<Check size={13} class="text-accent" />{/if}</span>{label}
        </button>
      {/each}
      <div class="my-1 h-px bg-line"></div>
      {#if paths.length}
        {#if anyExpanded}
          <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.collapseGroups(paths); menuOpen = false; }}><FoldVertical size={14} /> Collapse all groups</button>
        {:else}
          <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { ui.expandAllGroups(); menuOpen = false; }}><UnfoldVertical size={14} /> Expand all groups</button>
        {/if}
        <div class="my-1 h-px bg-line"></div>
      {/if}
      <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { menuOpen = false; ui.modal = { kind: "import-ssh-config" }; }}><FileInput size={14} /> Import ~/.ssh/config or Ansible…</button>
      <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { menuOpen = false; ui.modal = { kind: "import-inventory" }; }}><Cloud size={14} /> Import from a cloud or Kubernetes…</button>
      <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" role="menuitem" onclick={() => { menuOpen = false; ui.modal = { kind: "hosts-transfer" }; }}><ArrowDownUp size={14} /> Export or import as a file…</button>
    </div>
  {/if}

  <div class="px-3 pb-2">
    <div class="relative">
      <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input py-1.5 pl-8" placeholder={t("hosts.search")} bind:value={ui.search} />
    </div>
    <div class="mt-1.5 flex flex-wrap gap-1.5">
      {#if envChoices.length}
        <!-- One tap per environment instead of a drop-down: what is in production is the thing to see at a glance. -->
        <div class="flex w-full gap-1 rounded-lg border border-line bg-base p-0.5 text-[11px]" role="group" aria-label="Filter by environment">
          {#each ["", ...envChoices] as e (e)}
            <button
              type="button"
              class="min-w-0 flex-1 truncate rounded-md px-1.5 py-1 font-medium capitalize transition-colors {envFilter === e ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
              aria-pressed={envFilter === e}
              onclick={() => (envFilter = e)}
            >{e === "" ? t("hosts.env.all") : e === "production" ? t("hosts.env.production") : e === "development" ? t("hosts.env.development") : e === "staging" ? t("hosts.env.staging") : e}</button>
          {/each}
        </div>
      {/if}
      {#if allTags.length}
        <select class="input min-w-[6.5rem] flex-1 py-1 text-xs" bind:value={tagFilter} aria-label="Filter by tag">
          <option value="">Any tag</option>
          {#each allTags as t (t)}<option value={t}>{t}</option>{/each}
        </select>
      {/if}
    </div>
  </div>

  {#if ui.selectedHosts.size}
    <div class="mx-3 mb-2 flex flex-wrap items-center gap-2 rounded-md border border-accent/40 bg-accent/10 px-2 py-1.5 text-xs">
      <span class="flex-1">{ui.selectedHosts.size} selected</span>
      <button class="btn-primary py-0.5 text-xs" onclick={() => (ui.modal = { kind: "bulk-edit" })}>Edit…</button>
      <button
        class="btn-ghost py-0.5 text-xs"
        onclick={() => {
          const hs = [...ui.selectedHosts].map((id) => ({ id, label: vaultStore.hostById.get(id)?.data?.label ?? "" }));
          ui.openMany(hs, "tabs", "Selection");
        }}>Open</button
      >
      <button class="btn-ghost py-0.5 text-xs" onclick={() => (ui.selectedHosts = new Set(filtered.map((h) => h.id)))} title="Select every host shown">All</button>
      <button class="btn-ghost py-0.5 text-xs" onclick={() => (ui.selectedHosts = new Set())}>Clear</button>
    </div>
  {/if}

  <!-- Dropping a host on empty space moves it to the top level. -->
  <div
    bind:this={treeEl}
    bind:clientHeight={viewport}
    onscroll={onScroll}
    class="relative flex-1 overflow-y-auto px-2 pb-4 {rootDrop ? 'bg-accent/5' : ''}"
    role="tree"
    tabindex="-1"
    onkeydown={onTreeKeydown}
    onfocusin={(e) => {
      const key = (e.target as HTMLElement).closest("[data-tree-key]")?.getAttribute("data-tree-key");
      if (key) focusedKey = key;
    }}
    ondragover={(e) => {
      if (acceptsHost(e)) {
        e.preventDefault();
        rootDrop = true;
      }
    }}
    ondragleave={(e) => {
      if (e.currentTarget === e.target) rootDrop = false;
    }}
    ondrop={(e) => {
      rootDrop = false;
      void dropHostInto(e, "");
    }}
  >
    {#if vaultStore.loading}
      <div class="flex justify-center py-6"><Spinner label="Decrypting…" /></div>
    {:else if vaultStore.hosts.length === 0}
      <div class="anim-rise px-3 py-8 text-center">
        <Illustration scene="hosts" size={128} />
        <p class="mt-2 text-sm font-medium">{t("hosts.empty")}</p>
        <p class="mt-1 text-xs text-fg-muted">{t("hosts.empty.hint")}</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "host", id: null })}>
          <Plus size={14} /> {t("hosts.add")}
        </button>
      </div>
    {:else if filtered.length === 0}
      <p class="px-2 py-6 text-center text-xs text-fg-muted">
        {favoritesOnly && !ui.search && !envFilter && !tagFilter ? "No favorites yet. Star a host to pin it here." : t("hosts.noMatches")}
      </p>
    {:else}
      {#if favorites.length}
        <div class="mb-1">
          <button class="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-sm text-fg-muted hover:bg-panel-hover" aria-expanded={!favoritesFolded} onclick={() => ui.toggleGroup(FAV_KEY)}>
            {#if favoritesFolded}<ChevronRight size={14} />{:else}<ChevronDown size={14} />{/if}
            <Star size={13} class="text-warning" fill="currentColor" />
            <span class="font-medium text-fg">{t("hosts.favorites")}</span>
            <span class="text-xs">{favorites.length}</span>
          </button>
          {#if !favoritesFolded}
            {#each favorites as host (host.id)}
              <HostRow {host} depth={0} treeKey="f:{host.id}" focused={`f:${host.id}` === effectiveFocusedKey} />
            {/each}
          {/if}
        </div>
      {/if}
      {#if recentShown.length && !ui.search.trim() && !favoritesOnly}
        <div class="mb-2">
          <div class="flex items-center gap-1.5 px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">
            <Clock size={11} /> {t("hosts.recent")}
          </div>
          {#each recentShown as host (host.id)}
            <HostRow {host} depth={0} treeKey="r:{host.id}" focused={`r:${host.id}` === effectiveFocusedKey} />
          {/each}
          <div class="mx-2 mt-2 border-t border-line"></div>
        </div>
      {/if}
      {#if virtual}
        <div bind:this={listEl} class="relative" style:height="{win.total}px">
          <div style:transform="translateY({win.top}px)">
            {#each shownRows as row (row.key)}
              {#if row.kind === "group"}
                {@const g = groupMap.get(row.path)}
                {#if g}<GroupRow node={g} depth={row.depth} collapsed={folded.has(row.path)} focused={row.key === effectiveFocusedKey} />{/if}
              {:else}
                {@const h = vaultStore.hostById.get(row.id)}
                {#if h}<HostRow host={h} depth={row.depth} treeKey={row.key} focused={row.key === effectiveFocusedKey} />{/if}
              {/if}
            {/each}
          </div>
        </div>
      {:else}
        <HostTreeNode node={tree} depth={0} {effectiveFocusedKey} />
      {/if}
    {/if}
  </div>
</aside>
