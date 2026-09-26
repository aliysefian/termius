<script lang="ts">
  import { ArrowLeftRight, Globe, Loader2, Pencil, Play, Plus, Square, Trash2 } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ui } from "$lib/stores/ui.svelte";
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

  async function remove(id: string, label: string) {
    if (!confirm(`Delete forwarding rule "${label}"?`)) return;
    await vaultStore.deleteForward(id);
  }
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Port Forwarding</h2>
    <button class="icon-btn" title="New rule" onclick={() => (ui.modal = { kind: "forward", id: null })}>
      <Plus size={16} />
    </button>
  </div>
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
      {#each vaultStore.forwards as f (f.id)}
        {@const d = f.data!}
        {@const st = vaultStore.forwardStatus[f.id]}
        {@const host = vaultStore.hostById.get(d.host_id)?.data}
        {@const running = st?.state === "active" || st?.state === "starting"}
        <div class="group rounded-md px-2 py-2 hover:bg-panel-hover">
          <div class="flex items-center gap-2">
            <span class="h-2 w-2 shrink-0 rounded-full {dot(st)}" title={st?.state ?? "stopped"}></span>
            <div class="min-w-0 flex-1 truncate text-sm">{d.label}</div>
            <div class="flex opacity-0 group-hover:opacity-100">
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
    {/if}
  </div>
</aside>
