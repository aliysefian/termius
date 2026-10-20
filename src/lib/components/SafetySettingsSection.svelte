<script lang="ts">
  // Settings → Safety: the Config Time-Machine's switches. Local to this computer (it decides what this app reads
  // from servers and keeps here), so it is not part of the vault's shared safety settings.
  import { History, Trash2 } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { parsePatterns } from "$lib/confighistory/terminalEditInterceptor";
  import { formatBytes, snapshotVault } from "$lib/confighistory/snapshotVaultStore.svelte";
  import { DEFAULT_PREFS, settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { onMount } from "svelte";

  const ENVS = [
    { value: "production", label: "Production (PROD)" },
    { value: "staging", label: "Staging (STAGE)" },
    { value: "development", label: "Development (DEV)" },
    { value: "", label: "No environment set" },
  ];
  const RETENTION = [
    { v: 7, label: "7 days" },
    { v: 30, label: "30 days" },
    { v: 90, label: "90 days" },
    { v: 0, label: "Forever" },
  ];
  const SIZES = [
    { kb: 256, label: "256 KB" },
    { kb: 1024, label: "1 MB" },
    { kb: 2048, label: "2 MB" },
    { kb: 10240, label: "10 MB" },
  ];

  let patterns = $state(settings.prefs.cfgPatterns.join("\n"));

  onMount(() => void snapshotVault.refresh());

  function toggleEnv(value: string, on: boolean) {
    const rest = settings.prefs.cfgEnvs.filter((e) => e !== value);
    settings.prefs.cfgEnvs = on ? [...rest, value] : rest;
  }

  function commitPatterns() {
    const list = parsePatterns(patterns);
    settings.prefs.cfgPatterns = list.length ? list : [...DEFAULT_PREFS.cfgPatterns];
    patterns = settings.prefs.cfgPatterns.join("\n");
  }

  async function clearVault() {
    const { snapshots, bytes } = snapshotVault.usage;
    if (!(await ask(`Delete ${snapshots} saved ${snapshots === 1 ? "version" : "versions"} (${formatBytes(bytes)}) from this computer? Nothing on any server changes.`, { title: "Clear snapshot vault", confirm: "Clear", danger: true }))) return;
    await snapshotVault.clear();
  }
</script>

<div class="space-y-3 rounded-lg border border-line bg-panel/60 p-4" data-testid="config-timemachine-settings">
  <h3 class="flex items-center gap-2 text-sm font-semibold"><History size={14} class="text-accent" /> Configuration Time-Machine (Auto-Backup)</h3>

  <label class="flex items-start gap-3 text-sm">
    <input type="checkbox" class="accent-input mt-1" bind:checked={settings.prefs.cfgEnabled} />
    <span>
      <span class="font-medium">Enable Automatic Config Time-Machine</span>
      <span class="block text-xs text-fg-muted">Silently snapshot remote configuration files to this computer, encrypted with your vault key, before edits via SFTP or terminal editors (<code class="font-mono">vim</code>, <code class="font-mono">nano</code>, <code class="font-mono">sed -i</code>, <code class="font-mono">tee</code>). The file is read over a separate connection channel, so your terminal is never held up. Nothing is synced or uploaded.</span>
    </span>
  </label>

  <fieldset class="space-y-1.5 border-t border-line pt-3 {settings.prefs.cfgEnabled ? '' : 'opacity-50'}" disabled={!settings.prefs.cfgEnabled}>
    <legend class="label">Hosts covered</legend>
    <div class="flex flex-wrap gap-2">
      {#each ENVS as e (e.value)}
        <label class="flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs {settings.prefs.cfgEnvs.includes(e.value) ? 'border-accent/60 bg-accent/10' : 'border-line text-fg-muted'}">
          <input type="checkbox" class="accent-input" checked={settings.prefs.cfgEnvs.includes(e.value)} onchange={(ev) => toggleEnv(e.value, ev.currentTarget.checked)} /> {e.label}
        </label>
      {/each}
    </div>

    <label class="label mt-3" for="cfg-patterns">Monitored paths and glob patterns</label>
    <textarea id="cfg-patterns" class="input min-h-24 font-mono text-xs" spellcheck="false" bind:value={patterns} onblur={commitPatterns}></textarea>
    <p class="text-xs text-fg-muted">One per line. <code class="font-mono">*</code> stays in a folder, <code class="font-mono">**</code> crosses folders. Only files that match, and that a command or the editor is about to change, are kept.</p>

    <div class="mt-3 flex flex-wrap items-end gap-4">
      <div>
        <label class="label" for="cfg-ret">Keep versions for</label>
        <select id="cfg-ret" class="input py-1 text-xs" bind:value={settings.prefs.cfgRetentionDays}>{#each RETENTION as r (r.v)}<option value={r.v}>{r.label}</option>{/each}</select>
      </div>
      <div>
        <label class="label" for="cfg-size">Skip files larger than</label>
        <select id="cfg-size" class="input py-1 text-xs" bind:value={settings.prefs.cfgMaxKb}>
          {#each SIZES as s (s.kb)}<option value={s.kb}>{s.label}</option>{/each}
          {#if !SIZES.some((s) => s.kb === settings.prefs.cfgMaxKb)}<option value={settings.prefs.cfgMaxKb}>{settings.prefs.cfgMaxKb} KB</option>{/if}
        </select>
      </div>
      <label class="flex items-center gap-2 pb-1.5 text-xs"><input type="checkbox" class="accent-input" bind:checked={settings.prefs.cfgPostEdit} /> Also keep the file when the editor exits</label>
    </div>
  </fieldset>

  <div class="flex flex-wrap items-center gap-3 border-t border-line pt-3 text-xs">
    <span class="font-mono tabular-nums" data-testid="snapshot-usage">{formatBytes(snapshotVault.usage.bytes)} across {snapshotVault.usage.snapshots} {snapshotVault.usage.snapshots === 1 ? "snapshot" : "snapshots"}</span>
    <div class="flex-1"></div>
    <button class="btn-secondary px-2 py-1 text-xs" onclick={() => { const first = snapshotVault.files[0]; ui.configHistory = first ? { hostId: first.host_id } : null; if (!first) ui.notify("info", "Nothing has been kept yet."); }}><History size={12} /> Open history</button>
    <button class="btn-secondary px-2 py-1 text-xs text-danger" disabled={snapshotVault.usage.snapshots === 0} onclick={clearVault}><Trash2 size={12} /> Clear Snapshot Vault</button>
  </div>
</div>
