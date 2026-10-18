<script lang="ts">
  import { ExternalLink, Power } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { localUrl } from "$lib/nethops";
  import { networkStore } from "$lib/stores/network.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  // The tunnels started from this pane's detected ports, as badges for its header: where it listens here, where it
  // goes there, a button to open it in the browser and one to stop it.
  let { paneId }: { paneId: string } = $props();

  const list = $derived(networkStore.forwards[paneId] ?? []);

  async function open(port: number) {
    try {
      await openUrl(localUrl(port));
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#each list as f (f.id)}
  {@const status = vaultStore.forwardStatus[f.id]}
  {@const local = status?.state === "active" ? status.port : null}
  <span
    class="flex shrink-0 items-center gap-1.5 rounded border px-1.5 py-0.5 font-mono text-[11px] tabular-nums
      {status?.state === 'error' ? 'border-danger/30 bg-danger/10 text-danger' : 'border-line bg-panel text-fg'}"
    data-testid="pane-forward"
    data-state={status?.state ?? "starting"}
    title={status?.state === "error" ? status.message : `Tunnel through this session's host`}
  >
    <span class="h-1.5 w-1.5 shrink-0 rounded-full {status?.state === 'active' ? 'bg-success' : status?.state === 'error' ? 'bg-danger' : 'animate-pulse bg-warning'}"></span>
    {#if status?.state === "error"}
      <span class="max-w-[12rem] truncate">:{f.remotePort} failed</span>
    {:else}
      <span>localhost:{local ?? "…"} ➔ remote:{f.remotePort}</span>
    {/if}
    {#if local !== null}
      <button class="rounded p-0.5 text-fg-muted hover:bg-panel-hover hover:text-fg" title="Open in browser" aria-label="Open in browser" onclick={() => open(local)}><ExternalLink size={11} /></button>
    {/if}
    <button class="rounded p-0.5 text-fg-muted hover:bg-panel-hover hover:text-danger" title="Stop tunnel" aria-label="Stop tunnel" onclick={() => void networkStore.stopForward(paneId, f.id)}><Power size={11} /></button>
  </span>
{/each}
