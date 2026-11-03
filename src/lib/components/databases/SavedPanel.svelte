<script lang="ts">
  import { Copy, Pencil, Pin, PinOff, Search, Trash2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { search, sortSaved, type SavedQuery } from "$lib/savedqueries";
  import { ask, askText } from "$lib/dialogs.svelte";
  import { databases } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { connId, onopen }: { connId: string | null; onopen: (q: SavedQuery) => void } = $props();

  let text = $state("");
  const list = $derived(sortSaved(search(databases.saved, text)));

  async function rename(q: SavedQuery) {
    const name = (await askText("Name", q.name, { title: "Rename query", confirm: "Rename" }))?.trim();
    if (name) databases.updateSaved(q.id, { name });
  }

  async function describe(q: SavedQuery) {
    const description = await askText("Description", q.description, { title: "Describe the query", confirm: "Save", placeholder: "What it is for" });
    if (description !== null) databases.updateSaved(q.id, { description: description.trim() });
  }

  async function copy(sql: string) {
    try {
      await writeText(sql);
      ui.notify("info", "Copied the SQL.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function remove(q: SavedQuery) {
    if (await ask(`Delete the saved query "${q.name}"?`, { confirm: "Delete", danger: true })) databases.deleteSaved(q.id);
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex items-center gap-2 border-b border-line px-3 py-1.5">
    <div class="relative min-w-40 flex-1">
      <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input h-7 w-full pl-7 text-xs" type="search" placeholder="Search saved queries" aria-label="Search saved queries" bind:value={text} />
    </div>
    <span class="text-[11px] text-fg-muted">Saved on this computer, not synced</span>
  </div>
  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if list.length === 0}
      <p class="px-4 py-6 text-center text-xs text-fg-muted">{databases.saved.length === 0 ? "No saved queries yet. Press Ctrl+S in the editor to save one." : "No saved query matches."}</p>
    {:else}
      <ul class="divide-y divide-line/60" aria-label="Saved queries">
        {#each list as q (q.id)}
          <li class="group flex gap-2.5 px-3 py-2">
            <button class="mt-0.5 shrink-0 text-fg-muted hover:text-accent" title={q.pinned ? "Unpin" : "Pin to the top"} aria-label={q.pinned ? "Unpin" : "Pin to the top"} aria-pressed={q.pinned} onclick={() => databases.updateSaved(q.id, { pinned: !q.pinned })}>
              {#if q.pinned}<Pin size={14} class="text-accent" />{:else}<PinOff size={14} />{/if}
            </button>
            <button class="min-w-0 flex-1 text-left" onclick={() => onopen(q)} title="Open in a new tab">
              <span class="block truncate text-xs font-medium">{q.name}</span>
              {#if q.description}<span class="block truncate text-[11px] text-fg-muted">{q.description}</span>{/if}
              <span class="block truncate font-mono text-[11px] text-fg-muted/80">{q.sql.replace(/\s+/g, " ")}</span>
              <span class="block truncate text-[11px] text-fg-muted">{q.connectionName ?? "any connection"}{#if q.connectionId && q.connectionId !== connId} · opens on its own connection{/if} · {new Date(q.updatedAt).toLocaleDateString()}</span>
            </button>
            <div class="flex shrink-0 items-start opacity-0 focus-within:opacity-100 group-hover:opacity-100">
              <button class="icon-btn h-6 w-6" title="Rename" aria-label="Rename {q.name}" onclick={() => rename(q)}><Pencil size={12} /></button>
              <button class="icon-btn h-6 w-6" title="Edit the description" aria-label="Describe {q.name}" onclick={() => describe(q)}>…</button>
              <button class="icon-btn h-6 w-6" title="Copy the SQL" aria-label="Copy the SQL of {q.name}" onclick={() => copy(q.sql)}><Copy size={12} /></button>
              <button class="icon-btn h-6 w-6" title="Delete" aria-label="Delete {q.name}" onclick={() => remove(q)}><Trash2 size={12} /></button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
