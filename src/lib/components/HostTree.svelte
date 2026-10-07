<script lang="ts">
  import Illustration from "./Illustration.svelte";
  import { Activity, Clock, Cloud, FileInput, FoldVertical, Plus, Search, Server, UnfoldVertical } from "lucide-svelte";
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
  // While searching, every group opens: a match inside a folded group would otherwise be hidden.
  const folded = $derived(ui.search.trim() ? new Set<string>() : ui.collapsedGroups);
  const treeRows = $derived(flattenTree(tree, folded));
  const navRows = $derived([...(!ui.search.trim() && !favoritesOnly ? recentRows : []), ...treeRows]);

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

<aside class="flex min-w-0 flex-1 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between gap-1 px-4 pt-4 pb-2 @max-[16rem]:px-3">
    <h2 class="min-w-0 truncate text-sm font-semibold">{favoritesOnly ? t("hosts.favorites") : t("hosts.title")}</h2>
    <div class="flex shrink-0 [&>.icon-btn]:@max-[16rem]:h-7 [&>.icon-btn]:@max-[16rem]:w-7">
      {#if paths.length}
        {#if anyExpanded}
          <button class="icon-btn" title="Collapse all groups" onclick={() => ui.collapseGroups(paths)}><FoldVertical size={16} /></button>
        {:else}
          <button class="icon-btn" title="Expand all groups" onclick={() => ui.expandAllGroups()}><UnfoldVertical size={16} /></button>
        {/if}
      {/if}
      <button class="icon-btn" title="Check which hosts are reachable" disabled={vaultStore.checking} onclick={() => vaultStore.checkHealth()}>
        <Activity size={16} class={vaultStore.checking ? "animate-pulse text-accent" : ""} />
      </button>
      <button class="icon-btn" title="Import from ~/.ssh/config or an Ansible inventory" onclick={() => (ui.modal = { kind: "import-ssh-config" })}>
        <FileInput size={16} />
      </button>
      <button class="icon-btn" title="Import from a cloud, Tailscale, Kubernetes or Terraform" onclick={() => (ui.modal = { kind: "import-inventory" })}>
        <Cloud size={16} />
      </button>
      <button class="icon-btn" title="New host" onclick={() => (ui.modal = { kind: "host", id: null })}>
        <Plus size={16} />
      </button>
    </div>
  </div>

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
      <select class="input w-auto min-w-[6.5rem] flex-1 py-1 text-xs @min-[20rem]:w-24 @min-[20rem]:flex-none" bind:value={sortBy} aria-label="Sort hosts">
        <option value="name">Name</option>
        <option value="hostname">Address</option>
        <option value="updated">Recent edits</option>
        <option value="lastused">Last used</option>
      </select>
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
      {#if recent.length && !ui.search.trim() && !favoritesOnly}
        <div class="mb-2">
          <div class="flex items-center gap-1.5 px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">
            <Clock size={11} /> {t("hosts.recent")}
          </div>
          {#each recent as host (host.id)}
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
