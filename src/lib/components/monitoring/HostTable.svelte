<script lang="ts">
  import { ArrowDown, ArrowUp, ChevronDown, ChevronUp } from "lucide-svelte";
  import HealthBadge from "./HealthBadge.svelte";
  import { formatRate, sparkline, type RateUnit } from "$lib/netdata";
  import { netStore } from "$lib/stores/netstore.svelte";
  import { ago, formatValue } from "$lib/monitoring";
  import type { HostRow, SortKey } from "$lib/monitoringview";

  let { rows, selectedId, compact, now, unit, sort, dir, onsort, onpick }: {
    rows: HostRow[];
    selectedId: string | null;
    compact: boolean;
    now: number;
    unit: RateUnit;
    sort: SortKey;
    dir: "asc" | "desc";
    onsort: (k: SortKey) => void;
    onpick: (id: string) => void;
  } = $props();

  function key(e: KeyboardEvent, i: number) {
    const go = (to: number) => {
      const next = rows[Math.min(rows.length - 1, Math.max(0, to))];
      if (!next) return;
      e.preventDefault();
      onpick(next.id);
      queueMicrotask(() => document.querySelector<HTMLElement>(`[data-host-row="${next.id}"]`)?.focus());
    };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(rows.length - 1);
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onpick(rows[i].id);
    }
  }

  const spark = (id: string, name: string, which: "rx" | "tx") => sparkline(netStore.points(id, name), (p) => p[which], 56, 18, 30);
  const th = "border-b border-line px-3 py-2";
</script>

{#snippet head(label: string, k: SortKey, right = false)}
  <th class="{th} {right ? 'text-right' : ''}" aria-sort={sort === k ? (dir === "asc" ? "ascending" : "descending") : "none"}>
    <button class="inline-flex items-center gap-1 hover:text-fg" onclick={() => onsort(k)}>{label}{#if sort === k}{#if dir === "asc"}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}{/if}</button>
  </th>
{/snippet}

<div class="min-h-0 flex-1 overflow-auto">
  <table class="w-full border-separate border-spacing-0 text-sm">
    <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
      <tr>
        {@render head("Host", "name")}
        {@render head("Health", "health")}
        {@render head("CPU", "cpu", true)}
        {#if !compact}{@render head("Memory", "mem", true)}{/if}
        {@render head("↓ Download", "rx", true)}
        {@render head("↑ Upload", "tx", true)}
        {#if !compact}<th class="{th} text-right">Ping</th><th class="{th} text-right">Checked</th>{/if}
      </tr>
    </thead>
    <tbody>
      {#each rows as r, i (r.id)}
        {@const st = r.status}
        <tr data-host-row={r.id} tabindex="0" aria-selected={selectedId === r.id} class="cursor-pointer outline-none transition-colors focus-visible:bg-accent/10 {selectedId === r.id ? 'bg-accent/10' : 'hover:bg-panel-hover'} {r.target.enabled ? '' : 'opacity-60'}" onclick={() => onpick(r.id)} onkeydown={(e) => key(e, i)}>
          <td class="relative max-w-[16rem] border-b border-line/60 px-3 py-2.5">
            {#if selectedId === r.id}<span class="absolute inset-y-0 left-0 w-0.5 bg-accent" aria-hidden="true"></span>{/if}
            <div class="truncate font-medium">{r.target.label}</div>
            <div class="truncate font-mono text-[11px] text-fg-muted">{r.target.address}</div>
          </td>
          <td class="border-b border-line/60 px-3 py-2.5"><HealthBadge health={r.health} /></td>
          <td class="border-b border-line/60 px-3 py-2.5 text-right font-mono text-xs tabular-nums">{formatValue("cpu", st?.metrics?.cpu ?? null)}</td>
          {#if !compact}<td class="border-b border-line/60 px-3 py-2.5 text-right font-mono text-xs tabular-nums">{formatValue("mem", st?.metrics?.mem ?? null)}</td>{/if}
          {#each [["rx", ArrowDown, "#38bdf8"], ["tx", ArrowUp, "#a78bfa"]] as [w, Icon, color] (w)}
            {@const v = r.net ? (w === "rx" ? r.net.rx : r.net.tx) : null}
            <td class="border-b border-line/60 px-3 py-2.5 text-right">
              {#if r.net}
                <div class="flex items-center justify-end gap-2">
                  <svg viewBox="0 0 56 18" class="h-[18px] w-14 max-lg:hidden" aria-hidden="true">{#if spark(r.id, r.net.name, w as "rx" | "tx")}<polyline points={spark(r.id, r.net.name, w as "rx" | "tx")} fill="none" stroke={color as string} stroke-width="1.3" vector-effect="non-scaling-stroke" />{/if}</svg>
                  <span class="font-mono text-xs tabular-nums">{formatRate(v, unit).value} <span class="text-fg-muted">{formatRate(v, unit).unit}</span></span>
                </div>
              {:else}<span class="text-fg-muted" title="Needs an SSH host with network counters (Linux)">–</span>{/if}
            </td>
          {/each}
          {#if !compact}
            <td class="border-b border-line/60 px-3 py-2.5 text-right font-mono text-xs tabular-nums">{#if st?.ping}{st.ping.avg_ms !== null ? `${st.ping.avg_ms.toFixed(0)} ms` : "lost"}{:else}–{/if}</td>
            <td class="border-b border-line/60 px-3 py-2.5 text-right text-xs text-fg-muted">{st ? ago(st.checked_at, now) : "–"}</td>
          {/if}
        </tr>
      {/each}
    </tbody>
  </table>
</div>
