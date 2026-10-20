<script lang="ts">
  // Two versions of a file, line by line: split (left against right) or unified. The engine is the one the file
  // transfer's overwrite dialog uses (src/lib/diff.ts).
  import { diffLines, hunks, splitRows, type DiffLine } from "$lib/diff";

  let { before, after, layout = "split", leftLabel, rightLabel }: { before: string; after: string; layout?: "split" | "unified"; leftLabel: string; rightLabel: string } = $props();

  const diff = $derived(diffLines(before, after));
  const blocks = $derived(hunks(diff.lines, 3));
  const rows = $derived(splitRows(diff.lines));
  const same = $derived(diff.added === 0 && diff.removed === 0);

  const tone = (k: string) => (k === "add" ? "bg-success/10 text-success" : k === "del" ? "bg-danger/10 text-danger" : "text-fg-muted");
  const mark = (k: string) => (k === "add" ? "+" : k === "del" ? "−" : " ");
  const cell = (l: DiffLine | undefined) => (l ? tone(l.kind) : "bg-panel/30");
</script>

{#if same}
  <p class="flex h-full items-center justify-center text-xs text-fg-muted">The two are identical.</p>
{:else}
  <div class="h-full overflow-auto font-mono text-xs tabular-nums" data-testid="config-diff">
    {#if layout === "split"}
      <div class="sticky top-0 z-10 grid grid-cols-2 border-b border-line bg-panel text-[11px] text-fg-muted"><div class="px-3 py-1">{leftLabel}</div><div class="border-l border-line px-3 py-1">{rightLabel}</div></div>
      {#each rows as r, i (i)}
        <div class="grid grid-cols-2">
          <div class="flex min-w-0 {cell(r.left)}"><span class="w-10 shrink-0 select-none px-2 text-right opacity-60">{r.left?.oldNo ?? ""}</span><span class="whitespace-pre-wrap break-all pr-2">{r.left?.text ?? ""}</span></div>
          <div class="flex min-w-0 border-l border-line {cell(r.right)}"><span class="w-10 shrink-0 select-none px-2 text-right opacity-60">{r.right?.newNo ?? ""}</span><span class="whitespace-pre-wrap break-all pr-2">{r.right?.text ?? ""}</span></div>
        </div>
      {/each}
    {:else}
      {#each blocks as b, bi (bi)}
        {#if b.type === "fold"}
          <div class="border-y border-line/60 bg-panel/40 px-3 py-0.5 text-center text-[11px] text-fg-muted">{b.count} unchanged {b.count === 1 ? "line" : "lines"}</div>
        {:else}
          {#each b.lines as l, li (li)}
            <div class="flex {tone(l.kind)}">
              <span class="w-10 shrink-0 select-none px-2 text-right opacity-60">{l.oldNo ?? ""}</span>
              <span class="w-10 shrink-0 select-none px-2 text-right opacity-60">{l.newNo ?? ""}</span>
              <span class="w-4 shrink-0 select-none text-center">{mark(l.kind)}</span>
              <span class="whitespace-pre-wrap break-all pr-2">{l.text}</span>
            </div>
          {/each}
        {/if}
      {/each}
    {/if}
  </div>
{/if}
