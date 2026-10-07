<script lang="ts">
  import { FileUp, Library, Loader2, Plus } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { STARTER_PACK, newOnes, parsePack, type Entry } from "$lib/snippetpacks";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  let tab = $state<"starter" | "file">("starter");
  let fromFile = $state<{ name: string; snippets: Entry[]; skipped: { label: string; why: string }[] } | null>(null);
  let error = $state<string | null>(null);
  let adding = $state(false);
  let unticked = $state<Set<string>>(new Set());

  const have = $derived(vaultStore.snippets.flatMap((s) => (s.data ? [s.data] : [])));
  const offered = $derived<Entry[]>(tab === "starter" ? STARTER_PACK : (fromFile?.snippets ?? []));
  const split = $derived(newOnes(have, offered));
  const chosen = $derived(split.fresh.filter((e) => !unticked.has(`${e.folder}/${e.label}`)));
  const folders = $derived([...new Set(split.fresh.map((e) => e.folder))].sort());
  const key = (e: Entry) => `${e.folder}/${e.label}`;

  function toggle(e: Entry) {
    const next = new Set(unticked);
    if (next.has(key(e))) next.delete(key(e));
    else next.add(key(e));
    unticked = next;
  }

  async function pick() {
    error = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({ title: "Choose a snippet pack", multiple: false, directory: false, filters: [{ name: "Snippet pack", extensions: ["json"] }] });
      if (!path || Array.isArray(path)) return;
      const r = parsePack(await api.readTextFile(path));
      if (r.error) {
        fromFile = null;
        error = r.error;
        return;
      }
      fromFile = { name: path.split(/[\\/]/).pop() ?? path, snippets: r.snippets, skipped: r.skipped };
      unticked = new Set();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function add() {
    adding = true;
    error = null;
    let done = 0;
    try {
      for (const e of chosen) {
        await vaultStore.saveSnippet(null, { label: e.label, command: e.command, description: e.description, folder: e.folder || undefined, tags: e.tags });
        done++;
      }
      ui.notify("info", `Added ${done} snippet${done === 1 ? "" : "s"}. Nothing was run.`);
      ui.modal = null;
    } catch (e) {
      error = `Added ${done}, then: ${errorMessage(e)}`;
    } finally {
      adding = false;
    }
  }
</script>

<Modal title="Add snippets" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <div class="space-y-4 text-sm">
    <div class="flex w-fit gap-1 rounded-lg border border-line bg-base p-1" role="tablist">
      <button role="tab" aria-selected={tab === "starter"} class="flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium {tab === 'starter' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" onclick={() => (tab = "starter")}><Library size={14} /> Starter set</button>
      <button role="tab" aria-selected={tab === "file"} class="flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium {tab === 'file' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" onclick={() => (tab = "file")}><FileUp size={14} /> From a file</button>
    </div>

    {#if tab === "starter"}
      <p class="text-xs text-fg-muted">Commands for looking at a server: disk, memory, processes, services, logs, network, Docker and Git. They only read; none of them changes anything. A few ask for a value (a service name, a file) when you run them.</p>
    {:else if !fromFile}
      <p class="text-xs text-fg-muted">A pack is a <code>.json</code> file made with <strong>Export</strong> on the Snippets list. You see what is in it before anything is added, and nothing in it is run.</p>
      <button class="btn-secondary" onclick={() => void pick()}><FileUp size={14} /> Choose a file…</button>
    {:else}
      <p class="text-xs text-fg-muted"><strong>{fromFile.name}</strong>: {fromFile.snippets.length} snippet{fromFile.snippets.length === 1 ? "" : "s"}.{#if fromFile.skipped.length} {fromFile.skipped.length} left out as unusable.{/if} <button class="text-accent hover:underline" onclick={() => void pick()}>Choose another</button></p>
      {#if fromFile.skipped.length}
        <ul class="rounded-md border border-line bg-base/60 p-2 text-xs text-fg-muted">
          {#each fromFile.skipped.slice(0, 5) as sk, i (i)}<li>“{sk.label}”: {sk.why}</li>{/each}
          {#if fromFile.skipped.length > 5}<li>and {fromFile.skipped.length - 5} more</li>{/if}
        </ul>
      {/if}
    {/if}

    {#if error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-danger">{error}</p>{/if}

    {#if offered.length}
      <div class="max-h-80 space-y-3 overflow-y-auto rounded-md border border-line bg-base/40 p-3">
        {#each folders as f (f)}
          <div>
            <div class="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">{f || "No folder"}</div>
            {#each split.fresh.filter((e) => e.folder === f) as e (key(e))}
              <label class="flex cursor-pointer items-start gap-2 rounded px-1.5 py-1 hover:bg-panel-hover">
                <input type="checkbox" class="mt-1 accent-input" checked={!unticked.has(key(e))} onchange={() => toggle(e)} />
                <span class="min-w-0">
                  <span class="block font-medium">{e.label}</span>
                  <code class="block truncate text-[11px] text-fg-muted" title={e.command}>{e.command.split("\n")[0]}</code>
                </span>
              </label>
            {/each}
          </div>
        {:else}
          <p class="text-xs text-fg-muted">You already have all of these.</p>
        {/each}
      </div>
      {#if split.already}<p class="text-xs text-fg-muted">{split.already} already in your list, not offered again.</p>{/if}
    {/if}
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" disabled={!chosen.length || adding} onclick={() => void add()}>
      {#if adding}<Loader2 size={14} class="animate-spin" />{:else}<Plus size={14} />{/if} Add {chosen.length || ""} snippet{chosen.length === 1 ? "" : "s"}
    </button>
  {/snippet}
</Modal>
