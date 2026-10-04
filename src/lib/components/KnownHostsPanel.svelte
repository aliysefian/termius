<script lang="ts">
  import { onMount } from "svelte";
  import { FileInput, History, RefreshCw, ShieldCheck, Trash2 } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Spinner from "./Spinner.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type VaultKnownHost } from "$lib/types";

  let entries = $state<VaultKnownHost[]>([]);
  let filter = $state("");
  let loading = $state(false);
  let openHistory = $state<string | null>(null);

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
    entries.filter((e) => {
      const q = filter.trim().toLowerCase();
      return !q || e.host.includes(q) || e.fingerprint.toLowerCase().includes(q);
    }),
  );

  const label = (e: VaultKnownHost) => (e.port === 22 ? e.host : `[${e.host}]:${e.port}`);
  const when = (ms: number) => new Date(ms).toLocaleString();
  const savedHost = (e: VaultKnownHost) =>
    vaultStore.hosts.find((h) => h.data?.hostname.toLowerCase() === e.host && h.data.port === e.port)?.data?.label;

  async function forget(e: VaultKnownHost) {
    if (!await ask(`Stop trusting the key for ${label(e)}? You'll be asked to check its fingerprint again on the next connection, on every device.`)) return;
    try {
      await api.knownHosts.forget(e.host, e.port);
      await load();
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  async function importFile(pick: boolean) {
    let path: string | null = null;
    if (pick) {
      const f = await open({ multiple: false, directory: false, title: "Choose a known_hosts file" });
      if (typeof f !== "string") return;
      path = f;
    } else if (!await ask("Trust every host key in ~/.ssh/known_hosts? Hosts already in the vault keep the key they have.")) {
      return;
    }
    try {
      const r = await api.knownHosts.import(path);
      ui.notify(
        "info",
        `Imported ${r.added} key(s); ${r.existing} already known${r.hashed ? `, ${r.hashed} hashed entries skipped (their host names can't be read)` : ""}${r.invalid ? `, ${r.invalid} unreadable` : ""}.`,
      );
      await load();
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-4">
    <div class="flex items-center justify-between">
      <h1 class="flex items-center gap-2 text-lg font-semibold"><ShieldCheck size={18} class="text-accent" /> Known hosts</h1>
      <div class="flex gap-2">
        <button class="icon-btn" title="Reload" onclick={load}><RefreshCw size={14} class={loading ? "animate-spin" : ""} /></button>
        <button class="btn-secondary" onclick={() => importFile(true)}>From file…</button>
        <button class="btn-secondary" onclick={() => importFile(false)}><FileInput size={14} /> Import ~/.ssh/known_hosts</button>
      </div>
    </div>
    <p class="text-xs text-fg-muted">
      Server keys you've trusted, stored encrypted in the vault so all your devices check servers the same way. A server
      presenting a different key is refused until you compare fingerprints and replace it deliberately.
    </p>
    {#if entries.length > 6}
      <input class="input py-1.5 text-sm" placeholder="Filter by host or fingerprint…" bind:value={filter} />
    {/if}
    <div class="divide-y divide-line rounded-xl border border-line bg-panel">
      {#each shown as e (e.id)}
        <div class="group px-4 py-2.5">
          <div class="flex items-center gap-3">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2 text-sm">
                <span class="truncate font-mono">{label(e)}</span>
                {#if savedHost(e)}<span class="truncate text-xs text-fg-muted">{savedHost(e)}</span>{/if}
              </div>
              <div class="truncate font-mono text-[11px] text-fg-muted">{e.algorithm} · {e.fingerprint}</div>
              <div class="text-[11px] text-fg-muted">Trusted {when(e.trusted_at)} on {e.trusted_by}</div>
            </div>
            {#if e.history.length}
              <button class="icon-btn h-7 w-7" title="Replaced keys" onclick={() => (openHistory = openHistory === e.id ? null : e.id)}><History size={13} /></button>
            {/if}
            <button class="icon-btn reveal h-7 w-7 hover:text-danger" title="Stop trusting" onclick={() => forget(e)}>
              <Trash2 size={13} />
            </button>
          </div>
          {#if openHistory === e.id}
            <ul class="mt-2 space-y-1 rounded-md border border-line bg-base p-2 font-mono text-[11px] text-fg-muted">
              {#each e.history as h, i (i)}
                <li>{h.algorithm} {h.fingerprint}, replaced {when(h.replaced_at)} on {h.replaced_by}</li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else}
        {#if loading}
          <div class="flex justify-center py-8"><Spinner label="Loading…" /></div>
        {:else}
          <p class="px-4 py-8 text-center text-sm text-fg-muted">No trusted host keys yet. You'll be asked the first time you connect to a server.</p>
        {/if}
      {/each}
    </div>
  </div>
</div>
