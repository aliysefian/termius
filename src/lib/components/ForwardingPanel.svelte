<script lang="ts">
  import { ArrowLeftRight, Globe, Loader2, Pencil, Play, Plus, Search, Square, Trash2 } from "lucide-svelte";
  import { matchesForward, statusWord, type StatusFilter } from "$lib/forwardsearch";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ui } from "$lib/stores/ui.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { describeForward, errorMessage, type ForwardRule, type ForwardStatus } from "$lib/types";

  /** Local rules usually front a web UI; open it in the browser. */
  function browserUrl(d: ForwardRule, st: ForwardStatus | undefined): string | null {
    if (d.kind !== "local" || st?.state !== "active") return null;
    const host = d.bind_addr === "0.0.0.0" || d.bind_addr === "::" ? "127.0.0.1" : d.bind_addr;
    const port = d.bind_port || st.port;
    return `${port === 443 || d.dest_port === 443 ? "https" : "http"}://${host.includes(":") ? `[${host}]` : host}:${port}`;
  }

  function dot(s: ForwardStatus | undefined) {
    switch (s?.state) {
      case "active":
        return "bg-success";
      case "starting":
        return "bg-warning animate-pulse";
      case "error":
        return "bg-danger";
      default:
        return "bg-fg-muted/40";
    }
  }

  // -- search and filters ------------------------------------------------------
  let query = $state("");
  let hostFilter = $state("");
  let statusFilter = $state<StatusFilter>("all");

  /** Hosts that have at least one rule, for the host filter. */
  const ruleHosts = $derived(
    [...new Set(vaultStore.forwards.map((f) => f.data?.host_id).filter((h): h is string => !!h))]
      .map((id) => ({ id, label: vaultStore.hostById.get(id)?.data?.label ?? "missing host" }))
      .sort((a, b) => a.label.localeCompare(b.label)),
  );

  const shown = $derived(
    vaultStore.forwards.filter((f) => {
      const d = f.data;
      if (!d) return false;
      if (hostFilter && d.host_id !== hostFilter) return false;
      return matchesForward(d, vaultStore.hostById.get(d.host_id)?.data, vaultStore.forwardStatus[f.id], query, statusFilter);
    }),
  );

  /** Shown rules grouped under their host, hosts in name order. */
  const byHost = $derived.by(() => {
    const groups = new Map<string, typeof shown>();
    for (const f of shown) {
      const id = f.data!.host_id;
      groups.set(id, [...(groups.get(id) ?? []), f]);
    }
    return [...groups.entries()]
      .map(([id, list]) => ({ id, label: vaultStore.hostById.get(id)?.data?.label ?? "missing host", list: list.sort((a, b) => a.data!.label.localeCompare(b.data!.label)) }))
      .sort((a, b) => a.label.localeCompare(b.label));
  });

  const filtering = $derived(!!query.trim() || !!hostFilter || statusFilter !== "all");
  const stoppedShown = $derived(shown.filter((f) => statusWord(vaultStore.forwardStatus[f.id]) !== "running" && vaultStore.hostById.has(f.data!.host_id)));
  const runningShown = $derived(shown.filter((f) => statusWord(vaultStore.forwardStatus[f.id]) === "running"));

  async function remove(id: string, label: string) {
    if (!await ask(`Delete forwarding rule "${label}"?`)) return;
    await vaultStore.deleteForward(id);
  }
</script>

