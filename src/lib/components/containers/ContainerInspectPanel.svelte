<script lang="ts">
  import { onMount } from "svelte";
  import { Copy, Eye, EyeOff, Loader2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import JsonNode from "./JsonNode.svelte";
  import { redactInspect } from "$lib/containerview";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  /** `secrets` is true for a container, whose environment may hold some. */
  let { load, secrets = false }: { load: () => Promise<string>; secrets?: boolean } = $props();

  let data = $state<unknown>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  let reveal = $state(false);

  // Secret-looking environment values stay hidden until asked for; everything else is exactly what the runtime printed.
  const shown = $derived(data === null ? null : secrets ? redactInspect(data, reveal) : data);
  const root = $derived(Array.isArray(shown) && shown.length === 1 ? shown[0] : shown);

  onMount(async () => {
    try {
      data = JSON.parse(await load());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  });

  async function copy(text: string) {
    try {
      await writeText(text);
      ui.notify("info", "Copied.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col gap-2">
  {#if loading}
    <div class="flex items-center gap-2 py-10 text-sm text-fg-muted"><Loader2 size={15} class="animate-spin" /> Loading…</div>
  {:else if error}
    <p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>
  {:else}
    <div class="flex flex-wrap items-center gap-2">
      <input class="input h-8 min-w-[8rem] flex-1 py-0 text-xs" placeholder="Search keys and values" aria-label="Search the inspect output" bind:value={query} />
      {#if secrets}
        <button class="btn-secondary h-8 text-xs" aria-pressed={reveal} title={reveal ? "Hide secret-looking environment values again" : "Show environment values that look like passwords, tokens or keys"} onclick={() => (reveal = !reveal)}>
          {#if reveal}<EyeOff size={13} /> Hide secrets{:else}<Eye size={13} /> Reveal secrets{/if}
        </button>
      {/if}
      <button class="btn-secondary h-8 text-xs" onclick={() => void copy(JSON.stringify(root, null, 2))}><Copy size={13} /> Copy JSON</button>
    </div>
    {#if secrets && !reveal}<p class="text-[11px] text-fg-muted">Environment values that look like passwords, tokens or keys are hidden. Copy JSON copies what is shown.</p>{/if}
    <div class="min-h-0 flex-1 overflow-auto rounded-[10px] border border-line bg-base/60 p-2">
      <JsonNode label={null} value={root} {query} oncopy={(t) => void copy(t)} />
    </div>
  {/if}
</div>
