<script lang="ts">
  import { ServerCog, ClipboardPaste, Code, Pencil, Play, Plus, Trash2 } from "lucide-svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  async function remove(id: string, label: string) {
    if (!confirm(`Delete snippet "${label}"?`)) return;
    await vaultStore.deleteSnippet(id);
  }
</script>

<aside class="flex w-72 flex-col border-r border-line bg-panel">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <h2 class="text-sm font-semibold">Snippets</h2>
    <button class="icon-btn" title="New snippet" onclick={() => (ui.modal = { kind: "snippet", id: null })}>
      <Plus size={16} />
    </button>
  </div>
  <div class="flex-1 overflow-y-auto px-2 pb-4">
    {#if vaultStore.snippets.length === 0}
      <div class="px-3 py-10 text-center">
        <Code size={28} class="mx-auto mb-3 text-fg-muted/50" />
        <p class="text-sm text-fg-muted">No snippets yet.</p>
        <button class="btn-primary mt-4" onclick={() => (ui.modal = { kind: "snippet", id: null })}>
          <Plus size={14} /> Add snippet
        </button>
      </div>
    {:else}
      {#each vaultStore.snippets as s (s.id)}
        {@const d = s.data!}
        <div class="group rounded-md px-2 py-1.5 hover:bg-panel-hover">
          <div class="flex items-center gap-2">
            <div class="min-w-0 flex-1 truncate text-sm">{d.label}</div>
            <div class="flex opacity-0 group-hover:opacity-100">
              <button class="icon-btn h-6 w-6" title="Run on several hosts…" onclick={() => (ui.modal = { kind: "run-on-hosts", command: d.command })}><ServerCog size={12} /></button>
              <button class="icon-btn h-6 w-6" title="Paste into active terminal" onclick={() => runSnippet(d.command, { execute: false, scope: "pane" })}><ClipboardPaste size={12} /></button>
              <button class="icon-btn h-6 w-6 text-accent" title="Run in active terminal" onclick={() => runSnippet(d.command, { execute: true, scope: "pane" })}><Play size={12} /></button>
              <button class="icon-btn h-6 w-6" title="Edit" onclick={() => (ui.modal = { kind: "snippet", id: s.id })}><Pencil size={12} /></button>
              <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" onclick={() => remove(s.id, d.label)}><Trash2 size={12} /></button>
            </div>
          </div>
          <pre class="mt-1 truncate rounded bg-base px-2 py-1 font-mono text-xs text-fg-muted">{d.command}</pre>
        </div>
      {/each}
    {/if}
  </div>
</aside>
