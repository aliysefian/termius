<script module lang="ts">
  /** The usual guard for forms: `confirmClose={dirty ? DISCARD : null}`. */
  export const DISCARD = { title: "Unsaved changes", message: "Discard your changes?", confirm: "Discard" };
</script>

<script lang="ts">
  import { X } from "lucide-svelte";
  import { onMount, type Snippet as SvelteSnippet } from "svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { focusables, pushLayer, trapTab } from "$lib/layers";

  let {
    title,
    onclose,
    children,
    footer,
    width = "max-w-lg",
    confirmClose = null,
  }: {
    title: string;
    onclose: () => void;
    children: SvelteSnippet;
    footer?: SvelteSnippet;
    width?: string;
    /** When set, Escape, the backdrop and the X ask this before closing. */
    confirmClose?: { title?: string; message: string; confirm?: string } | null;
  } = $props();

  const titleId = `modal-title-${Math.random().toString(36).slice(2, 8)}`;
  let dialog = $state<HTMLDivElement>();
  let asking = false;

  async function requestClose() {
    if (asking) return;
    if (confirmClose) {
      asking = true;
      try {
        const ok = await ask(confirmClose.message, { title: confirmClose.title ?? "Please confirm", confirm: confirmClose.confirm ?? "Close", danger: true });
        if (!ok) return;
      } finally {
        asking = false;
      }
    }
    onclose();
  }

  // Focus goes to the first field on open and back to the opener on close.
  onMount(() => {
    const opener = document.activeElement as HTMLElement | null;
    const unlayer = pushLayer(() => void requestClose());
    queueMicrotask(() => {
      if (!dialog || dialog.contains(document.activeElement)) return;
      const items = focusables(dialog).filter((el) => !el.closest("header"));
      (items[0] ?? dialog).focus();
    });
    return () => {
      unlayer();
      if (opener && document.contains(opener)) opener.focus();
    };
  });
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && void requestClose()}
>
  <div
    bind:this={dialog}
    class="flex w-full {width} max-h-[90vh] flex-col rounded-xl border border-line bg-panel shadow-2xl outline-none"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    onkeydown={(e) => dialog && trapTab(e, dialog)}
  >
    <header class="flex items-center justify-between border-b border-line px-5 py-3">
      <h2 id={titleId} class="text-sm font-semibold">{title}</h2>
      <button class="icon-btn" onclick={() => void requestClose()} aria-label="Close"><X size={16} /></button>
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