<aside class="flex min-w-0 flex-1 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Tunnels</h2>
    <button class="icon-btn" title="New rule" onclick={() => (ui.modal = { kind: "forward", id: null })}>
      <Plus size={16} />
    </button>
  </div>
  {#if vaultStore.forwards.length > 0}
    <div class="space-y-1.5 px-3 pb-2">
      <div class="relative">
        <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input py-1.5 pl-7 text-sm" placeholder="Search rules, hosts, ports…" bind:value={query} aria-label="Search forwarding rules" />
      </div>
      <div class="flex gap-1.5">
        <select class="input flex-1 py-1 text-xs" bind:value={hostFilter} aria-label="Filter by host">
          <option value="">All hosts</option>
          {#each ruleHosts as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
        </select>
        <select class="input w-28 py-1 text-xs" bind:value={statusFilter} aria-label="Filter by status">
          <option value="all">Any status</option>
          <option value="running">Running</option>
          <option value="stopped">Stopped</option>
          <option value="error">Failed</option>
        </select>
      </div>
      {#if filtering && shown.length}
        <div class="flex items-center justify-between text-[11px] text-fg-muted">
          <span>{shown.length} of {vaultStore.forwards.length} rules</span>
          <span class="flex gap-2">
            {#if stoppedShown.length}<button class="hover:text-fg" onclick={() => stoppedShown.forEach((f) => vaultStore.startForward(f.id))}>Start these</button>{/if}
            {#if runningShown.length}<button class="hover:text-fg" onclick={() => runningShown.forEach((f) => vaultStore.stopForward(f.id))}>Stop these</button>{/if}
          </span>
        </div>
      {/if}
    </div>
  {/if}
  <div class="flex-1 overflow-y-auto px-2 pb-4">
    {#if vaultStore.forwards.length === 0}
      <div class="px-3 py-10 text-center">
        <ArrowLeftRight size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm text-fg-muted">No forwarding rules yet.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "forward", id: null })}>
          <Plus size={14} /> Add rule
        </button>
      </div>
    {:else}
      {#each byHost as g (g.id)}
        {#if !hostFilter}
          <div class="mt-2 flex items-center justify-between px-2 pb-0.5 text-[11px] font-medium text-fg-muted first:mt-0">
            <span class="truncate">{g.label}</span>
            <span>{g.list.length}</span>
          </div>
        {/if}
        {#each g.list as f (f.id)}
          {@const d = f.data!}
          {@const st = vaultStore.forwardStatus[f.id]}
          {@const host = vaultStore.hostById.get(d.host_id)?.data}
          {@const running = st?.state === "active" || st?.state === "starting"}
          <div class="group rounded-md px-2 py-2 hover:bg-panel-hover">
            <div class="flex items-center gap-2">
              <span class="h-2 w-2 shrink-0 rounded-full {dot(st)}" title={st?.state ?? "stopped"}></span>
              <div class="min-w-0 flex-1 truncate text-sm">{d.label}</div>
              <div class="reveal flex">
                <button class="icon-btn h-6 w-6" title="Edit" disabled={running} onclick={() => (ui.modal = { kind: "forward", id: f.id })}><Pencil size={12} /></button>
                <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" onclick={() => remove(f.id, d.label)}><Trash2 size={12} /></button>
              </div>
              {#if browserUrl(d, st)}
                <button class="icon-btn h-7 w-7" title="Open {browserUrl(d, st)} in the browser" onclick={() => openUrl(browserUrl(d, st)!).catch((e) => ui.notify("error", errorMessage(e)))}>
                  <Globe size={14} />
                </button>
              {/if}
              {#if running}
                <button class="icon-btn h-7 w-7 text-success" title="Stop" onclick={() => vaultStore.stopForward(f.id)}>
                  {#if st?.state === "starting"}<Loader2 size={14} class="animate-spin" />{:else}<Square size={14} />{/if}
                </button>
              {:else}
                <button class="icon-btn h-7 w-7" title="Start" disabled={!host} onclick={() => vaultStore.startForward(f.id)}><Play size={14} /></button>
              {/if}
            </div>
            <div class="mt-0.5 truncate pl-4 font-mono text-xs text-fg-muted">{describeForward(d)}</div>
            <div class="truncate pl-4 text-xs text-fg-muted">
              via {host?.label ?? "missing host"}
              {#if st?.state === "active" && d.bind_port === 0}· port {st.port}{/if}
            </div>
            {#if st?.state === "error"}
              <div class="mt-1 pl-4 text-xs text-danger" title={st.message}>{st.message}</div>
            {/if}
          </div>
        {/each}
      {:else}
        <p class="px-2 py-6 text-center text-xs text-fg-muted">No rules match.</p>
      {/each}
    {/if}
  </div>
</aside>
