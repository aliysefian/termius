<script lang="ts">
  import { onMount } from "svelte";
  import { Copy } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "$lib/api";
  import { matchingLines, splitMatches } from "$lib/containerdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { sourceKey, id, name }: { sourceKey: string; id: string; name: string } = $props();

  let lines = $state<string[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");

  const hits = $derived(matchingLines(lines, query));

  onMount(async () => {
    const src = containers.sources[sourceKey];
    if (!src) {
      error = "This source was closed.";
      loading = false;
      return;
    }
    try {
      const raw = await api.containers.inspect(src.sessionId, src.runtime, id);
      lines = JSON.stringify(JSON.parse(raw), null, 2).split("\n");
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  });

  async function copy() {
    try {
      await writeText(lines.join("\n"));
      ui.notify("info", "Copied the inspect output.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

<Modal title="Inspect · {name}" onclose={() => (ui.modal = null)} width="max-w-4xl">
  {#if loading}
    <div class="flex items-center gap-2 py-10 text-sm text-fg-muted"><Spinner /> Loading…</div>
  {:else if error}
    <p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>
  {:else}
    <div class="mb-2 flex items-center gap-2">
      <input class="input h-8 flex-1 text-xs" placeholder="Find in this output" aria-label="Find in this output" bind:value={query} />
      <span class="text-xs text-fg-muted" aria-live="polite">{query ? `${hits.length} line${hits.length === 1 ? "" : "s"}` : ""}</span>
      <button class="btn-secondary py-1 text-xs" onclick={copy}><Copy size={12} /> Copy</button>
    </div>
    <pre class="max-h-[65vh] overflow-auto rounded-md border border-line bg-base p-3 font-mono text-xs leading-5">{#each lines as line, i (i)}{#if query}{#each splitMatches(line, query) as part, j (j)}{#if part.hit}<mark class="rounded-sm bg-warning/40 text-fg">{part.text}</mark>{:else}{part.text}{/if}{/each}{:else}{line}{/if}
{/each}</pre>
  {/if}
</Modal>
