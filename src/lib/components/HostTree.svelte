<script lang="ts">
  import { Plus, Search, Server } from "lucide-svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { buildTree } from "$lib/tree";
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
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Hosts</h2>
    <button class="icon-btn" title="New host" onclick={() => (ui.modal = { kind: "host", id: null })}>
      <Plus size={16} />
    </button>
  </div>

  <div class="px-3 pb-2">
    <div class="relative">
      <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input py-1.5 pl-8" placeholder="Search hosts…" bind:value={ui.search} />
    </div>
  </div>

  <div class="flex-1 overflow-y-auto px-2 pb-4">
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
      <HostTreeNode node={tree} depth={0} />
    {/if}
  </div>
</aside>
