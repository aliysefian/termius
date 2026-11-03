<script lang="ts">
  import { tick } from "svelte";
  import { ArrowDownToLine, ChevronRight, Loader2, ScrollText } from "lucide-svelte";
  import LogDetails from "./LogDetails.svelte";
  import { buildOffsets, charsFor, visibleRows, ROW_H, WRAP_LINE_H, type LayoutOptions } from "$lib/ops/loglayout";
  import { PRIORITY_BADGE, type Row, type Sev } from "$lib/ops/logrows";
  import { retryText } from "$lib/ops/logstatus";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  let { onfocussearch }: { onfocussearch?: () => void } = $props();

  const DETAIL_H = 330;
  let scroller = $state<HTMLDivElement>();
  let top = $state(0);
  let viewH = $state(400);
  let viewW = $state(900);

  const rows = $derived(opsLogs.shown);
  const cols = $derived(settings.prefs.opsLogColumns);
  const show = $derived({ time: cols.includes("time"), host: cols.includes("host"), severity: cols.includes("severity"), unit: cols.includes("unit") });

  const W = { expand: 28, time: 104, host: 116, sev: 66, unit: 150 };
  const fixed = $derived(W.expand + (show.time ? W.time : 0) + (show.host ? W.host : 0) + (show.severity ? W.sev : 0) + (show.unit ? W.unit : 0));
  const template = $derived([`${W.expand}px`, show.time ? `${W.time}px` : "", show.host ? `${W.host}px` : "", show.severity ? `${W.sev}px` : "", show.unit ? `${W.unit}px` : "", "minmax(0,1fr)"].filter(Boolean).join(" "));
  const wrap = $derived(settings.prefs.opsLogWrap);

  const opts = $derived<LayoutOptions>({ wrap, charsPerLine: charsFor(Math.max(0, viewW - fixed - 24)), open: opsLogs.expanded, detailH: DETAIL_H });
  const offsets = $derived(buildOffsets(rows, opts));
  const total = $derived(offsets[offsets.length - 1] ?? 0);
  const range = $derived(visibleRows(offsets, top, viewH));
  const visible = $derived(rows.slice(range.start, range.end).map((r, k) => ({ r, i: range.start + k })));

  // Following: new events scroll into view until the person scrolls up; scrolling back to the end resumes.
  $effect(() => {
    void total;
    if (opsLogs.follow && !opsLogs.paused && scroller) void tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  function onscroll(e: Event) {
    const el = e.currentTarget as HTMLDivElement;
    top = el.scrollTop;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < ROW_H * 1.5;
    if (!opsLogs.paused) opsLogs.follow = atEnd;
  }

  const hostHue = (id: string) => {
    const i = Math.max(0, opsLogs.hosts.indexOf(id));
    return `hsl(${(i * 67 + 250) % 360} 70% 68%)`;
  };

  const clock = (ms: number) => {
    const d = new Date(ms);
    const p = (n: number, w = 2) => String(n).padStart(w, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  };
  const sevBadge: Record<Sev, string> = { error: "bg-danger/15 text-danger", warn: "bg-warning/15 text-warning", info: "bg-accent/10 text-fg-muted", debug: "bg-fg-muted/10 text-fg-muted" };
  const sevRow: Record<Sev, string> = { error: "border-l-danger bg-danger/[0.04]", warn: "border-l-warning bg-warning/[0.04]", info: "border-l-transparent", debug: "border-l-transparent text-fg-muted" };
  const badge = (r: Row) => (r.priority === null ? ({ error: "ERROR", warn: "WARN", info: "INFO", debug: "DEBUG" } as const)[r.sev] : PRIORITY_BADGE[r.priority]);

  /** A unit without its usual suffix, to save room (the full name is in the tooltip and the details). */
  const unitShort = (u: string) => u.replace(/\.service$/, "");

  const compiled = $derived(opsLogs.compiled);
  function parts(text: string): { text: string; hit: boolean }[] {
    const spans = compiled.spans(text);
    if (!spans.length) return [{ text, hit: false }];
    const out: { text: string; hit: boolean }[] = [];
    let at = 0;
    for (const [a, b] of spans) {
      if (a > at) out.push({ text: text.slice(at, a), hit: false });
      out.push({ text: text.slice(a, b), hit: true });
      at = b;
    }
    if (at < text.length) out.push({ text: text.slice(at), hit: false });
    return out;
  }

  // -- keyboard ---------------------------------------------------------------------------------------------------
  const selIndex = $derived(rows.findIndex((r) => r.id === opsLogs.selected));

  function reveal(i: number) {
    if (!scroller || i < 0) return;
    const y = offsets[i];
    const h = offsets[i + 1] - y;
    if (y < scroller.scrollTop) scroller.scrollTop = y;
    else if (y + h > scroller.scrollTop + viewH) scroller.scrollTop = y + h - viewH;
  }

  function select(i: number) {
    const r = rows[Math.max(0, Math.min(rows.length - 1, i))];
    if (!r) return;
    opsLogs.selected = r.id;
    if (!opsLogs.paused) opsLogs.follow = false;
    void tick().then(() => reveal(rows.indexOf(r)));
  }

  function key(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    // Typing somewhere inside (the details' text) isn't a command.
    if (target.closest("pre, input, textarea, select, details, button") && e.key !== "Escape") return;
    const page = Math.max(1, Math.floor(viewH / ROW_H) - 1);
    switch (e.key) {
      case "ArrowDown": e.preventDefault(); select(selIndex < 0 ? 0 : selIndex + 1); break;
      case "ArrowUp": e.preventDefault(); select(selIndex < 0 ? rows.length - 1 : selIndex - 1); break;
      case "PageDown": e.preventDefault(); select((selIndex < 0 ? 0 : selIndex) + page); break;
      case "PageUp": e.preventDefault(); select((selIndex < 0 ? 0 : selIndex) - page); break;
      case "Home": e.preventDefault(); select(0); break;
      case "End": e.preventDefault(); select(rows.length - 1); opsLogs.follow = !opsLogs.paused; break;
      case "Enter": if (rows[selIndex]) { e.preventDefault(); opsLogs.toggle(rows[selIndex]); } break;
      case "Escape": if (opsLogs.expanded) { e.preventDefault(); e.stopPropagation(); opsLogs.expanded = null; } break;
      case " ": e.preventDefault(); opsLogs.togglePause(); break;
      case "c":
      case "C":
        if ((e.ctrlKey || e.metaKey) && rows[selIndex] && !(window.getSelection()?.toString() ?? "")) {
          e.preventDefault();
          void (e.shiftKey ? opsLogs.copyEvent(rows[selIndex]) : opsLogs.copyMessage(rows[selIndex]));
        }
        break;
      case "f":
      case "F":
        if (e.ctrlKey || e.metaKey) { e.preventDefault(); onfocussearch?.(); }
        break;
    }
  }

  const hostList = $derived(Object.entries(opsLogs.hostStates).map(([id, s]) => ({ id, label: opsLogs.label(id), ...s })));
  const idle = $derived(!opsLogs.running && !opsLogs.startedAt && opsLogs.rows.length === 0);
</script>

<div class="flex min-h-0 flex-1 flex-col bg-base" data-testid="log-table">
  <div class="grid shrink-0 items-center border-b border-line bg-panel/60 px-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted" style="grid-template-columns: {template}; height: 26px" role="presentation" aria-hidden="true">
    <span></span>
    {#if show.time}<span class="px-1">Time</span>{/if}
    {#if show.host}<span class="px-1">Host</span>{/if}
    {#if show.severity}<span class="px-1">Severity</span>{/if}
    {#if show.unit}<span class="px-1">Unit</span>{/if}
    <span class="px-1">Message</span>
  </div>

  <div class="relative min-h-0 flex-1">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={scroller}
      bind:clientHeight={viewH}
      bind:clientWidth={viewW}
      {onscroll}
      onkeydown={key}
      class="absolute inset-0 overflow-auto font-mono text-xs leading-5 focus-visible:outline focus-visible:outline-1 focus-visible:-outline-offset-1 focus-visible:outline-accent"
      role="grid"
      tabindex="0"
      aria-label="Log events"
      aria-rowcount={rows.length}
      aria-activedescendant={opsLogs.selected ? `logrow-${opsLogs.selected}` : undefined}
      data-testid="log-scroller"
    >
      {#if rows.length > 0}
        <div class="relative" style="height: {total}px">
          {#each visible as { r, i } (r.id)}
            {@const open = opsLogs.expanded === r.id}
            {@const h = offsets[i + 1] - offsets[i]}
            <div class="absolute inset-x-0 border-b border-line/30" style="top: {offsets[i]}px; height: {h}px" role="row" aria-rowindex={i + 1} aria-expanded={open}>
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div
                id="logrow-{r.id}"
                class="grid cursor-pointer items-start border-l-2 px-1 hover:bg-panel-hover/70 {sevRow[r.sev]} {opsLogs.selected === r.id ? '!bg-accent/15 !border-l-accent' : ''}"
                style="grid-template-columns: {template}; height: {h - (open ? DETAIL_H : 0)}px; padding-top: {wrap && h - (open ? DETAIL_H : 0) > ROW_H ? 3 : 2}px"
                role="gridcell"
                tabindex="-1"
                aria-selected={opsLogs.selected === r.id}
                onclick={() => { opsLogs.toggle(r); scroller?.focus({ preventScroll: true }); if (!opsLogs.paused) opsLogs.follow = false; }}
              >
                <span class="flex h-5 items-center text-fg-muted"><ChevronRight size={12} class="transition-transform {open ? 'rotate-90' : ''}" /></span>
                {#if show.time}<span class="truncate px-1 text-fg-muted" title="{new Date(r.ts).toLocaleString()} · {new Date(r.ts).toISOString()}">{clock(r.ts)}</span>{/if}
                {#if show.host}<span class="flex items-center gap-1 truncate px-1" title={r.host}><span class="h-1.5 w-1.5 shrink-0 rounded-full" style:background={hostHue(r.hostId)}></span><span class="truncate" style:color={hostHue(r.hostId)}>{r.host}</span></span>{/if}
                {#if show.severity}<span class="px-1"><span class="inline-block rounded px-1.5 text-[10px] font-semibold leading-4 {sevBadge[r.sev]} {r.guessed ? 'outline-dashed outline-1 outline-line' : ''}" title={r.guessed ? "Severity guessed from the words in the line" : `Priority ${r.priority}`}>{badge(r)}</span></span>{/if}
                {#if show.unit}<span class="truncate px-1 text-fg-muted" title="{r.unit || r.ident}{r.pid ? ` [${r.pid}]` : ''}">{unitShort(r.unit || r.ident)}{#if r.pid}<span class="opacity-60">[{r.pid}]</span>{/if}</span>{/if}
                <span class="min-w-0 px-1 {wrap ? 'overflow-hidden whitespace-pre-wrap break-all' : 'truncate whitespace-pre'}" style={wrap ? `max-height:${h - (open ? DETAIL_H : 0) - 4}px; line-height:${WRAP_LINE_H}px` : ""}>{#each parts(r.message) as p, k (k)}{#if p.hit}<mark class="rounded-sm bg-accent/35 text-inherit">{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
              </div>
              {#if open}<LogDetails row={r} height={DETAIL_H} />{/if}
            </div>
          {/each}
        </div>
      {:else}
        <div class="flex h-full flex-col items-center justify-center gap-3 px-6 py-8 text-center font-sans" data-testid="log-empty">
          {#if idle}
            <ScrollText size={34} class="text-accent/70" />
            <div>
              <p class="text-sm font-medium">Follow the logs of your hosts, live</p>
              <p class="mx-auto mt-1 max-w-md text-xs text-fg-muted">Add hosts above and press <strong>Follow Logs</strong>. Events from every host arrive in one stream, each marked with the host it came from. Select, stream, filter, investigate, act.</p>
            </div>
          {:else if opsLogs.isFiltered && opsLogs.rows.length > 0}
            <p class="text-sm">No event matches the filters.</p>
            <button class="btn-secondary h-8 text-xs" onclick={() => (opsLogs.filter = { ...opsLogs.filter, text: "", sev: "all", hosts: [], units: [], range: "live", from: null, to: null })}>Clear filters</button>
          {:else if opsLogs.running}
            <Loader2 size={22} class="animate-spin text-fg-muted" />
            <p class="text-sm">{opsLogs.phase === "failed" ? "Nothing could be followed." : "Waiting for events…"}</p>
            <ul class="max-w-xl space-y-1 text-xs text-fg-muted">
              {#each hostList as h (h.id)}
                <li><span class="font-medium text-fg">{h.label}</span>: {retryText(h, opsLogs.now)}{#if h.notice} <span class="text-warning">· {h.notice}</span>{/if}</li>
              {/each}
            </ul>
          {:else}
            <p class="text-sm text-fg-muted">Stopped. Nothing was received.</p>
          {/if}
        </div>
      {/if}
    </div>

    {#if rows.length > 0 && !opsLogs.follow && !opsLogs.paused}
      <button class="absolute bottom-3 right-5 inline-flex items-center gap-1 rounded-full border border-accent/50 bg-panel px-3 py-1 text-xs text-accent shadow-lg hover:bg-accent/10" onclick={() => (opsLogs.follow = true)}><ArrowDownToLine size={12} /> Jump to latest</button>
    {/if}
    {#if opsLogs.paused}
      <button class="absolute bottom-3 right-5 inline-flex items-center gap-1 rounded-full border border-warning/50 bg-panel px-3 py-1 text-xs text-warning shadow-lg hover:bg-warning/10" onclick={() => opsLogs.togglePause()}>Paused{#if opsLogs.arrivedWhilePaused} · {opsLogs.arrivedWhilePaused.toLocaleString()} new{/if} · Resume</button>
    {/if}
  </div>
</div>
