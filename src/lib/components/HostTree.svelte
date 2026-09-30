<script lang="ts">
  import { Activity, Clock, FileInput, FoldVertical, Plus, Search, Server, UnfoldVertical } from "lucide-svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import HostRow from "./HostRow.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { buildTree, groupPaths, type HostSort } from "$lib/tree";
  import { ENVIRONMENTS } from "$lib/types";
  import { acceptsHost, dropHostInto } from "$lib/hostdrag.svelte";
  import HostTreeNode from "./HostTreeNode.svelte";

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
</script>

<aside class="flex min-w-0 flex-1 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between gap-1 px-4 pt-4 pb-2 @max-[16rem]:px-3">
    <h2 class="min-w-0 truncate text-sm font-semibold">{favoritesOnly ? "Favorites" : "Hosts"}</h2>
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
      <button class="icon-btn" title="New host" onclick={() => (ui.modal = { kind: "host", id: null })}>
        <Plus size={16} />
      </button>
    </div>
  </div>

  <div class="px-3 pb-2">
    <div class="relative">
      <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input py-1.5 pl-8" placeholder="Search hosts…" bind:value={ui.search} />
    </div>
    <div class="mt-1.5 flex flex-wrap gap-1.5">
      <select class="input min-w-[6.5rem] flex-1 py-1 text-xs" bind:value={envFilter} aria-label="Filter by environment">
        <option value="">Any environment</option>
        {#each envChoices as e (e)}<option value={e}>{e}</option>{/each}
      </select>
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
    class="flex-1 overflow-y-auto px-2 pb-4 {rootDrop ? 'bg-accent/5' : ''}"
    role="tree"
    tabindex="-1"
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
      <p class="px-2 py-6 text-center text-xs text-fg-muted">Decrypting…</p>
    {:else if vaultStore.hosts.length === 0}
      <div class="px-3 py-10 text-center">
        <Server size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm text-fg-muted">No hosts yet.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "host", id: null })}>
          <Plus size={14} /> Add your first host
        </button>
      </div>
    {:else if filtered.length === 0}
      <p class="px-2 py-6 text-center text-xs text-fg-muted">
        {favoritesOnly && !ui.search && !envFilter && !tagFilter ? "No favorites yet. Star a host to pin it here." : "No matches."}
      </p>
    {:else}
      {#if recent.length && !ui.search.trim() && !favoritesOnly}
        <div class="mb-2">
          <div class="flex items-center gap-1.5 px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">
            <Clock size={11} /> Recent
          </div>
          {#each recent as host (host.id)}
            <HostRow {host} depth={0} />
          {/each}
          <div class="mx-2 mt-2 border-t border-line"></div>
        </div>
      {/if}
      <HostTreeNode node={tree} depth={0} />
    {/if}
  </div>
</aside>
