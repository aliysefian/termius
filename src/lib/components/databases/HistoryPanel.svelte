<script lang="ts">
  import { CircleAlert, CircleCheck, Copy, Search, Star, Trash2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { HISTORY_KEEP_CHOICES, search, type HistoryEntry } from "$lib/dbhistory";
  import { ask } from "$lib/dialogs.svelte";
  import { databases } from "$lib/stores/databases.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage } from "$lib/types";

  let { onopen }: { onopen: (sql: string) => void } = $props();

  let text = $state("");
  const list = $derived(search(databases.history, text));

  async function copy(sql: string) {
    try {
      await writeText(sql);
      ui.notify("info", "Copied the SQL.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function saveAsQuery(h: HistoryEntry) {
    const tab = databases.newTab(databases.active?.connId ?? databases.tabs[0]?.connId ?? "", h.sql);
    await databases.saveQuery(tab.id, true);
  }

  async function clear() {
    if (!(await ask("Clear the query history on this computer?", { confirm: "Clear", danger: true }))) return;
    databases.clearHistory();
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex flex-wrap items-center gap-2 border-b border-line px-3 py-1.5">
    <div class="relative min-w-40 flex-1">
      <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input h-7 w-full pl-7 text-xs" type="search" placeholder="Search the history" aria-label="Search the history" bind:value={text} />
    </div>
    <label class="flex items-center gap-1.5 text-[11px] text-fg-muted" title="How many runs to keep on this computer. History is never synced.">
      Keep
      <select class="input !w-auto h-7 py-0 text-xs" bind:value={settings.prefs.dbHistoryKeep} onchange={() => databases.trimHistory()}>
        {#each HISTORY_KEEP_CHOICES as n (n)}<option value={n}>{n === 0 ? "nothing" : n}</option>{/each}
      </select>
    </label>
    <button class="btn-secondary h-7 px-2 text-xs" disabled={databases.history.length === 0} onclick={clear}><Trash2 size={12} /> Clear</button>
  </div>
  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if list.length === 0}
      <p class="px-4 py-6 text-center text-xs text-fg-muted">{databases.history.length === 0 ? (settings.prefs.dbHistoryKeep === 0 ? "History is off. Choose how much to keep above." : "Nothing yet. History stays on this computer and is never synced.") : "No run matches."}</p>
    {:else}
      <ul class="divide-y divide-line/60" aria-label="Query history">
        {#each list as h (h.at + h.sql + h.connection)}
          <li class="group flex gap-2.5 px-3 py-2">
            <span class="mt-0.5 shrink-0">
              {#if h.ok === false}<CircleAlert size={14} class="text-danger" aria-label="Failed" />{:else if h.ok}<CircleCheck size={14} class="text-success" aria-label="Succeeded" />{:else}<span class="inline-block h-3.5 w-3.5"></span>{/if}
            </span>
            <button class="min-w-0 flex-1 text-left" onclick={() => onopen(h.sql)} title="Open in the editor">
              <span class="block truncate font-mono text-xs">{h.sql.replace(/\s+/g, " ")}</span>
              <span class="block truncate text-[11px] text-fg-muted">
                {h.connection} · {new Date(h.at).toLocaleString()}{#if h.ms !== undefined} · {h.ms} ms{/if}{#if h.rows !== undefined && h.rows !== null} · {h.rows.toLocaleString()} row{h.rows === 1 ? "" : "s"}{/if}{#if h.ok === false && h.error} · <span class="text-danger">{h.error}</span>{/if}
              </span>
            </button>
            <div class="flex shrink-0 items-start opacity-0 focus-within:opacity-100 group-hover:opacity-100">
              <button class="icon-btn h-6 w-6" title="Copy the SQL" aria-label="Copy the SQL" onclick={() => copy(h.sql)}><Copy size={12} /></button>
              <button class="icon-btn h-6 w-6" title="Save as a query" aria-label="Save as a query" onclick={() => saveAsQuery(h)}><Star size={12} /></button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
