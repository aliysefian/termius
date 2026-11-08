<script lang="ts">
  import { onMount } from "svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { AlertTriangle, ArrowDown, ArrowUp, Copy, Loader2, RefreshCw, ScrollText, Waypoints } from "lucide-svelte";
  import { addressText, filterPorts, scopeText, sortPorts, tunnelFor, type Dir, type PortFilter, type PortSort } from "$lib/hostlists";
  import { hostDetails } from "$lib/stores/hostdetails.svelte";
  import { ops } from "$lib/stores/ops.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { hostId }: { hostId: Uuid } = $props();
  const label = $derived(vaultStore.hostById.get(hostId)?.data?.label ?? "host");
  const list = $derived(hostDetails.ports[hostId] ?? null);
  const error = $derived(hostDetails.errors[`ports:${hostId}`] ?? null);
  const loading = $derived(!!hostDetails.loading[`ports:${hostId}`] && !list);

  let f = $state<PortFilter>({ query: "", proto: "all", scope: "all", process: "" });
  let sort = $state<{ key: PortSort; dir: Dir }>({ key: "port", dir: "asc" });
  let refreshing = $state(false);

  onMount(() => hostDetails.watch("ports", hostId));

  const processes = $derived([...new Set((list?.items ?? []).map((p) => p.process).filter((p): p is string => !!p))].sort());
  const rows = $derived(list ? sortPorts(filterPorts(list.items, f), sort.key, sort.dir) : []);

  function sortBy(key: PortSort) {
    sort = sort.key === key ? { key, dir: sort.dir === "asc" ? "desc" : "asc" } : { key, dir: "asc" };
  }
  async function copy(text: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied ${text}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
  async function refresh() {
    refreshing = true;
    try {
      await hostDetails.refresh("ports", hostId, true);
    } finally {
      refreshing = false;
    }
  }
  const HEADS: [PortSort | null, string][] = [["proto", "Proto"], ["address", "Address"], ["port", "Port"], ["process", "Process"], [null, "User"], [null, "Reach"]];
</script>

<div class="flex h-full min-h-0 flex-col gap-2" data-testid="port-table">
  <div class="flex flex-wrap items-center gap-2 text-xs">
    <input class="input h-8 w-56 text-xs" placeholder="Search port, address or process…" aria-label="Search ports" bind:value={f.query} />
    <select class="input h-8 w-24 py-0 text-xs" aria-label="Protocol" bind:value={f.proto}><option value="all">All protocols</option><option value="tcp">TCP</option><option value="udp">UDP</option></select>
    <select class="input h-8 w-36 py-0 text-xs" aria-label="Scope" bind:value={f.scope}><option value="all">Any address</option><option value="public">Not loopback only</option><option value="local">Loopback only</option></select>
    <select class="input h-8 w-36 py-0 text-xs" aria-label="Process" bind:value={f.process}><option value="">Any process</option>{#each processes as p (p)}<option value={p}>{p}</option>{/each}</select>
    <span class="text-fg-muted" aria-live="polite">{#if list}{rows.length === list.items.length ? `${list.items.length} listening` : `${rows.length} of ${list.items.length}`}{/if}</span>
    <button class="icon-btn ml-auto h-7 w-7" title="Refresh now" aria-label="Refresh ports" onclick={refresh} disabled={refreshing}><RefreshCw size={14} class={refreshing ? "animate-spin" : ""} /></button>
  </div>
  {#if error}
    <p class="flex items-start gap-2 rounded-md border {list ? 'border-warning/40 bg-warning/10 text-warning' : 'border-danger/30 bg-danger/10 text-danger'} px-3 py-2 text-xs" role="alert"><AlertTriangle size={14} class="mt-0.5 shrink-0" /><span>{list ? `The last reading failed (${error}); showing the one before it.` : error}</span></p>
  {/if}

  {#if loading}
    <div class="flex flex-1 items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading the listening ports…</div>
  {:else if list}
    <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-line bg-panel">
      <table class="w-full min-w-[44rem] text-left text-xs">
        <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
          <tr>
            {#each HEADS as [k, name] (name)}
              <th class="px-2 py-1.5 font-medium" aria-sort={k && sort.key === k ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
                {#if k}<button class="inline-flex items-center gap-1 hover:text-fg" onclick={() => sortBy(k)}>{name}{#if sort.key === k}{#if sort.dir === "asc"}<ArrowUp size={11} />{:else}<ArrowDown size={11} />{/if}{/if}</button>{:else}{name}{/if}
              </th>
            {/each}
            <th class="px-2 py-1.5 text-right font-medium">Actions</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as p (`${p.proto}|${p.address}|${p.port}`)}
            {@const tunnel = tunnelFor(p, hostId, label)}
            <tr class="border-t border-line/60 hover:bg-panel-hover/50">
              <td class="px-2 py-1 uppercase">{p.proto}{#if p.ipv6}<span class="ml-1 normal-case text-fg-muted">v6</span>{/if}</td>
              <td class="px-2 py-1 font-mono">{p.address === "0.0.0.0" || p.address === "::" || p.address === "*" ? `all (${p.address})` : p.address}</td>
              <td class="px-2 py-1 font-mono text-sm">{p.port}</td>
              <td class="px-2 py-1">{#if p.process}{p.process}{#if p.pid !== null} <span class="text-fg-muted">({p.pid})</span>{/if}{:else}<span class="text-fg-muted">owner not shown</span>{/if}</td>
              <td class="px-2 py-1">{p.user ?? "—"}</td>
              <td class="px-2 py-1 {p.local_only ? '' : 'text-fg-muted'}" title={scopeText(p)}>{p.local_only ? "This host only" : "Bound beyond loopback"}</td>
              <td class="px-2 py-1">
                <div class="flex justify-end gap-0.5">
                  <button class="icon-btn h-6 w-6" title="Copy {addressText(p)}" aria-label="Copy address {addressText(p)}" onclick={() => copy(addressText(p))}><Copy size={13} /></button>
                  {#if tunnel}<button class="icon-btn h-6 w-6" title="Open an SSH tunnel to this port from this computer" aria-label="Create an SSH tunnel to port {p.port}" onclick={() => (ui.modal = { kind: "forward", id: null, template: tunnel })}><Waypoints size={13} /></button>{/if}
                  {#if p.process}<button class="icon-btn h-6 w-6" title="Show {p.process}'s logs in Operations" aria-label="Show logs of {p.process}" onclick={() => { ui.modal = null; void ops.viewLogs(hostId, undefined, { text: p.process ?? "" }); }}><ScrollText size={13} /></button>{/if}
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if rows.length === 0}<p class="py-8 text-center text-xs text-fg-muted">{list.items.length ? "No listener matches." : "Nothing is listening."}</p>{/if}
    </div>
    <p class="text-[11px] text-fg-muted">
      Read with <code>{list.source}</code>.
      {#if list.owners_hidden}Some owners are not shown: the host only tells a user about their own programs, so the rest needs root.{/if}
      {#if list.source === "proc"}This host has neither <code>ss</code> nor <code>netstat</code>, so only the kernel's tables are read, which name no program.{/if}
      Listening is not the same as reachable: whether anything outside can connect depends on the address, the firewall and the network. A tunnel opens the port on this computer through SSH.
    </p>
  {/if}
</div>
