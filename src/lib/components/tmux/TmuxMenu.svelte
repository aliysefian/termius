<script lang="ts">
  import type { Server } from "lucide-svelte";
  import { keepInView } from "$lib/actions";

  export interface MenuItem {
    id: string;
    label: string;
    icon?: typeof Server;
    danger?: boolean;
    disabled?: boolean;
    why?: string;
    /** A line is drawn above this item. */
    group?: string;
  }

  let { items, x, y, label, onpick, onclose }: { items: MenuItem[]; x: number; y: number; label: string; onpick: (id: string) => void; onclose: () => void } = $props();

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
    const list = [...(menu?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
    const at = list.indexOf(document.activeElement as HTMLElement);
    list[(at + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length]?.focus();
  }
</script>

<button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}></button>
<div bind:this={menu} class="fixed z-50 w-56 rounded-xl border border-line bg-panel p-1 shadow-xl" use:keepInView={{ x, y }} role="menu" aria-label={label} tabindex="-1" onkeydown={key}>
  {#each items as item, i (item.id)}
    {#if i > 0 && items[i - 1].group !== item.group}<div class="my-1 h-px bg-line" role="separator"></div>{/if}
    <button
      class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none disabled:opacity-40 disabled:hover:bg-transparent {item.danger ? 'text-danger' : ''}"
      role="menuitem"
      disabled={item.disabled}
      title={item.why}
      onclick={() => {
        onclose();
        onpick(item.id);
      }}
    >
      {#if item.icon}<item.icon size={15} />{:else}<span class="w-[15px]"></span>{/if}
      {item.label}
    </button>
  {/each}
</div>
