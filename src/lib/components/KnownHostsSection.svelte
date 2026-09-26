<script lang="ts">
  import { onMount } from "svelte";
  import { RefreshCw, ShieldCheck, Trash2 } from "lucide-svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type KnownHost } from "$lib/types";

  let entries = $state<KnownHost[]>([]);
  let filter = $state("");
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      entries = await api.knownHosts.list();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      loading = false;
    }
  }
  onMount(load);

  const shown = $derived(
    entries.filter((e) => !filter || e.hosts.some((h) => h.toLowerCase().includes(filter.toLowerCase()))),
  );

  async function remove(e: KnownHost) {
    const name = e.hashed ? "this hashed entry" : e.hosts.join(", ");
    if (!confirm(`Forget the pinned key for ${name}? The next connection will trust whatever key the server presents.`)) return;
    try {
      await api.knownHosts.remove(e.line);
      await load();
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }
</script>

<section class="rounded-xl border border-line bg-panel p-5">
  <div class="mb-1 flex items-center justify-between">
    <h2 class="flex items-center gap-2 text-sm font-semibold"><ShieldCheck size={15} class="text-accent" /> Known hosts</h2>
    <button class="icon-btn h-7 w-7" title="Reload" onclick={load}><RefreshCw size={13} class={loading ? "animate-spin" : ""} /></button>
  </div>
  <p class="mb-3 text-xs text-fg-muted">
    Server keys pinned on this computer the first time you connected. Remove an entry only if you know the server's key
    changed legitimately.
  </p>
  {#if entries.length > 5}
    <input class="input mb-2 py-1.5 text-xs" placeholder="Filter hosts…" bind:value={filter} />
  {/if}
  <div class="max-h-72 divide-y divide-line overflow-y-auto rounded-md border border-line">
    {#each shown as e (e.line)}
      <div class="group flex items-center gap-3 px-3 py-2">
        <div class="min-w-0 flex-1">
          <div class="truncate font-mono text-xs">{e.hashed ? "(hashed host name)" : e.hosts.join(", ")}</div>
          <div class="truncate font-mono text-[11px] text-fg-muted">{e.algorithm} · {e.fingerprint ?? "unreadable key"}</div>
        </div>
        <button class="icon-btn h-7 w-7 opacity-0 hover:text-danger group-hover:opacity-100" title="Forget" onclick={() => remove(e)}>
          <Trash2 size={13} />
        </button>
      </div>
    {:else}
      <p class="px-3 py-6 text-center text-xs text-fg-muted">{loading ? "Loading…" : "No pinned keys yet."}</p>
    {/each}
  </div>
</section>
