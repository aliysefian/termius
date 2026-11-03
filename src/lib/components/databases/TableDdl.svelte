<script lang="ts">
  import { Copy } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type DbTableMeta } from "$lib/types";

  let { meta, onquery }: { meta: DbTableMeta; onquery: (sql: string) => void } = $props();

  async function copy() {
    if (!meta.ddl) return;
    try {
      await writeText(meta.ddl.text);
      ui.notify("info", "Copied the definition.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#if !meta.ddl}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">The server didn't return a definition.</p>
{:else}
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex items-center gap-2 border-b border-line px-3 py-1.5 text-xs">
      <p class="min-w-0 flex-1 text-fg-muted">
        {#if meta.ddl.reconstructed}Rebuilt by SSHVault from the server's catalog. It describes the same columns, constraints and indexes, but it is not guaranteed to be identical to the statement that created the table.{:else}As the server reports it.{/if}
      </p>
      <button class="btn-secondary h-6 px-2 text-xs" onclick={copy}><Copy size={12} /> Copy</button>
      <button class="btn-secondary h-6 px-2 text-xs" onclick={() => onquery(meta.ddl!.text)}>Open in a query tab</button>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="min-h-0 flex-1 overflow-auto whitespace-pre p-4 font-mono text-xs leading-5" tabindex="0" aria-label="Definition">{meta.ddl.text}</pre>
  </div>
{/if}
