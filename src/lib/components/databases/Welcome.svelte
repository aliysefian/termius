<script lang="ts">
  import { Database, Plug, Plus, Search, SquarePlus, Table2, Info } from "lucide-svelte";
  import { databases } from "$lib/stores/databases.svelte";
  import { dbWorkspace } from "$lib/stores/dbworkspace.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let { onnewquery, onfocussearch }: { onnewquery: (connId: string) => void; onfocussearch: () => void } = $props();

  const connections = $derived(vaultStore.dbConnections.filter((c) => c.data));
  const connected = $derived(connections.filter((c) => databases.sessions[c.id]));
  const target = $derived(dbWorkspace.focus && "connId" in dbWorkspace.focus ? dbWorkspace.focus.connId : (connected[0]?.id ?? null));
</script>

<div class="flex h-full items-center justify-center p-6" data-testid="db-welcome">
  <div class="w-full max-w-md text-center">
    {#if connections.length === 0}
      <Database size={28} class="mx-auto text-accent" />
      <h2 class="mt-3 text-[15px] font-semibold">Connect to a database</h2>
      <p class="mt-1 text-sm text-fg-muted">Add a PostgreSQL, MySQL, SQL Server or other connection. It can go through one of your SSH hosts.</p>
      <div class="mt-4 flex flex-wrap justify-center gap-2">
        <button class="btn-primary text-sm" onclick={() => (ui.modal = { kind: "db-connection", id: null })}><Plus size={14} /> New connection</button>
        <button class="btn-secondary text-sm" onclick={() => (ui.view = "topology")}><Info size={14} /> How connections work</button>
      </div>
    {:else if connected.length === 0}
      <Plug size={28} class="mx-auto text-accent" />
      <h2 class="mt-3 text-[15px] font-semibold">Connect to a database</h2>
      <p class="mt-1 text-sm text-fg-muted">Pick a connection in the explorer, or connect to the first one.</p>
      <div class="mt-4 flex flex-wrap justify-center gap-2">
        <button class="btn-primary text-sm" onclick={() => connections[0] && databases.connect(connections[0].id)}><Plug size={14} /> Connect to {connections[0].data!.name}</button>
        <button class="btn-secondary text-sm" onclick={() => (ui.modal = { kind: "db-connection", id: null })}><Plus size={14} /> New connection</button>
      </div>
    {:else}
      <Database size={28} class="mx-auto text-accent" />
      <h2 class="mt-3 text-[15px] font-semibold">Ready to explore your database</h2>
      <p class="mt-1 text-sm text-fg-muted">Open a table from the explorer, or write a query.</p>
      <div class="mt-4 grid grid-cols-2 gap-2 text-sm">
        <button class="btn-primary justify-center" onclick={() => target && onnewquery(target)}><SquarePlus size={14} /> New SQL query</button>
        <button class="btn-secondary justify-center" onclick={onfocussearch}><Table2 size={14} /> Browse tables</button>
        <button class="btn-secondary justify-center" onclick={onfocussearch}><Search size={14} /> Search objects</button>
        <button class="btn-secondary justify-center" onclick={() => target && dbWorkspace.selectConnection(target)}><Info size={14} /> Connection details</button>
      </div>
    {/if}
  </div>
</div>
