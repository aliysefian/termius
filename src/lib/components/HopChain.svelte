<script lang="ts">
  import { Copy, RefreshCw, SquareTerminal, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { keepInView } from "$lib/actions";
  import { formatMs, formatUptime, hopHealth, HEALTH_DOT, shortFingerprint, type HopHealth } from "$lib/nethops";
  import { networkStore } from "$lib/stores/network.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Uuid } from "$lib/types";

  // The route to a host as a row of nodes: this computer, each jump host, the host. A node is the host's state (a
  // dot) and name; the line between two nodes carries what that link adds to the round trip. The numbers come from a
  // connected session of this host (`paneId`), or from any connected session of it when only `hostId` is known.
  let { hostId, paneId, polling = true, showAlways = false }: { hostId: Uuid; paneId?: string; polling?: boolean; showAlways?: boolean } = $props();

  const pane = $derived(paneId ?? networkStore.livePaneFor(hostId));
  const live = $derived(pane ? networkStore.hops[pane] : undefined);
  const sessionUp = $derived(pane ? ui.paneInfo[pane]?.status === "connected" : false);
  const chain = $derived(networkStore.chainOf(hostId));

  interface Node {
    /** The saved host this node is, when it is one. */
    id: Uuid | undefined;
    name: string;
    address: string;
    health: HopHealth;
    segment: number | null | undefined;
    total: number | null | undefined;
    index: number;
  }

  const nodes = $derived.by<Node[]>(() => {
    const info = live?.info;
    const count = info?.length ?? chain.length;
    return Array.from({ length: count }, (_, i) => {
      const hop = info?.[i];
      // The saved host behind this node: the chain, when it matches what was connected, else found by address.
      const id = info && info.length === chain.length ? chain[i] : info ? vaultStore.hosts.find((h) => h.data?.hostname === hop?.host && h.data?.port === hop?.port)?.id : chain[i];
      const rtt = live?.rtts[i];
      return {
        id,
        name: (id && vaultStore.hostById.get(id)?.data?.label) || hop?.label || "host",
        address: hop ? `${hop.host}:${hop.port}` : (id ? `${vaultStore.hostById.get(id)?.data?.hostname ?? ""}:${vaultStore.hostById.get(id)?.data?.port ?? 22}` : ""),
        health: pane ? hopHealth(rtt, live?.misses[i] ?? 0, sessionUp) : "unknown",
        segment: rtt?.segment_ms,
        total: rtt?.total_ms,
        index: i,
      };
    });
  });

  // A direct host with nothing measured yet has no route worth drawing; a chain always does.
  const visible = $derived(showAlways || nodes.length > 1 || (pane !== undefined && live !== undefined));

  // -- live numbers -------------------------------------------------------------------------------------------------

  $effect(() => {
    const id = pane;
    if (!id || !polling || !sessionUp) return;
    void networkStore.loadHops(id).then(() => networkStore.probeHops(id));
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void networkStore.probeHops(id);
    }, 5000);
    return () => clearInterval(timer);
  });

  // -- inspector ----------------------------------------------------------------------------------------------------

  let open = $state<{ index: number; x: number; y: number } | null>(null);
  let now = $state(Date.now());
  $effect(() => {
    if (!open) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const inspected = $derived(open ? nodes[open.index] : undefined);
  const hop = $derived(open ? live?.info[open.index] : undefined);

  function inspect(e: MouseEvent, index: number) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    open = open?.index === index ? null : { index, x: r.left, y: r.bottom + 6 };
  }

  const healthText: Record<HopHealth, string> = { up: "Connected", slow: "Slow or not answering", down: "Dropped", unknown: "Not measured" };
</script>

<svelte:window onkeydown={(e) => { if (open && e.key === "Escape") open = null; }} />

{#if visible}
  <div class="flex min-w-0 items-center gap-1 font-mono text-[11px]" role="group" aria-label="Route to this host" data-testid="hop-chain">
    <span class="flex shrink-0 items-center gap-1 text-fg-muted" title="This computer">
      <span class="h-1.5 w-1.5 rounded-full bg-fg-muted/60"></span>You
    </span>
    {#each nodes as n (n.index)}
      <span class="flex shrink-0 items-center gap-1 text-fg-muted" aria-hidden="true">
        <span class="h-px w-3 bg-line"></span>
        <span class="tabular-nums {n.segment === null ? 'text-warning' : ''}" data-testid="hop-latency">{formatMs(n.segment)}</span>
        <span class="h-px w-3 bg-line"></span>
        <span class="-ml-1">▸</span>
      </span>
      <button
        type="button"
        class="flex min-w-0 max-w-[9rem] items-center gap-1.5 rounded px-1 py-0.5 text-fg hover:bg-panel-hover {open?.index === n.index ? 'bg-panel-hover' : ''}"
        title="{n.name} · {n.address}"
        aria-haspopup="dialog"
        aria-expanded={open?.index === n.index}
        onclick={(e) => inspect(e, n.index)}
        data-testid="hop-node"
      >
        <span class="h-1.5 w-1.5 shrink-0 rounded-full {HEALTH_DOT[n.health]}" data-health={n.health}></span>
        <span class="truncate">{n.name}</span>
      </button>
    {/each}
  </div>
{/if}

{#if open && inspected}
  <button type="button" class="fixed inset-0 z-40 cursor-default" aria-label="Close" onclick={() => (open = null)}></button>
  <div
    class="fixed z-50 w-72 rounded-lg border border-line bg-panel p-3 text-xs shadow-lg"
    role="dialog"
    aria-label="Hop details: {inspected.name}"
    use:keepInView={{ x: open.x, y: open.y }}
    data-testid="hop-inspector"
  >
    <div class="mb-2 flex items-center gap-2">
      <span class="h-2 w-2 shrink-0 rounded-full {HEALTH_DOT[inspected.health]}"></span>
      <span class="min-w-0 flex-1 truncate text-sm font-semibold">{inspected.name}</span>
      <button class="icon-btn h-6 w-6" aria-label="Close" onclick={() => (open = null)}><X size={12} /></button>
    </div>
    <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono tabular-nums">
      <dt class="text-fg-muted">Status</dt>
      <dd class="font-sans">{healthText[inspected.health]}</dd>
      <dt class="text-fg-muted">Address</dt>
      <dd class="truncate" title={inspected.address}>{inspected.address}</dd>
      <dt class="text-fg-muted">Round trip</dt>
      <dd>{formatMs(inspected.total)}{#if inspected.index > 0 && inspected.segment !== undefined} <span class="text-fg-muted">(+{formatMs(inspected.segment)})</span>{/if}</dd>
      {#if hop}
        <dt class="text-fg-muted">Uptime</dt>
        <dd>{formatUptime(now / 1000 - hop.connected_at)}</dd>
        <dt class="text-fg-muted">Connected in</dt>
        <dd>{formatMs(hop.connect_ms)}</dd>
        <dt class="text-fg-muted">Host key</dt>
        <dd>{hop.algorithm ?? "–"}</dd>
        <dt class="text-fg-muted">Fingerprint</dt>
        <dd class="flex items-center gap-1">
          <span class="truncate" title={hop.fingerprint ?? ""}>{shortFingerprint(hop.fingerprint)}</span>
          {#if hop.fingerprint}
            <button class="icon-btn h-5 w-5 shrink-0" title="Copy fingerprint" aria-label="Copy fingerprint" onclick={() => void writeText(hop.fingerprint ?? "").catch(() => {})}><Copy size={11} /></button>
          {/if}
        </dd>
      {/if}
    </dl>
    <div class="mt-3 flex gap-2">
      {#if pane}
        <button
          class="btn-secondary flex-1 py-1 text-xs"
          title="Reconnects the whole session, through every hop"
          onclick={() => {
            ui.requestReconnect(pane);
            open = null;
          }}
        >
          <RefreshCw size={12} /> Reconnect hop
        </button>
      {/if}
      {#if inspected.id}
        {@const target = inspected.id}
        <button
          class="btn-secondary flex-1 py-1 text-xs"
          onclick={() => {
            ui.openTerminal(target, inspected.name);
            open = null;
          }}
        >
          <SquareTerminal size={12} /> Open direct shell
        </button>
      {/if}
    </div>
    {#if !pane}
      <p class="mt-2 text-fg-muted">Numbers appear while a terminal to this host is open.</p>
    {/if}
  </div>
{/if}
