<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { ArrowDownToLine, ChevronDown, ChevronUp, Copy, Eraser, Pause, Play } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { LogBuffer, matchingLines, splitMatches } from "$lib/containerdata";
  import { visibleRange } from "$lib/dbdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerLogEvent, type Uuid } from "$lib/types";

  let { sourceKey, id, name }: { sourceKey: string; id: string; name: string } = $props();

  const ROW_H = 18;
  const TAILS = [100, 500, 2000, 10_000] as const;

  const buffer = new LogBuffer(20_000);
  let tail = $state<number>(500);
  let follow = $state(true);
  let timestamps = $state(false);
  let query = $state("");
  let onlyMatches = $state(false);
  /** Bumped whenever the buffer changes, so the view recomputes. */
  let version = $state(0);
  let running = $state(false);
  let endNote = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(400);
  /** Whether the view is pinned to the newest line. */
  let stick = $state(true);
  let cur = $state(0);

  let streamId: Uuid | null = null;
  let generation = 0;
  let pending = "";
  let flushTimer = 0;

  // A new object every time: the buffer's own array keeps its identity as it grows, which a derived value would take for "unchanged".
  const snapshot = $derived.by(() => ({ version, lines: buffer.all() }));
  const hits = $derived(matchingLines(snapshot.lines, query));
  const display = $derived.by(() => (onlyMatches && query ? hits.map((i) => snapshot.lines[i]) : snapshot.lines));
  const displayHits = $derived(onlyMatches && query ? display.map((_, i) => i) : hits);
  const total = $derived((void snapshot.version, display.length));
  const allCount = $derived((void snapshot.version, snapshot.lines.length));
  const range = $derived(visibleRange(scrollTop, viewport, ROW_H, total));
  const shown = $derived((void snapshot.version, display.slice(range.start, range.end)));

  function flush() {
    flushTimer = 0;
    if (!pending) return;
    buffer.push(pending);
    pending = "";
    version++;
    if (stick && scroller) queueMicrotask(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  }

  function onEvent(gen: number, e: ContainerLogEvent) {
    // Events from a stream we have since replaced are ignored.
    if (gen !== generation) return;
    if (e.event === "chunk") {
      pending += e.text;
      // Batch bursts into one redraw per frame.
      if (!flushTimer) flushTimer = requestAnimationFrame(flush);
      return;
    }
    cancelAnimationFrame(flushTimer);
    flush();
    running = false;
    streamId = null;
    endNote = e.error ? e.error : e.code && e.code !== 0 ? `The command ended with status ${e.code}.` : follow ? "The log ended (the container stopped, or was removed)." : null;
  }

  async function start() {
    const src = containers.sources[sourceKey];
    if (!src) {
      endNote = "This source was closed.";
      return;
    }
    await stop();
    const gen = ++generation;
    buffer.clear();
    version++;
    endNote = null;
    stick = true;
    running = true;
    try {
      streamId = await api.containers.logsStart(src.sessionId, src.runtime, id, { tail, follow, timestamps }, (e) => onEvent(gen, e));
      // A restart may have happened while this was starting.
      if (gen !== generation) await api.containers.logsStop(streamId).catch(() => {});
    } catch (e) {
      running = false;
      endNote = errorMessage(e);
    }
  }

  async function stop() {
    generation++;
    const s = streamId;
    streamId = null;
    running = false;
    if (s) await api.containers.logsStop(s).catch(() => {});
  }

  onMount(() => void start());
  onDestroy(() => {
    cancelAnimationFrame(flushTimer);
    void stop();
  });

  function toggleFollow() {
    follow = !follow;
    void start();
  }

  function onScroll(e: Event & { currentTarget: HTMLDivElement }) {
    const el = e.currentTarget;
    scrollTop = el.scrollTop;
    stick = el.scrollTop + el.clientHeight >= el.scrollHeight - ROW_H;
  }

  function jumpToLatest() {
    stick = true;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  function go(step: number) {
    if (!displayHits.length) return;
    cur = (cur + step + displayHits.length) % displayHits.length;
    const row = displayHits[cur];
    stick = false;
    if (scroller) scroller.scrollTop = Math.max(0, row * ROW_H - viewport / 2);
  }

  // A new search starts at its first hit.
  $effect(() => {
    void query;
    cur = -1;
  });

  async function copy() {
    try {
      await writeText(display.join("\n"));
      ui.notify("info", `Copied ${display.length.toLocaleString()} line${display.length === 1 ? "" : "s"}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function clear() {
    buffer.clear();
    pending = "";
    version++;
  }
</script>

<Modal title="Logs · {name}" onclose={() => (ui.modal = null)} width="max-w-6xl">
  <div class="flex h-[68vh] min-h-72 flex-col gap-2">
    <div class="flex flex-wrap items-center gap-2 text-xs">
      <button class="btn-secondary py-1 text-xs" onclick={toggleFollow} title={follow ? "Stop following; show a snapshot" : "Follow new lines as they arrive"} aria-pressed={follow}>
        {#if follow}<Pause size={12} /> Following{:else}<Play size={12} /> Snapshot{/if}
      </button>
      <label class="flex items-center gap-1.5 text-fg-muted">
        Lines
        <select class="input h-7 w-24 py-0 text-xs" bind:value={tail} onchange={start} aria-label="How many lines from the end">
          {#each TAILS as n (n)}<option value={n}>{n.toLocaleString()}</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-1.5 text-fg-muted">
        <input type="checkbox" class="accent-input" bind:checked={timestamps} onchange={start} /> Timestamps
      </label>
      <div class="ml-auto flex items-center gap-1.5">
        <input
          class="input h-7 w-52 py-0 text-xs"
          placeholder="Search the log"
          aria-label="Search the log"
          bind:value={query}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              go(e.shiftKey ? -1 : 1);
            }
          }}
        />
        <span class="w-16 text-right text-fg-muted" aria-live="polite">{query ? (displayHits.length ? `${Math.max(cur, -1) + 1 || "–"}/${displayHits.length}` : "none") : ""}</span>
        <button class="icon-btn h-6 w-6" title="Previous match (Shift+Enter)" aria-label="Previous match" onclick={() => go(-1)} disabled={!displayHits.length}><ChevronUp size={14} /></button>
        <button class="icon-btn h-6 w-6" title="Next match (Enter)" aria-label="Next match" onclick={() => go(1)} disabled={!displayHits.length}><ChevronDown size={14} /></button>
        <label class="flex items-center gap-1.5 text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={onlyMatches} /> Only matches</label>
        <button class="btn-secondary py-1 text-xs" onclick={copy} disabled={!total}><Copy size={12} /> Copy</button>
        <button class="icon-btn h-7 w-7" title="Clear the view (the container's log is untouched)" aria-label="Clear" onclick={clear}><Eraser size={14} /></button>
      </div>
    </div>

    <div class="relative min-h-0 flex-1 rounded-md border border-line bg-base">
      <!-- A scrollable region must take keyboard focus so it can be scrolled without a mouse. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div bind:this={scroller} bind:clientHeight={viewport} class="h-full overflow-auto font-mono text-xs" role="log" aria-label="Log of {name}" tabindex="0" onscroll={onScroll}>
        <div style="height: {total * ROW_H}px; position: relative; min-width: max-content">
          <div style="transform: translateY({range.start * ROW_H}px)">
            {#each shown as line, k (range.start + k)}
              {@const row = range.start + k}
              <div class="whitespace-pre px-2 {displayHits[cur] === row ? 'bg-accent/20' : ''}" style="height: {ROW_H}px; line-height: {ROW_H}px">
                {#if query}
                  {#each splitMatches(line, query) as part, i (i)}{#if part.hit}<mark class="rounded-sm bg-warning/40 text-fg">{part.text}</mark>{:else}{part.text}{/if}{/each}
                {:else}{line}{/if}
              </div>
            {/each}
          </div>
        </div>
        {#if total === 0}
          <p class="p-3 text-fg-muted">{running ? "Waiting for output…" : onlyMatches && query ? "No lines match." : "The log is empty."}</p>
        {/if}
      </div>
      {#if follow && !stick && total}
        <button class="btn-primary absolute bottom-3 right-4 py-1 text-xs shadow-lg" onclick={jumpToLatest}><ArrowDownToLine size={12} /> Latest</button>
      {/if}
    </div>

    <div class="flex items-center gap-3 text-xs text-fg-muted">
      <span>{allCount.toLocaleString()} line{allCount === 1 ? "" : "s"}{buffer.dropped ? ` · the oldest ${buffer.dropped.toLocaleString()} were dropped to keep the view light` : ""}</span>
      <span class={endNote && !running ? "text-warning" : ""} role="status">{running ? (follow ? "Following…" : "Reading…") : (endNote ?? "")}</span>
    </div>
  </div>
</Modal>
