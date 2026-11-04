<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { ChevronDown } from "lucide-svelte";

  let {
    label,
    ariaLabel,
    title = "",
    active = false,
    align = "left",
    width = 260,
    icon,
    children,
    onopen,
    class: cls = "",
  }: {
    /** Text on the button. */
    label?: string;
    ariaLabel: string;
    title?: string;
    /** Marks the button as holding a choice (a filter that is set). */
    active?: boolean;
    align?: "left" | "right";
    width?: number;
    icon?: Snippet;
    children: Snippet<[() => void]>;
    onopen?: () => void;
    class?: string;
  } = $props();

  let open = $state(false);
  let button = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();
  let pos = $state<{ left: number; top: number; width: number; maxHeight: number } | null>(null);

  function place() {
    if (!button) return;
    const r = button.getBoundingClientRect();
    const w = Math.min(width, window.innerWidth - 16);
    const left = align === "right" ? r.right - w : r.left;
    pos = { left: Math.max(8, Math.min(left, window.innerWidth - w - 8)), top: r.bottom + 6, width: w, maxHeight: Math.max(160, window.innerHeight - r.bottom - 16) };
  }

  async function toggle() {
    open = !open;
    if (open) {
      onopen?.();
      await tick();
      place();
      await tick();
      place();
    }
  }
  const close = () => {
    open = false;
    button?.focus();
  };

  $effect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!panel?.contains(t) && !button?.contains(t)) open = false;
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", esc, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", esc, true);
      window.removeEventListener("resize", place);
    };
  });
</script>

<button
  bind:this={button}
  class="inline-flex h-8 items-center gap-1 rounded-md border px-2 text-xs transition-colors hover:bg-panel-hover focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent {active ? 'border-accent/60 bg-accent/10 text-accent' : 'border-line text-fg-muted hover:text-fg'} {cls}"
  aria-haspopup="dialog"
  aria-expanded={open}
  aria-label={ariaLabel}
  {title}
  onclick={() => void toggle()}
>
  {@render icon?.()}
  {#if label}<span class="max-w-28 truncate">{label}</span>{/if}
  <ChevronDown size={11} class="opacity-60" />
</button>

{#if open && pos}
  <div bind:this={panel} class="fixed z-50 overflow-y-auto rounded-lg border border-line bg-panel p-2 text-xs shadow-2xl" style="left: {pos.left}px; top: {pos.top}px; width: {pos.width}px; max-height: {pos.maxHeight}px" role="dialog" aria-label={ariaLabel}>
    {@render children(close)}
  </div>
{/if}
