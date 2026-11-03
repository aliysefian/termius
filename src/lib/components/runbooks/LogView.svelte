<script lang="ts">
  import { ArrowDownToLine, Copy, Download, Search, X } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import * as api from "$lib/api";
  import { visibleRange } from "$lib/dbdata";
  import { clock, filterLog, logText, noFilter, type LogFilter, type RunLive } from "$lib/runbookrun";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { live, runName }: { live: RunLive; runName: string } = $props();

  const ROW_H = 20;
  let filter = $state<LogFilter>(noFilter());
  let follow = $state(true);
  let scroller = $state<HTMLDivElement>();
  let top = $state(0);
  let height = $state(200);

  const lines = $derived(filterLog(live.log, filter));
  const range = $derived(visibleRange(top, height, ROW_H, lines.length, 10));
  const shown = $derived(lines.slice(range.start, range.end));
  const filtered = $derived(filter.hostId !== null || filter.step !== null || filter.problemsOnly || filter.search.trim() !== "");

  // Following the end: new lines scroll into view until the person scrolls up.
  $effect(() => {
    void lines.length;
    if (follow && scroller) scroller.scrollTop = scroller.scrollHeight;
  });

  function onscroll(e: Event) {
    const el = e.currentTarget as HTMLDivElement;
    top = el.scrollTop;
    // Scrolling away from the bottom pauses following; coming back to it resumes.
    follow = el.scrollHeight - el.scrollTop - el.clientHeight < ROW_H * 2;
  }

  async function copy() {
    try {
      await writeText(logText(lines));
      ui.notify("info", `Copied ${lines.length.toLocaleString()} line${lines.length === 1 ? "" : "s"}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function exportLog() {
    const path = await save({ title: "Export the log", defaultPath: `${runName.replace(/[^\w.-]+/g, "-").toLowerCase() || "run"}.log`, filters: [{ name: "Log", extensions: ["log", "txt"] }] });
    if (!path) return;
    try {
      await api.exportTextFile(path, logText(lines) + "\n");
      ui.notify("info", "Exported.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const tone = { stdout: "", stderr: "text-warning", info: "text-fg-muted", error: "text-danger" } as const;
  const stepOptions = $derived(live.stepNames.map((n, i) => ({ i, n })));
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex flex-wrap items-center gap-1.5 border-b border-line px-2 py-1">
    <select class="input h-7 w-auto max-w-40 py-0 text-xs" aria-label="Filter by host" bind:value={filter.hostId}>
      <option value={null}>All hosts</option>
      {#each live.hosts as h (h.hostId)}<option value={h.hostId}>{h.label}</option>{/each}
    </select>
    <select class="input h-7 w-auto max-w-44 py-0 text-xs" aria-label="Filter by step" bind:value={filter.step}>
      <option value={null}>All steps</option>
      {#each stepOptions as s (s.i)}<option value={s.i}>{s.i + 1}. {s.n}</option>{/each}
    </select>
    <label class="flex items-center gap-1 text-xs"><input type="checkbox" class="accent-input" bind:checked={filter.problemsOnly} /> Errors only</label>
    <div class="relative w-40">
      <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input h-7 w-full pl-6 pr-6 text-xs" type="search" placeholder="Search the log" aria-label="Search the log" bind:value={filter.search} />
      {#if filter.search}<button class="icon-btn absolute right-0.5 top-1/2 h-5 w-5 -translate-y-1/2" aria-label="Clear the search" onclick={() => (filter.search = "")}><X size={11} /></button>{/if}
    </div>
    <span class="text-[11px] text-fg-muted" aria-live="polite">{lines.length.toLocaleString()}{filtered ? ` of ${live.log.length.toLocaleString()}` : ""} line{lines.length === 1 ? "" : "s"}{#if live.dropped} · {live.dropped.toLocaleString()} older dropped{/if}</span>
    <div class="ml-auto flex items-center gap-1">
      <button class="icon-btn h-7 w-7 {follow ? 'text-accent' : ''}" title={follow ? "Following the end. Scroll up to pause." : "Paused. Jump to the end and follow."} aria-label="Follow the end" aria-pressed={follow} onclick={() => { follow = true; if (scroller) scroller.scrollTop = scroller.scrollHeight; }}><ArrowDownToLine size={13} /></button>
      <button class="icon-btn h-7 w-7" title="Copy these lines" aria-label="Copy these lines" disabled={lines.length === 0} onclick={copy}><Copy size={13} /></button>
      <button class="icon-btn h-7 w-7" title="Export these lines to a file" aria-label="Export the log" disabled={lines.length === 0} onclick={exportLog}><Download size={13} /></button>
    </div>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={scroller} bind:clientHeight={height} class="relative min-h-0 flex-1 overflow-auto bg-base font-mono text-[11.5px]" role="log" aria-label="Live log" tabindex="0" {onscroll} data-testid="log">
    {#if live.log.length === 0}
      <p class="px-3 py-3 text-fg-muted">Nothing has been logged yet.</p>
    {:else if lines.length === 0}
      <p class="px-3 py-3 text-fg-muted">No line matches these filters.</p>
    {:else}
      <div style="height: {lines.length * ROW_H}px; position: relative">
        <div style="transform: translateY({range.start * ROW_H}px)">
          {#each shown as l (l.n)}
            <div class="flex gap-2 whitespace-pre px-3 {l.stream === 'error' ? 'bg-danger/5' : ''}" style="height: {ROW_H}px; line-height: {ROW_H}px">
              <span class="shrink-0 text-fg-muted/70">{clock(l.at)}</span>
              <span class="w-28 shrink-0 truncate text-accent" title={l.host}>[{l.host}]</span>
              <span class="w-8 shrink-0 text-fg-muted/70">{l.step !== null ? `#${l.step + 1}` : ""}</span>
              <span class="min-w-0 {tone[l.stream]}">{#if l.stream === "stderr"}<span class="sr-only">stderr: </span>{/if}{l.text}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
