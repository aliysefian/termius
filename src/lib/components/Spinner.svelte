<script lang="ts">
  import { Loader2 } from "lucide-svelte";

  /** One spinner, so every loading state in the app looks the same. */
  let { size = 14, label = undefined }: { size?: number; label?: string } = $props();
</script>

<span class="inline-flex items-center gap-1.5 text-fg-muted" role="status">
  <Loader2 {size} class="animate-spin" />
  {#if label}<span class="text-xs">{label}</span>{/if}
</span>
