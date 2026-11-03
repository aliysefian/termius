<script lang="ts">
  import { Lock, LockOpen, Plug } from "lucide-svelte";
  import Badge from "../Badge.svelte";
  import { engineInfo } from "$lib/dbengines";
  import { databases, type QueryTab } from "$lib/stores/databases.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo } from "$lib/types";

  let { tab }: { tab: QueryTab } = $props();

  const rec = $derived(vaultStore.dbConnections.find((c) => c.id === tab.connId));
  const c = $derived(rec?.data);
  const s = $derived(databases.sessions[tab.connId]);
  const env = $derived(envInfo(c?.environment));
  const connections = $derived(vaultStore.dbConnections.filter((x) => x.data));
  const readOnly = $derived(databases.isReadOnlyConnection(tab.connId));
  const production = $derived(databases.isProduction(tab.connId));

  async function switchTo(id: string) {
    if (id === tab.connId) return;
    tab.connId = id;
    await databases.connect(id);
  }
</script>

<div class="flex h-8 shrink-0 items-center gap-2 border-b border-line bg-base px-3 text-xs {production ? 'bg-danger/5' : ''}" aria-label="Connection context">
  <span class="h-2 w-2 shrink-0 rounded-full {s ? 'bg-success' : 'bg-fg-muted/40'}" title={s ? "Connected" : "Not connected"}></span>
  {#if tab.table}
    <span class="truncate font-medium">{c?.name}</span>
  {:else}
    <label class="sr-only" for="ctx-conn">Connection of this query</label>
    <select id="ctx-conn" class="input !w-auto h-6 max-w-56 py-0 text-xs font-medium" value={tab.connId} onchange={(e) => void switchTo(e.currentTarget.value)} disabled={tab.running}>
      {#each connections as x (x.id)}<option value={x.id}>{x.data!.name}</option>{/each}
    </select>
  {/if}
  {#if env.short}<Badge tone={env.tone ?? "neutral"}>{env.short}</Badge>{/if}
  <span class="truncate text-fg-muted">{c ? engineInfo(c.engine).label : ""}{#if s?.version} · {s.version}{/if}{#if c?.database} · {c.database}{/if}</span>
  <span class="ml-auto flex shrink-0 items-center gap-2 text-fg-muted">
    {#if readOnly}<span class="inline-flex items-center gap-1 text-success"><Lock size={11} /> read-only</span>{:else}<span class="inline-flex items-center gap-1"><LockOpen size={11} /> read-write</span>{/if}
    {#if !s}<button class="inline-flex items-center gap-1 text-accent hover:underline" onclick={() => void databases.connect(tab.connId)}><Plug size={11} /> Connect</button>{/if}
  </span>
</div>
