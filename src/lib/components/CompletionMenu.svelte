<script lang="ts">
  import { MENU_HEADER, MENU_PAD, MENU_ROW, type MenuGroup, type MenuItem, type Placement } from "$lib/completion/menu";

  let {
    groups,
    selected,
    place,
    fontFamily,
    onpick,
  }: {
    groups: MenuGroup[];
    /** Index into the items of all groups, in order. */
    selected: number;
    place: Placement;
    fontFamily: string;
    onpick: (index: number) => void;
  } = $props();

  /** The groups with each item's place in the whole list. */
  const numbered = $derived.by(() => {
    let n = 0;
    return groups.map((g) => ({ title: g.title, items: g.items.map((item) => ({ item, index: n++ })) }));
  });

  let list: HTMLDivElement | undefined;
  // Keep the chosen row in view when the list is shorter than its content.
  $effect(() => {
    list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  });

  /** The label cut into runs, the matched characters marked. */
  function runs(text: string, hit: number[]): { text: string; hit: boolean }[] {
    if (hit.length === 0) return [{ text, hit: false }];
    const on = new Set(hit);
    const out: { text: string; hit: boolean }[] = [];
    let i = 0;
    for (const ch of text) {
      const h = on.has(i);
      const last = out[out.length - 1];
      if (last && last.hit === h) last.text += ch;
      else out.push({ text: ch, hit: h });
      i += ch.length;
    }
    return out;
  }
</script>

{#snippet marked(item: MenuItem, which: "label" | "detail")}
  {#each runs(which === "label" ? item.label : item.detail, which === "label" ? item.labelHit : item.detailHit) as r, i (i)}
    {#if r.hit}<mark class="bg-transparent font-semibold text-accent underline decoration-accent/50 underline-offset-2">{r.text}</mark>{:else}{r.text}{/if}
  {/each}
{/snippet}

<!-- The terminal keeps the keyboard, so the rows are not focusable; choosing is by the keys the shortcut
     handler routes here, or by a click, which must not take focus away from the terminal. -->
<div
  bind:this={list}
  class="pointer-events-auto absolute z-20 overflow-y-auto rounded-md border border-line bg-panel text-[12px] text-fg shadow-lg"
  data-testid="completion-menu"
  role="listbox"
  aria-label="Suggestions"
  tabindex="-1"
  style:left="{place.left}px"
  style:top="{place.top}px"
  style:width="{place.width}px"
  style:max-height="{place.height}px"
  style:padding="{MENU_PAD}px"
  style:font-family={fontFamily}
  onmousedown={(e) => e.preventDefault()}
>
  {#each numbered as g (g.title)}
    <div class="flex items-center px-2 text-[10px] font-semibold uppercase tracking-wide text-fg-muted" style:height="{MENU_HEADER}px" role="presentation">{g.title}</div>
    {#each g.items as { item, index } (item.id)}
      <div
        class="flex cursor-pointer items-baseline gap-2 truncate rounded px-2 {index === selected ? 'bg-accent/20' : 'hover:bg-panel-hover'}"
        style:height="{MENU_ROW}px"
        style:line-height="{MENU_ROW}px"
        role="option"
        tabindex="-1"
        aria-selected={index === selected}
        data-index={index}
        data-kind={item.kind}
        onclick={() => onpick(index)}
        onkeydown={() => {}}
      >
        <span class="shrink-0 truncate whitespace-pre" style:max-width="62%">{@render marked(item, "label")}</span>
        {#if item.detail}<span class="min-w-0 truncate text-fg-muted">{@render marked(item, "detail")}</span>{/if}
      </div>
    {/each}
  {/each}
</div>
