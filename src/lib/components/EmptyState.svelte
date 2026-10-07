<script lang="ts">
  import type { Snippet as SvelteSnippet } from "svelte";
  import type { Server } from "lucide-svelte";
  import Illustration, { type Scene } from "./Illustration.svelte";

  /**
   * One look for every "nothing here yet" panel: an icon, a line of text,
   * and an optional action. Compact drops the icon and padding for use
   * inside a list that's merely filtered down to nothing.
   */
  let {
    icon = undefined,
    art = undefined,
    text,
    compact = false,
    children,
  }: {
    icon?: typeof Server;
    /** A small illustration, used instead of the icon. */
    art?: Scene;
    text: string;
    compact?: boolean;
    children?: SvelteSnippet;
  } = $props();
</script>

<div class="anim-rise text-center {compact ? 'px-2 py-6' : 'px-3 py-8'}">
  {#if art && !compact}
    <div class="mb-2"><Illustration scene={art} size={120} /></div>
  {:else if icon && !compact}
    {@const Icon = icon}
    <Icon size={28} class="mx-auto mb-3 text-fg-muted/50" />
  {/if}
  <p class="text-sm text-fg-muted {compact ? 'text-xs' : ''}">{text}</p>
  {#if children}
    <div class="mt-4">{@render children()}</div>
  {/if}
</div>
