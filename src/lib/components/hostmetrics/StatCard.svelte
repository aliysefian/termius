<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Frame } from "$lib/hostframes";
  import Spark from "./Spark.svelte";

  let {
    title,
    value,
    sub = "",
    frames = [],
    pick,
    max = 100,
    now,
    windowMs,
    slot = 1,
    icon,
    onopen,
    children,
  }: {
    title: string;
    /** The big figure, already written ("24.8%", "Unavailable"). */
    value: string;
    sub?: string;
    frames?: Frame[];
    pick?: (f: Frame) => number | null;
    /** The value at the top of the sparkline; 0 is the bottom. */
    max?: number;
    now: number;
    windowMs: number;
    slot?: 1 | 2 | 3 | 4;
    icon?: Snippet;
    onopen?: () => void;
    children?: Snippet;
  } = $props();

</script>

{#snippet body()}
  <div class="flex items-center gap-2 text-sm font-semibold text-fg">
    {#if icon}<span class="text-accent" aria-hidden="true">{@render icon()}</span>{/if}
    {title}
  </div>
  <div class="text-2xl font-semibold tabular-nums leading-tight">{value}</div>
  {#if children}{@render children()}{/if}
  {#if sub}<div class="truncate text-xs text-fg-muted">{sub}</div>{/if}
  {#if pick}<Spark {frames} {pick} {max} {now} {windowMs} {slot} />{/if}
{/snippet}

{#if onopen}
  <button type="button" class="card flex h-full w-full min-w-0 flex-col gap-1 overflow-hidden rounded-lg border border-line bg-panel p-3 text-left hover:bg-panel-hover" onclick={onopen}>{@render body()}</button>
{:else}
  <div class="card flex h-full w-full min-w-0 flex-col gap-1 overflow-hidden rounded-lg border border-line bg-panel p-3 text-left">{@render body()}</div>
{/if}

