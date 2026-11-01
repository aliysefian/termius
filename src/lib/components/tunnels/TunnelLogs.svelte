<script lang="ts">
  import { ChevronDown, ChevronRight, ClipboardCopy, Search } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { tunnels } from "$lib/stores/tunnels.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { diagnostics, filterLogs, logTime, type LevelFilter, type TunnelRow } from "$lib/tunnels";
  import { errorMessage } from "$lib/types";

  let { row }: { row: TunnelRow } = $props();

  let query = $state("");
  let level = $state<LevelFilter>("all");
  let follow = $state(true);
  let open = $state<Set<number>>(new Set());
  let box = $state<HTMLDivElement>();

  const all = $derived(tunnels.logs[row.id] ?? []);
  const shown = $derived(filterLogs(all, query, level));

  // Stay at the newest line while following; scrolling up by hand turns following off.
  $effect(() => {
    void shown.length;
    if (follow && box) queueMicrotask(() => box && (box.scrollTop = box.scrollHeight));
  });
  function scrolled() {
    if (!box) return;
    const atEnd = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
    if (!atEnd && follow) follow = false;
  }

  async function copy() {
    try {
      await writeText(diagnostics(row, shown));
      ui.notify("info", "Diagnostics copied. They contain no passwords or keys.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const tone = { info: "text-fg-muted", warn: "text-warning", error: "text-danger" } as const;
  const word = { info: "INFO", warn: "WARN", error: "ERROR" } as const;
</script>

<div class="flex h-full min-h-0 flex-col gap-3">
  <div class="flex flex-wrap items-center gap-2">
    <div class="relative min-w-[8rem] flex-1">
      <Search size={14} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" aria-hidden="true" />
      <input class="input h-8 py-0 pl-8 text-sm" placeholder="Search the log" aria-label="Search the log" bind:value={query} />
    </div>
    <select class="input h-8 w-auto py-0 text-sm" aria-label="Severity" bind:value={level}>
      <option value="all">All levels</option>
      <option value="info">Info</option>
      <option value="warn">Warnings</option>
      <option value="error">Errors</option>
    </select>
    <label class="flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={follow} /> Auto-scroll</label>
    <button class="icon-btn h-8 w-8 border border-line" title="Copy diagnostics" aria-label="Copy diagnostics" onclick={() => void copy()}><ClipboardCopy size={15} /></button>
  </div>

  <div bind:this={box} class="min-h-[12rem] flex-1 overflow-y-auto rounded-[10px] border border-line bg-base/60 p-1 font-mono text-xs leading-5" role="log" aria-live="off" aria-label="Tunnel log" onscroll={scrolled}>
    {#each shown as l, i (l.at_ms + ":" + i)}
      {@const long = l.message.length > 90}
      <div class="flex items-start gap-2 rounded px-2 py-0.5 hover:bg-panel-hover">
        <span class="shrink-0 tabular-nums text-fg-muted">{logTime(l.at_ms)}</span>
        <span class="w-11 shrink-0 font-semibold {tone[l.level]}">{word[l.level]}</span>
        <span class="min-w-0 flex-1 {open.has(l.at_ms * 1000 + i) ? 'whitespace-pre-wrap break-words' : 'truncate'}" title={long ? "" : l.message}>{l.message}</span>
        {#if long}
          <button class="shrink-0 text-fg-muted hover:text-fg" aria-label={open.has(l.at_ms * 1000 + i) ? "Collapse this line" : "Expand this line"} aria-expanded={open.has(l.at_ms * 1000 + i)} onclick={() => { const k = l.at_ms * 1000 + i; const n = new Set(open); if (!n.delete(k)) n.add(k); open = n; }}>
            {#if open.has(l.at_ms * 1000 + i)}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
          </button>
        {/if}
      </div>
    {:else}
      <p class="p-4 text-center font-sans text-sm text-fg-muted">
        {all.length ? "No lines match." : "Nothing logged yet. Start the tunnel and its events appear here."}
      </p>
    {/each}
  </div>
  <p class="text-[11px] text-fg-muted">The last {all.length} of at most 200 lines. Passwords, keys and tokens are removed before a line is kept.</p>
</div>
