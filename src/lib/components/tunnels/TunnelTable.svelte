<script lang="ts">
  import { Ellipsis, Play, Square } from "lucide-svelte";
  import TunnelStatusBadge from "./TunnelStatusBadge.svelte";
  import TunnelActionsMenu from "./TunnelActionsMenu.svelte";
  import { tunnelActions } from "./actions";
  import { tunnels } from "$lib/stores/tunnels.svelte";
  import { KIND_TEXT, formatUptime, mapping, type TunnelRow } from "$lib/tunnels";

  let { rows, compact, now }: { rows: TunnelRow[]; /** Drop the least important columns. */ compact: boolean; /** The clock for uptimes, ticked by the page. */ now: number } = $props();

  let menu = $state<{ row: TunnelRow; x: number; y: number } | null>(null);
  const allChecked = $derived(rows.length > 0 && rows.every((r) => tunnels.checked.has(r.id)));
  const someChecked = $derived(rows.some((r) => tunnels.checked.has(r.id)));

  function openMenu(e: MouseEvent, row: TunnelRow) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const context = e.type === "contextmenu";
    menu = { row, x: context ? e.clientX : r.right - 224, y: context ? e.clientY : r.bottom + 4 };
  }

  function pick(row: TunnelRow) {
    tunnels.select(row.id);
    tunnels.reveal();
  }

  /** Up and down move the selection, Enter opens the details, Space ticks the row. */
  function key(e: KeyboardEvent, i: number) {
    if ((e.target as HTMLElement).closest("button, input, a")) return;
    const go = (to: number) => {
      const next = rows[Math.min(rows.length - 1, Math.max(0, to))];
      if (!next) return;
      e.preventDefault();
      tunnels.select(next.id);
      queueMicrotask(() => document.querySelector<HTMLElement>(`[data-tunnel-row="${next.id}"]`)?.focus());
    };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(rows.length - 1);
    else if (e.key === "Enter") {
      e.preventDefault();
      pick(rows[i]);
    } else if (e.key === " ") {
      e.preventDefault();
      tunnels.toggleChecked(rows[i].id);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      menu = { row: rows[i], x: r.left + 40, y: r.bottom };
    }
  }

  const uptime = (r: TunnelRow) => (r.status === "active" && r.activeSince ? formatUptime(now - r.activeSince) : "–");
</script>

<div class="min-h-0 flex-1 overflow-auto">
  <table class="w-full border-separate border-spacing-0 text-sm">
    <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
      <tr>
        <th class="w-10 border-b border-line py-2 pl-4 pr-1">
          <input
            type="checkbox"
            class="accent-input"
            aria-label="Select all tunnels shown"
            checked={allChecked}
            indeterminate={someChecked && !allChecked}
            onchange={(e) => tunnels.setAllChecked(rows.map((r) => r.id), e.currentTarget.checked)}
          />
        </th>
        <th class="border-b border-line px-3 py-2">Name</th>
        <th class="border-b border-line px-3 py-2">Type</th>
        <th class="border-b border-line px-3 py-2">Port mapping</th>
        {#if !compact}<th class="border-b border-line px-3 py-2">Host</th>{/if}
        <th class="border-b border-line px-3 py-2">Status</th>
        {#if !compact}<th class="border-b border-line px-3 py-2">Uptime</th>{/if}
        <th class="w-24 border-b border-line py-2 pl-2 pr-4 text-right"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row, i (row.id)}
        {@const m = mapping(row.rule, row.boundPort)}
        {@const a = tunnelActions(row)}
        {@const selected = tunnels.selectedId === row.id}
        <tr
          data-tunnel-row={row.id}
          tabindex="0"
          aria-selected={selected}
          class="group cursor-pointer outline-none transition-colors focus-visible:bg-accent/10 {selected ? 'bg-accent/10' : 'hover:bg-panel-hover'}"
          onclick={() => pick(row)}
          onkeydown={(e) => key(e, i)}
          oncontextmenu={(e) => {
            e.preventDefault();
            tunnels.select(row.id);
            openMenu(e, row);
          }}
        >
          <td class="relative border-b border-line/60 py-2.5 pl-4 pr-1">
            {#if selected}<span class="absolute inset-y-0 left-0 w-0.5 bg-accent" aria-hidden="true"></span>{/if}
            <input type="checkbox" class="accent-input" aria-label="Select {row.rule.label}" checked={tunnels.checked.has(row.id)} onclick={(e) => e.stopPropagation()} onchange={() => tunnels.toggleChecked(row.id)} />
          </td>
          <td class="max-w-[16rem] border-b border-line/60 px-3 py-2.5">
            <div class="truncate font-medium">{row.rule.label}</div>
            {#if row.rule.description}<div class="truncate text-xs text-fg-muted">{row.rule.description}</div>{/if}
          </td>
          <td class="border-b border-line/60 px-3 py-2.5 text-fg-muted">{KIND_TEXT[row.rule.kind]}</td>
          <td class="border-b border-line/60 px-3 py-2.5 font-mono text-xs tabular-nums">
            <span class="whitespace-nowrap">{m.from}</span> <span class="text-fg-muted" aria-label="to">→</span> <span class="whitespace-nowrap">{m.to}</span>
          </td>
          {#if !compact}
            <td class="max-w-[10rem] truncate border-b border-line/60 px-3 py-2.5 {row.host ? '' : 'text-danger'}" title={row.host?.hostname}>{row.hostLabel}</td>
          {/if}
          <td class="border-b border-line/60 px-3 py-2.5">
            <TunnelStatusBadge status={row.status} title={row.error ?? undefined} />
          </td>
          {#if !compact}
            <td class="border-b border-line/60 px-3 py-2.5 text-xs tabular-nums text-fg-muted">{uptime(row)}</td>
          {/if}
          <td class="border-b border-line/60 py-2.5 pl-2 pr-3 text-right">
            <div class="inline-flex items-center gap-0.5">
              {#if row.status === "active" || row.status === "connecting"}
                <button class="icon-btn h-8 w-8" title={a.canStop ? "Stop" : a.stopWhy} aria-label="Stop {row.rule.label}" disabled={!a.canStop} onclick={(e) => { e.stopPropagation(); a.stop(); }}><Square size={15} /></button>
              {:else}
                <button class="icon-btn h-8 w-8" title={a.canStart ? "Start" : a.startWhy} aria-label="Start {row.rule.label}" disabled={!a.canStart} onclick={(e) => { e.stopPropagation(); a.start(); }}><Play size={15} /></button>
              {/if}
              <button class="icon-btn h-8 w-8" title="More actions" aria-label="More actions for {row.rule.label}" aria-haspopup="menu" onclick={(e) => { e.stopPropagation(); openMenu(e, row); }}><Ellipsis size={16} /></button>
            </div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if menu}
  <TunnelActionsMenu row={menu.row} x={menu.x} y={menu.y} onclose={() => (menu = null)} />
{/if}
