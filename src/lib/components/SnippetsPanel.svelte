<script lang="ts">
  import { ServerCog, ClipboardPaste, Code, Folder, Pencil, Play, Plus, Search, Trash2 } from "lucide-svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { ask } from "$lib/dialogs.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let query = $state("");
  let tag = $state("");
  const allTags = $derived([...new Set(vaultStore.snippets.flatMap((s) => s.data?.tags ?? []))].sort());

  /** Matching snippets grouped by folder, folders sorted, unfiled first. */
  const byFolder = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const groups = new Map<string, typeof vaultStore.snippets>();
    for (const s of vaultStore.snippets) {
      const d = s.data;
      if (!d) continue;
      if (tag && !(d.tags ?? []).includes(tag)) continue;
      if (q && ![d.label, d.command, d.description, d.folder ?? "", ...(d.tags ?? [])].some((x) => x.toLowerCase().includes(q))) continue;
      const f = d.folder ?? "";
      groups.set(f, [...(groups.get(f) ?? []), s]);
    }
    return [...groups.entries()]
      .sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)))
      .map(([folder, list]) => ({ folder, list: list.sort((x, y) => x.data!.label.localeCompare(y.data!.label)) }));
  });

  async function remove(id: string, label: string) {
    if (!await ask(`Delete snippet "${label}"?`)) return;
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
      <div class="mb-2 space-y-1.5 px-1">
        <div class="relative">
          <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
          <input class="input py-1 pl-7 text-xs" placeholder="Search snippets…" bind:value={query} />
        </div>
        {#if allTags.length}
          <select class="input py-1 text-xs" bind:value={tag} aria-label="Filter by tag">
            <option value="">Any tag</option>
            {#each allTags as t (t)}<option value={t}>{t}</option>{/each}
          </select>
        {/if}
      </div>
      {#each byFolder as g (g.folder)}
        {#if g.folder}
          <div class="mt-2 flex items-center gap-1.5 px-2 pb-1 text-[11px] font-medium text-fg-muted"><Folder size={11} /> {g.folder}</div>
        {/if}
      {#each g.list as s (s.id)}
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
          {#if d.tags?.length}
            <div class="mt-1 flex flex-wrap gap-1">{#each d.tags as t (t)}<span class="rounded bg-accent/10 px-1 text-[10px] text-accent">{t}</span>{/each}</div>
          {/if}
        </div>
      {/each}
      {:else}
        <p class="px-2 py-4 text-center text-xs text-fg-muted">No matches.</p>
      {/each}
    {/if}
  </div>
</aside>
