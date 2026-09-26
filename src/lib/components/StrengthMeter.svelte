<script lang="ts">
  import { estimate } from "$lib/strength";
  let { password }: { password: string } = $props();
  const s = $derived(estimate(password));
  const colors = ["bg-danger", "bg-danger", "bg-warning", "bg-success", "bg-success"];
</script>

{#if password}
  <div class="mt-1.5" aria-live="polite">
    <div class="flex gap-1">
      {#each [0, 1, 2, 3] as i (i)}
        <div class="h-1 flex-1 rounded {i < Math.max(1, s.score) ? colors[s.score] : 'bg-line'}"></div>
      {/each}
    </div>
    <p class="mt-1 text-xs {s.score < 2 ? 'text-danger' : 'text-fg-muted'}">
      {s.label}{s.score < 2 ? ". Use a longer passphrase, for example four random words." : ""}
    </p>
  </div>
{/if}
