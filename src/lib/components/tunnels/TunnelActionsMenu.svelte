<script lang="ts">
  import { Copy, FileText, Pencil, Play, RotateCw, SquareTerminal, Square, Trash2 } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import { openTunnelTerminal, tunnelActions } from "./actions";
  import type { TunnelRow } from "$lib/tunnels";

  let { row, x, y, onclose }: { row: TunnelRow; x: number; y: number; onclose: () => void } = $props();

  const a = $derived(tunnelActions(row));
  let menu = $state<HTMLDivElement>();

  $effect(() => {
    menu?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
  });

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(menu?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  }

  const item = "flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none disabled:opacity-40 disabled:hover:bg-transparent";
  const run = (f: () => void) => () => {
    onclose();
    f();
  };
</script>

<button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}></button>
<div bind:this={menu} class="fixed z-50 w-56 rounded-xl border border-line bg-panel p-1 shadow-xl" use:keepInView={{ x, y }} role="menu" aria-label="Actions for {row.rule.label}" tabindex="-1" onkeydown={key}>
  <button class={item} role="menuitem" disabled={!a.canStart} title={a.startWhy} onclick={run(a.start)}><Play size={15} /> Start</button>
  <button class={item} role="menuitem" disabled={!a.canStop} title={a.stopWhy} onclick={run(a.stop)}><Square size={15} /> Stop</button>
  <button class={item} role="menuitem" disabled={!a.canRestart} title={a.restartWhy} onclick={run(a.restart)}><RotateCw size={15} /> Restart</button>
  <div class="my-1 h-px bg-line" role="separator"></div>
  <button class={item} role="menuitem" onclick={run(a.edit)}><Pencil size={15} /> Edit</button>
  <button class={item} role="menuitem" onclick={run(a.duplicate)}><Copy size={15} /> Duplicate</button>
  <button class={item} role="menuitem" disabled={!row.host} title={row.host ? "" : "Its host no longer exists"} onclick={run(() => openTunnelTerminal(row))}><SquareTerminal size={15} /> Open SSH Terminal</button>
  <button class={item} role="menuitem" onclick={run(a.logs)}><FileText size={15} /> View Logs</button>
  <div class="my-1 h-px bg-line" role="separator"></div>
  <button class="{item} text-danger" role="menuitem" onclick={run(a.remove)}><Trash2 size={15} /> Delete</button>
</div>
