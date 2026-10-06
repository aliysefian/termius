<script lang="ts">
  import { charWidth, type Ghost } from "$lib/completion/ghost";

  let {
    ghost,
    fontFamily,
    fontSize,
    color,
  }: {
    ghost: Ghost | null;
    fontFamily: string;
    fontSize: number;
    color: string;
  } = $props();
</script>

<!-- Nothing here takes the mouse or the keyboard: the faint text is only drawn. Each character gets
     its own cell: the terminal rounds its cells to whole pixels, so the font's natural advance would
     drift away from the line it continues. -->
{#if ghost}
  <div
    class="pointer-events-none absolute z-[5] select-none overflow-hidden whitespace-pre"
    data-testid="completion-ghost"
    aria-hidden="true"
    style:left="{ghost.left}px"
    style:top="{ghost.top}px"
    style:height="{ghost.cellHeight}px"
    style:width="{ghost.cellWidth * ghost.cellsWide}px"
    style:font-family={fontFamily}
    style:font-size="{fontSize}px"
    style:line-height="{ghost.cellHeight}px"
    style:color
    style:opacity="0.5"
  >{#each [...ghost.text] as ch, i (i)}<span class="inline-block" style:width="{ghost.cellWidth * charWidth(ch.codePointAt(0) ?? 0)}px">{ch}</span>{/each}</div>
{/if}
