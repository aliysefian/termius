<script lang="ts">
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { registered, search } from "$lib/terminalsearch";

  let query = $state("");
  let regex = $state(false);
  let matchCase = $state(false);
  let input: HTMLInputElement;

  const sources = registered();
  const result = $derived(search(sources, query, { regex, matchCase }));

  onMount(() => input?.focus());

  function go(paneId: string, line: number) {
    const src = sources.find((s) => s.paneId === paneId);
    ui.modal = null;
    if (!src || !ui.showPane(paneId)) return;
    // Let the tab show before scrolling its terminal.
    setTimeout(() => src.reveal(line), 60);
  }
</script>

<Modal title="Search all open terminals" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <div class="space-y-3 text-sm">
    <div class="flex items-center gap-2">
      <input bind:this={input} bind:value={query} class="input flex-1" placeholder="Text to find in every terminal's screen and scrollback" aria-label="Search text" data-testid="termsearch-input" />
      <label class="flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" bind:checked={regex} /> Pattern</label>
      <label class="flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" bind:checked={matchCase} /> Match case</label>
    </div>
    {#if sources.length === 0}
      <p class="text-fg-muted">No terminals are open.</p>
    {:else if result.error}
      <p class="text-danger">{result.error}</p>
    {:else if query && result.hits.length === 0}
      <p class="text-fg-muted">Nothing found in {sources.length} terminal{sources.length === 1 ? "" : "s"}.</p>
    {/if}
    <ul class="max-h-96 space-y-0.5 overflow-auto" data-testid="termsearch-hits">
      {#each result.hits as h (h.paneId + ":" + h.line)}
        <li>
          <button class="flex w-full items-baseline gap-2 rounded px-2 py-1 text-left hover:bg-hover focus-visible:bg-hover" onclick={() => go(h.paneId, h.line)}>
            <span class="w-28 shrink-0 truncate text-xs text-fg-muted">{h.label}</span>
            <span class="min-w-0 flex-1 truncate font-mono text-xs">{h.text.slice(0, h.from)}<mark class="rounded bg-accent/30 text-fg">{h.text.slice(h.from, h.to)}</mark>{h.text.slice(h.to)}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if result.truncated}<p class="text-xs text-fg-muted">Showing the first matches only. Narrow the search to see the rest.</p>{/if}
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Close</button>
  {/snippet}
</Modal>
