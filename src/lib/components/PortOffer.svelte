<script lang="ts">
  import { SquareTerminal, X } from "lucide-svelte";
  import { networkStore } from "$lib/stores/network.svelte";
  import type { Uuid } from "$lib/types";

  // A small pill in a corner of the terminal: a server in this session started listening on a port that is not
  // forwarded yet. It never takes the terminal's focus and goes away with one click.
  let { paneId, hostId }: { paneId: string; hostId: Uuid } = $props();

  const offers = $derived(networkStore.offers[paneId] ?? []);
  const port = $derived(offers[0]);
</script>

{#if port !== undefined}
  <div
    class="anim-pop pointer-events-auto absolute bottom-3 right-3 z-20 flex max-w-[calc(100%-1.5rem)] items-center gap-3 rounded-md border border-line bg-panel/95 px-3 py-2 text-xs shadow-sm"
    role="status"
    data-testid="port-offer"
  >
    <SquareTerminal size={14} class="shrink-0 text-fg-muted" aria-hidden="true" />
    <span class="min-w-0 truncate">
      Port <span class="font-mono tabular-nums">{port}</span> detected on remote{#if offers.length > 1}<span class="text-fg-muted"> · +{offers.length - 1} more</span>{/if}
    </span>
    <button class="btn-primary shrink-0 px-2 py-1 text-xs" onclick={() => void networkStore.forwardPort(paneId, hostId, port)}>
      Forward to <span class="font-mono tabular-nums">:{port}</span>
    </button>
    <button class="icon-btn h-6 w-6 shrink-0" title="Not now" aria-label="Dismiss" onclick={() => networkStore.dismissOffer(paneId, port)}><X size={13} /></button>
  </div>
{/if}
