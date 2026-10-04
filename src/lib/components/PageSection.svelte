<script lang="ts">
  import type { Snippet as SvelteSnippet } from "svelte";
  import type { Server } from "lucide-svelte";

  /** One card shell for a full-page view's sections (Settings, Keys, Vault, Groups). */
  let {
    title,
    icon = undefined,
    description = undefined,
    action,
    tone = "line",
    children,
  }: {
    title: string;
    icon?: typeof Server;
    description?: string;
    /** Rendered at the top right of the header, next to the title. */
    action?: SvelteSnippet;
    tone?: "line" | "warning";
    children: SvelteSnippet;
  } = $props();
</script>

<section class="rounded-xl border {tone === 'warning' ? 'border-warning/40' : 'border-line'} bg-panel p-5">
  <div class="mb-1 flex items-start justify-between gap-3">
    <h2 class="flex items-center gap-2 text-sm font-semibold {tone === 'warning' ? 'text-warning' : ''}">
      {#if icon}
        {@const Icon = icon}
        <Icon size={15} class={tone === "warning" ? "" : "text-accent"} />
      {/if}
      {title}
    </h2>
    {#if action}{@render action()}{/if}
  </div>
  {#if description}
    <p class="mb-4 text-xs text-fg-muted">{description}</p>
  {/if}
  {@render children()}
</section>
