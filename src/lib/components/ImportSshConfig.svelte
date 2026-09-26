<script lang="ts">
  import { onMount } from "svelte";
  import { TriangleAlert, FileInput, Loader2 } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { describeForward, errorMessage, type ImportSummary, type KeyImport, type SshConfigPreview } from "$lib/types";

  let preview = $state<SshConfigPreview | null>(null);
  let picked = $state<Set<string>>(new Set());
  let group = $state("Imported");
  /** Key files are only copied into the vault if the user chooses to. */
  let keyImport = $state<KeyImport>("reference");
  const pickedHosts = $derived(preview?.hosts.filter((h) => picked.has(h.alias)) ?? []);
  const hasKeys = $derived(pickedHosts.some((h) => h.identity_file));
  const hasCommands = $derived(pickedHosts.some((h) => h.proxy_command));
  let loading = $state(false);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let summary = $state<ImportSummary | null>(null);

  async function load(path: string | null) {
    loading = true;
    error = null;
    try {
      preview = await api.sshConfig.preview(path);
      const existing = new Set(preview.existing);
      picked = new Set(preview.hosts.filter((h) => !existing.has(h.alias)).map((h) => h.alias));
    } catch (e) {
      error = errorMessage(e);
      preview = null;
    } finally {
      loading = false;
    }
  }
  onMount(() => load(null));

  let source = $state<"ssh" | "ansible">("ssh");

  async function chooseFile() {
    const f = await open({ multiple: false, directory: false, title: "Choose an OpenSSH config file" });
    if (typeof f === "string") {
      source = "ssh";
      await load(f);
    }
  }

  async function chooseInventory() {
    const f = await open({ multiple: false, directory: false, title: "Choose an Ansible inventory (INI)" });
    if (typeof f !== "string") return;
    source = "ansible";
    loading = true;
    error = null;
    try {
      preview = await api.ansible.preview(f);
      const existing = new Set(preview.existing);
      picked = new Set(preview.hosts.filter((h) => !existing.has(h.alias)).map((h) => h.alias));
    } catch (e) {
      error = errorMessage(e);
      preview = null;
    } finally {
      loading = false;
    }
  }

  function toggle(alias: string) {
    const next = new Set(picked);
    if (next.has(alias)) next.delete(alias);
    else next.add(alias);
    picked = next;
  }

  async function doImport() {
    if (!preview) return;
    importing = true;
    error = null;
    try {
      const hosts = preview.hosts.filter((h) => picked.has(h.alias));
      summary = await api.sshConfig.import(hosts, group, keyImport);
      await vaultStore.reloadAll();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      importing = false;
    }
  }
</script>

