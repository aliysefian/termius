<script lang="ts">
  import { AlertTriangle } from "lucide-svelte";
  import { countText } from "$lib/infra/inventory";
  import { infra } from "$lib/stores/infra.svelte";
  import { TYPE_ICON } from "./style";

  // Counts cover only the hosts that have been read; a card never presents a partial total as complete.
  const cards = $derived(
    [
      { label: "Hosts", c: infra.counts.hosts, icon: TYPE_ICON.host, host: true },
      { label: "Containers", c: infra.counts.containers, icon: TYPE_ICON.container, host: false },
      { label: "Services", c: infra.counts.services, icon: TYPE_ICON.systemd_service, host: false },
      { label: "Tmux sessions", c: infra.counts.tmux, icon: TYPE_ICON.tmux_session, host: false },
    ].map((x) => {
      const unread = !x.host && x.c.hostsObserved === 0 && x.c.hostsFailed === 0;
      return {
        ...x,
        value: unread ? "—" : String(x.c.known),
        line: x.host ? `${x.c.known} saved` : unread ? "Not read yet" : countText(x.c),
        warn: !x.host && (x.c.hostsFailed > 0 || x.c.stale > 0),
      };
    }),
  );
</script>

<div class="grid grid-cols-2 gap-3 xl:grid-cols-4" data-testid="infra-summary">
  {#each cards as card (card.label)}
    {@const I = card.icon}
    <div class="flex min-w-0 items-center gap-3 rounded-lg border border-line bg-panel px-3 py-2.5" title={card.host ? "Saved hosts; none were contacted to count them" : "Counts cover only the hosts that have been read"}>
      <span class="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-base text-fg-muted"><I size={17} /></span>
      <div class="min-w-0">
        <div class="text-xs text-fg-muted">{card.label}</div>
        <div class="text-xl font-semibold leading-tight tabular-nums">{card.value}</div>
        <div class="flex items-start gap-1 text-[11px] leading-snug {card.warn ? 'text-warning' : 'text-fg-muted'}">
          {#if card.warn}<AlertTriangle size={11} class="mt-0.5 shrink-0" />{/if}<span class="min-w-0">{card.line}</span>
        </div>
      </div>
    </div>
  {/each}
</div>
