<script lang="ts">
  import { Container, Ship } from "lucide-svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { t as tr } from "$lib/i18n/index.svelte";

  let { current }: { current: "containers" | "kubernetes" } = $props();
  const TABS = [
    { view: "containers", label: "tabs.containers", icon: Container },
    { view: "kubernetes", label: "tabs.kubernetes", icon: Ship },
  ] as const;
</script>

<!-- Containers and Kubernetes are two pages of one rail entry; this is the way between them. -->
<div class="flex gap-1 border-b border-line bg-panel px-4 pt-2" role="tablist" aria-label="Containers or Kubernetes">
  {#each TABS as tab (tab.view)}
    <button
      role="tab"
      aria-selected={current === tab.view}
      class="-mb-px flex items-center gap-1.5 rounded-t-md border border-b-0 px-3 py-1.5 text-xs font-medium transition-colors {current === tab.view ? 'border-line bg-base text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
      onclick={() => (ui.view = tab.view)}
    >
      <tab.icon size={13} class={current === tab.view ? "text-accent" : ""} />
      {tr(tab.label)}
    </button>
  {/each}
</div>