<Modal title={source === "ansible" ? "Import from Ansible inventory" : "Import from SSH config"} onclose={() => (ui.modal = null)} width="max-w-3xl">
  {#if summary}
    <div class="space-y-3 text-sm">
      <p>
        Imported <strong>{summary.hosts_created}</strong> host{summary.hosts_created === 1 ? "" : "s"} and created
        <strong>{summary.identities_created}</strong> credential{summary.identities_created === 1 ? "" : "s"}{summary.keys_imported
          ? `, copied ${summary.keys_imported} key(s) into the vault`
          : ""}{summary.forwards_created ? `, ${summary.forwards_created} tunnel(s)` : ""}{summary.proxies_created
          ? `, ${summary.proxies_created} ProxyCommand(s)`
          : ""}.
      </p>
      {#if summary.proxies_created}
        <p class="text-xs text-warning">
          Imported ProxyCommands don't run until you review and approve them under Groups and proxies.
        </p>
      {/if}
      {#if summary.skipped_existing.length}
        <p class="text-fg-muted">Skipped because they already exist: {summary.skipped_existing.join(", ")}</p>
      {/if}
      {#each summary.warnings as w, i (i)}
        <p class="flex gap-2 text-xs text-warning"><TriangleAlert size={13} class="mt-0.5 shrink-0" /> {w}</p>
      {/each}
    </div>
  {:else if loading}
    <div class="flex items-center justify-center gap-2 py-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading config…</div>
  {:else}
    <div class="space-y-4">
      <div class="flex items-center gap-2">
        <div class="input flex-1 truncate font-mono text-xs">{preview?.path ?? "~/.ssh/config"}</div>
        <button class="btn-ghost border border-line" onclick={chooseFile}><FileInput size={14} /> SSH config…</button>
        <button class="btn-ghost border border-line" onclick={chooseInventory}><FileInput size={14} /> Ansible inventory…</button>
      </div>

      {#if error}
        <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
      {/if}

      {#if preview}
        {#if preview.hosts.length === 0}
          <p class="py-6 text-center text-sm text-fg-muted">No importable hosts found. Wildcard-only entries are skipped.</p>
        {:else}
          <div class="max-h-72 overflow-y-auto rounded-md border border-line">
            <table class="w-full text-left text-xs">
              <thead class="sticky top-0 bg-panel text-fg-muted">
                <tr class="border-b border-line">
                  <th class="w-8 px-2 py-1.5">
                    <input
                      type="checkbox"
                      class="accent-[#7b61ff]"
                      aria-label="Select all"
                      checked={picked.size === preview.hosts.length - preview.existing.length}
                      onchange={(e) => {
                        const existing = new Set(preview!.existing);
                        picked = e.currentTarget.checked
                          ? new Set(preview!.hosts.filter((h) => !existing.has(h.alias)).map((h) => h.alias))
                          : new Set();
                      }}
                    />
                  </th>
                  <th class="px-2 py-1.5 font-medium">Host</th>
                  <th class="px-2 py-1.5 font-medium">Address</th>
                  <th class="px-2 py-1.5 font-medium">User</th>
                  <th class="px-2 py-1.5 font-medium">Key / jump</th>
                  {#if source === "ansible"}<th class="px-2 py-1.5 font-medium">Group</th>{/if}
                </tr>
              </thead>
              <tbody>
                {#each preview.hosts as h (h.alias)}
                  {@const exists = preview.existing.includes(h.alias)}
                  <tr class="border-b border-line/50 {exists ? 'opacity-50' : ''}">
                    <td class="px-2 py-1.5">
                      <input type="checkbox" class="accent-[#7b61ff]" disabled={exists} checked={picked.has(h.alias)} onchange={() => toggle(h.alias)} aria-label="Import {h.alias}" />
                    </td>
                    <td class="px-2 py-1.5 font-medium">{h.alias}{exists ? " (exists)" : ""}</td>
                    <td class="px-2 py-1.5 font-mono">{h.hostname}{h.port !== 22 ? `:${h.port}` : ""}</td>
                    <td class="px-2 py-1.5 font-mono">{h.user ?? "—"}</td>
                    <td class="max-w-48 truncate px-2 py-1.5 font-mono text-fg-muted" title={h.identity_file ?? ""}>
                      {h.identity_file ? h.identity_file.split(/[\\/]/).pop() : "agent"}{h.proxy_jump ? ` · via ${h.proxy_jump}` : ""}{h.proxy_command
                        ? " · ProxyCommand"
                        : ""}
                      {#if h.forwards?.length}
                        <span class="block" title={h.forwards.map(describeForward).join("\n")}>{h.forwards.length} forward(s)</span>
                      {/if}
                    </td>
                    {#if source === "ansible"}<td class="px-2 py-1.5 text-fg-muted">{h.group ?? "—"}</td>{/if}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>

          <div class="flex items-end gap-3">
            <div class="flex-1">
              <label class="label" for="imp-group">Put imported hosts in group</label>
              <input id="imp-group" class="input" bind:value={group} placeholder="Leave empty for top level" />
            </div>
          </div>

          {#if hasKeys}
            <fieldset class="space-y-1.5 rounded-md border border-line p-3 text-xs">
              <legend class="px-1 font-medium">Key files (IdentityFile)</legend>
              <label class="flex items-start gap-2">
                <input type="radio" class="mt-0.5 accent-[#7b61ff]" bind:group={keyImport} value="reference" />
                <span><strong>Reference by path</strong> (default). The key stays in <code>~/.ssh</code> on this computer; other computers need the same file.</span>
              </label>
              <label class="flex items-start gap-2">
                <input type="radio" class="mt-0.5 accent-[#7b61ff]" bind:group={keyImport} value="copy" />
                <span><strong>Copy into the vault.</strong> Keys are stored encrypted in the Key Manager and sync to all your devices. Encrypted keys stay passphrase-protected.</span>
              </label>
            </fieldset>
          {/if}
          {#if hasCommands}
            <p class="flex gap-2 text-xs text-fg-muted">
              <TriangleAlert size={13} class="mt-0.5 shrink-0 text-warning" />
              ProxyCommands are imported switched off. Nothing runs until you read and approve each one.
            </p>
          {/if}
        {/if}
        {#each preview.warnings as w, i (i)}
          <p class="flex gap-2 text-xs text-warning"><TriangleAlert size={13} class="mt-0.5 shrink-0" /> {w}</p>
        {/each}
      {/if}
    </div>
  {/if}
  {#snippet footer()}
    {#if summary}
      <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
    {:else}
      <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" disabled={!preview || picked.size === 0 || importing} onclick={doImport}>
        {#if importing}<Loader2 size={14} class="animate-spin" />{/if}
        Import {picked.size || ""} host{picked.size === 1 ? "" : "s"}
      </button>
    {/if}
  {/snippet}
</Modal>
