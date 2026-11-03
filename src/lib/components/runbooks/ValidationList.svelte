<script lang="ts">
  import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-svelte";
  import type { ValidationItem } from "$lib/runbookrun";

  let { items, onstep }: { items: ValidationItem[]; onstep?: (step: number) => void } = $props();
  const icon = { ok: CircleCheck, warn: TriangleAlert, error: CircleAlert, info: Info } as const;
  const tone = { ok: "text-success", warn: "text-warning", error: "text-danger", info: "text-fg-muted" } as const;
  const word = { ok: "Passed", warn: "Warning", error: "Error", info: "Note" } as const;
</script>

<ul class="space-y-1" aria-label="Validation results" data-testid="validation">
  {#each items as it, i (i + it.text)}
    {@const Icon = icon[it.level]}
    <li class="flex items-start gap-2 text-xs">
      <Icon size={13} class="mt-0.5 shrink-0 {tone[it.level]}" aria-hidden="true" />
      <span class="sr-only">{word[it.level]}:</span>
      {#if it.step !== undefined && it.step !== null && onstep}
        <button class="text-left hover:underline" onclick={() => onstep(it.step!)}>{it.text}</button>
      {:else}
        <span class="min-w-0 break-words">{it.text}</span>
      {/if}
    </li>
  {/each}
</ul>
