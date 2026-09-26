<script lang="ts">
  import { Activity, Clock, FileInput, Plus, Search, Server } from "lucide-svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import HostRow from "./HostRow.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { buildTree } from "$lib/tree";
  import { acceptsHost, dropHostInto } from "$lib/hostdrag.svelte";
  import HostTreeNode from "./HostTreeNode.svelte";

  const filtered = $derived.by(() => {
    const q = ui.search.trim().toLowerCase();
    if (!q) return vaultStore.hosts;
    return vaultStore.hosts.filter((h) => {
      const d = h.data;
      if (!d) return false;
      return [d.label, d.hostname, d.group, ...d.tags].some((s) => s.toLowerCase().includes(q));
    });
  });
  const tree = $derived(buildTree(filtered));
  let rootDrop = $state(false);
  const recent = $derived(
    settings.recent
      .map((id) => vaultStore.hostById.get(id))
      .filter((h): h is NonNullable<typeof h> => !!h?.data)
      .slice(0, 5),
  );
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Hosts</h2>
    <div class="flex">
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
  </div>

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
      <p class="px-2 py-6 text-center text-xs text-fg-muted">No matches.</p>
    {:else}
      {#if recent.length && !ui.search.trim()}
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
