<script lang="ts">
  import type { Snippet as SvelteSnippet } from "svelte";

  let {
    tone = "neutral",
    title = undefined,
    outline = false,
    children,
  }: {
    tone?: "neutral" | "accent" | "danger" | "warning" | "success";
    title?: string;
    /** A hairline in the badge's own colour, for a pill that has to stand out from its neighbours (a tab's environment). */
    outline?: boolean;
    children: SvelteSnippet;
  } = $props();
</script>

<span class="badge badge-{tone}" class:badge-outline={outline} {title}>{@render children()}</span>
