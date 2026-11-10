<script lang="ts">
  // A snapshot of a pane's text, drawn as text nodes (never as HTML). It is not a terminal: it takes no input.
  import { parseAnsi, type Span } from "$lib/tmux/ansi";

  let {
    text,
    ansi = true,
    maxLines = 2000,
    follow = true,
    highlight = null,
    compact = false,
    ariaLabel,
  }: { text: string; ansi?: boolean; maxLines?: number; follow?: boolean; highlight?: { text: string; caseSensitive: boolean; regex: boolean } | null; compact?: boolean; ariaLabel: string } = $props();

  const lines = $derived<Span[][]>(ansi ? parseAnsi(text, maxLines) : text.split("\n").slice(-maxLines).map((t) => [{ text: t.replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "") }]));

  const matcher = $derived.by(() => {
    if (!highlight?.text) return null;
    try {
      return new RegExp(highlight.regex ? highlight.text : highlight.text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), highlight.caseSensitive ? "g" : "gi");
    } catch {
      return null;
    }
  });

  /** A span's text cut into [text, isMatch] runs. */
  function runs(t: string): [string, boolean][] {
    if (!matcher) return [[t, false]];
    const out: [string, boolean][] = [];
    let last = 0;
    for (const m of t.matchAll(matcher)) {
      if (!m[0]) continue;
      if (m.index > last) out.push([t.slice(last, m.index), false]);
      out.push([m[0], true]);
      last = m.index + m[0].length;
    }
    if (last < t.length) out.push([t.slice(last), false]);
    return out.length ? out : [[t, false]];
  }

  const style = (s: Span) =>
    [s.fg ? `color:${s.fg}` : "", s.bg ? `background:${s.bg}` : "", s.bold ? "font-weight:600" : "", s.dim ? "opacity:.65" : "", s.italic ? "font-style:italic" : "", s.underline || s.strike ? `text-decoration:${[s.underline ? "underline" : "", s.strike ? "line-through" : ""].filter(Boolean).join(" ")}` : ""]
      .filter(Boolean)
      .join(";");

  let box = $state<HTMLDivElement>();
  let pinned = true;

  function onscroll() {
    if (!box) return;
    pinned = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
  }

  // New text keeps the view at the bottom, like a terminal, unless the person scrolled up to read.
  $effect(() => {
    void lines;
    if (follow && pinned && box) queueMicrotask(() => box && (box.scrollTop = box.scrollHeight));
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={box}
  class="h-full w-full overflow-auto bg-[#0b0f1a] font-mono {compact ? 'p-1.5 text-[10px] leading-[1.25]' : 'p-3 text-[12.5px] leading-[1.35]'} text-[#d7dae0]"
  style="--ansi-bg:#0b0f1a;--ansi-fg:#d7dae0"
  role="region"
  aria-label={ariaLabel}
  tabindex="0"
  {onscroll}
>
  <pre class="m-0 min-w-max whitespace-pre font-[inherit]">{#each lines as line, i (i)}<div class="min-h-[1em]">{#each line as s, j (j)}<span style={style(s)}>{#each runs(s.text) as [t, hit], k (k)}{#if hit}<mark class="rounded-[2px] bg-warning/60 text-inherit">{t}</mark>{:else}{t}{/if}{/each}</span>{/each}</div>{/each}</pre>
</div>
