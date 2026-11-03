<script lang="ts">
  import { Copy, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { prettyJson } from "$lib/dbdata";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { column, type, text, onclose }: { column: string; type: string; text: string; onclose: () => void } = $props();

  let pretty = $state(true);
  const json = $derived(prettyJson(text));
  const shown = $derived(json && pretty ? json : text);
  const bytes = $derived(new TextEncoder().encode(text).length);

  async function copy() {
    try {
      await writeText(shown);
      ui.notify("info", "Copied the value.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

<section class="flex h-full min-h-0 flex-col border-l border-line bg-panel" aria-label="Value of {column}">
  <header class="flex items-center gap-2 border-b border-line px-3 py-2">
    <div class="min-w-0 flex-1">
      <h3 class="truncate text-xs font-semibold" title={column}>{column}</h3>
      <p class="truncate font-mono text-[11px] text-fg-muted">{type} · {text.length.toLocaleString()} characters · {bytes.toLocaleString()} bytes</p>
    </div>
    {#if json}
      <button class="btn-secondary h-6 px-2 text-[11px]" aria-pressed={pretty} onclick={() => (pretty = !pretty)}>{pretty ? "Raw" : "Pretty"}</button>
    {/if}
    <button class="icon-btn h-6 w-6" title="Copy the value" aria-label="Copy the value" onclick={copy}><Copy size={13} /></button>
    <button class="icon-btn h-6 w-6" title="Close" aria-label="Close the value viewer" onclick={onclose}><X size={13} /></button>
  </header>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <pre class="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all p-3 font-mono text-xs leading-5" tabindex="0">{shown}</pre>
</section>
