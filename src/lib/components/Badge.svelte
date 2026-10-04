<script lang="ts">
  import type { Snippet as SvelteSnippet } from "svelte";

  let {
    tone = "neutral",
    title = undefined,
    children,
  }: {
    tone?: "neutral" | "accent" | "danger" | "warning" | "success";
    title?: string;
    children: SvelteSnippet;
  } = $props();
</script>

<span class="badge badge-{tone}" {title}>{@render children()}</span>
