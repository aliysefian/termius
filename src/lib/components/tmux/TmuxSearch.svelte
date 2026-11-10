<script lang="ts">
  import { Search, X } from "lucide-svelte";
  import { onMount } from "svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let { onclose, embedded = false, focus = true }: { onclose?: () => void; focus?: boolean; /** Drawn inside the bottom panel: it does not close itself when a hit is opened. */ embedded?: boolean } = $props();

  let input = $state<HTMLInputElement>();
  onMount(() => {
    if (focus) input?.focus();
  });

  const scopes = [
    { id: "all", label: "All watched hosts" },
    { id: "host", label: "Selected host" },
    { id: "session", label: "Selected session" },
    { id: "window", label: "Selected window" },
    { id: "pane", label: "Selected pane" },
  ] as const;

  const report = $derived(mon.searchReport);
  const total = $derived(report?.results.reduce((n, r) => n + r.hits.length, 0) ?? 0);

  const nameOf = (server: string) => {
    const snap = mon.snapshotOf(server);
    return snap ? (vaultStore.hostById.get(snap.host_id)?.data?.label ?? "host") : "host";
  };
  const sessionName = (server: string, id: string) => mon.snapshotOf(server)?.sessions.find((s) => s.id === id)?.name ?? id;

  /** A hit's line cut into runs, marking the characters that matched. */
  function segments(h: { text: string; spans: [number, number][] }): [string, boolean][] {
    const chars = [...h.text];
    const out: [string, boolean][] = [];
    let at = 0;
    for (const [a, b] of h.spans) {
      if (a > at) out.push([chars.slice(at, a).join(""), false]);
      out.push([chars.slice(a, b).join(""), true]);
      at = b;
    }
    if (at < chars.length) out.push([chars.slice(at).join(""), false]);
    return out;
  }

  function go(server: string, r: { session_id: string; window_id: string; pane_id: string }) {
    mon.reveal(server, r.session_id, r.window_id, r.pane_id);
    if (!embedded) onclose?.();
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Enter") void mon.search();
    if (e.key === "Escape" && !embedded) onclose?.();
  }
</script>

<section class="flex h-full min-w-0 flex-1 flex-col bg-base" aria-label="Search pane output">
  <header class="shrink-0 space-y-2 border-b border-line bg-panel/50 p-3">
    <div class="flex items-center gap-2">
      <Search size={14} class="text-fg-muted" />
      <input
        class="min-w-0 flex-1 rounded-md border border-line bg-base px-2 py-1 text-sm focus:border-accent focus:outline-none"
        placeholder="Search the text currently in the panes"
        aria-label="Text to search for in pane output"
        bind:this={input}
        bind:value={mon.searchText}
        onkeydown={key}
      />
      {#if mon.searching}
        <button class="rounded-md border border-line px-2 py-1 text-xs hover:bg-panel-hover" onclick={() => mon.cancelSearch()}>Cancel</button>
      {:else}
        <button class="rounded-md border border-accent/50 bg-accent/10 px-2 py-1 text-xs text-accent hover:bg-accent/20 disabled:opacity-50" disabled={!mon.searchText} onclick={() => mon.search()}>Search</button>
      {/if}
      {#if !embedded}<button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" aria-label="Close search" onclick={() => { mon.clearSearch(); onclose?.(); }}><X size={14} /></button>{/if}
    </div>
    <div class="flex flex-wrap items-center gap-3 text-xs text-fg-muted">
      <label class="flex items-center gap-1"><input type="checkbox" bind:checked={mon.searchCase} /> Match case</label>
      <label class="flex items-center gap-1"><input type="checkbox" bind:checked={mon.searchRegex} /> Regular expression</label>
      <label class="flex items-center gap-1">In
        <select class="rounded border border-line bg-base px-1 py-0.5 text-fg" bind:value={mon.searchScope}>
          {#each scopes as s (s.id)}<option value={s.id}>{s.label}</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-1">History
        <select class="rounded border border-line bg-base px-1 py-0.5 text-fg" bind:value={mon.prefs.searchScrollback}>
          {#each [0, 200, 1000, 5000] as n (n)}<option value={n}>{n === 0 ? "Screen only" : `${n} lines`}</option>{/each}
        </select>
      </label>
    </div>
    <p class="text-[11px] text-fg-muted">Searches the screen and the scrollback tmux still holds for each pane. Output tmux has already discarded cannot be found. Captured text stays in memory and is never saved or sent anywhere.</p>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto p-3 text-sm" aria-live="polite">
    {#if mon.searching}
      <p class="text-fg-muted">Searching…</p>
    {:else if mon.searchError}
      <p class="text-danger" role="alert">{mon.searchError}</p>
    {:else if report}
      <p class="mb-2 text-xs text-fg-muted">
        {total} {total === 1 ? "match" : "matches"} in {report.results.length} {report.results.length === 1 ? "pane" : "panes"} · {report.searched} searched
        {#if report.limited} · stopped at the limit; narrow the search{/if}{#if report.cancelled} · cancelled{/if}
      </p>
      {#if report.failed.length}<p class="mb-2 text-xs text-warning">{report.failed.length} {report.failed.length === 1 ? "pane" : "panes"} could not be read.</p>{/if}
      {#each report.results as r (r.server + r.pane_id)}
        <div class="mb-3 rounded-lg border border-line">
          <button class="flex w-full items-center gap-2 border-b border-line bg-panel/50 px-2 py-1 text-left text-xs hover:bg-panel-hover" onclick={() => go(r.server, r)}>
            <span class="font-medium">{nameOf(r.server)}</span><span class="text-fg-muted">›</span><span>{sessionName(r.server, r.session_id)}</span><span class="font-mono text-fg-muted">{r.pane_id}</span><span class="ml-auto text-fg-muted">{r.hits.length}</span>
          </button>
          {#each r.hits as h, i (i)}
            <div class="px-2 py-1 font-mono text-[11.5px]">
              {#each h.before as l, j (j)}<div class="truncate text-fg-muted/70">{l}</div>{/each}
              <div class="whitespace-pre-wrap break-all"><span class="mr-2 select-none text-fg-muted">{h.line}</span>{#each segments(h) as [t, hit], k (k)}{#if hit}<mark class="rounded-[2px] bg-warning/60 text-inherit">{t}</mark>{:else}{t}{/if}{/each}</div>
              {#each h.after as l, j (j)}<div class="truncate text-fg-muted/70">{l}</div>{/each}
            </div>
          {/each}
        </div>
      {/each}
      {#if total === 0}<p class="text-fg-muted">No matches.</p>{/if}
    {:else}
      <p class="text-fg-muted">Type something and press Enter.</p>
    {/if}
  </div>
</section>
