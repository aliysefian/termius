<script lang="ts">
  import { X } from "lucide-svelte";
  import type { Snippet as SvelteSnippet } from "svelte";

  let {
    title,
    onclose,
    children,
    footer,
    width = "max-w-lg",
  }: {
    title: string;
    onclose: () => void;
    children: SvelteSnippet;
    footer?: SvelteSnippet;
    width?: string;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && onclose()}
>
  <div
    class="flex w-full {width} max-h-[90vh] flex-col rounded-xl border border-line bg-panel shadow-2xl"
    role="dialog"
    aria-modal="true"
    aria-label={title}
  >
    <header class="flex items-center justify-between border-b border-line px-5 py-3">
      <h2 class="text-sm font-semibold">{title}</h2>
      <button class="icon-btn" onclick={onclose} aria-label="Close"><X size={16} /></button>
    </header>
    <div class="flex-1 overflow-y-auto px-5 py-4">
      {@render children()}
    </div>
    {#if footer}
      <footer class="flex justify-end gap-2 border-t border-line px-5 py-3">
        {@render footer()}
      </footer>
    {/if}
  </div>
</div>
