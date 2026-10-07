<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Loader2 } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { BACKUP_SUFFIX, DETECT_SCRIPT, blockFor, installScript, parseDetect, removeScript, type Detected } from "$lib/integrationinstall";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  let { hostId }: { hostId: Uuid } = $props();

  const host = $derived(vaultStore.hostById.get(hostId)?.data);
  const production = $derived(envInfo(vaultStore.effectiveEnv(host)).value === "production");
  let session: Uuid | null = null;
  let found = $state<Detected | null>(null);
  let busy = $state(true);
  let error = $state<string | null>(null);
  let done = $state<string | null>(null);

  async function run(script: string): Promise<string> {
    if (!session) throw new Error("Not connected.");
    const r = await api.monitor.exec(session, script, 30);
    if (r.code !== 0) throw new Error((r.stderr || r.stdout).trim() || `The host answered with exit ${r.code}.`);
    return r.stdout;
  }

  onMount(async () => {
    try {
      session = await api.monitor.open(hostId);
      found = parseDetect(await run(DETECT_SCRIPT));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  });
  onDestroy(() => {
    if (session) void api.monitor.close(session).catch(() => {});
  });

  async function change(install: boolean) {
    if (!found?.shell || !host) return;
    // A startup file on a production host is not edited without typing its name.
    if (production && !(await ask(`${install ? "Add lines to" : "Remove lines from"} ${found.file} on ${host.label}? This is a production host.`, { title: "Production host", confirm: install ? "Install" : "Remove", danger: true, requireText: host.label }))) return;
    busy = true;
    error = null;
    try {
      const out = (await run(install ? installScript(found.shell) : removeScript(found.shell))).trim();
      found = parseDetect(await run(DETECT_SCRIPT));
      done =
        out === "DONE" ? `Added. A copy of the old file is ${found.file}${BACKUP_SUFFIX}. It takes effect in the next shell you open on this host.`
        : out === "GONE" ? `Removed. A copy of the file as it was is ${found.file}${BACKUP_SUFFIX}.`
        : out === "ALREADY" ? "It was already there." : "There was nothing to remove.";
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Shell integration{host ? ` on ${host.label}` : ''}" onclose={() => (ui.modal = null)} width="max-w-xl">
  <div class="space-y-3 text-sm">
    <p class="text-fg-muted">With it, SSHVault can tell where each command starts and ends: the current folder in the header, jumping between prompts, copying a command's output, notices when a long command finishes, and exact smart completion.</p>
    {#if busy && !found}
      <div class="flex items-center gap-2 py-4 text-fg-muted"><Loader2 size={15} class="animate-spin" /> Looking at the host…</div>
    {/if}
    {#if error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-danger whitespace-pre-wrap">{error}</p>{/if}
    {#if done}<p class="rounded-md border border-success/30 bg-success/10 px-3 py-2 text-success">{done}</p>{/if}
    {#if found}
      {#if !found.shell}
        <p>This host's login shell is <strong>{found.name || "unknown"}</strong>. Automatic setup works for bash, zsh and fish; for others, see the lines under Settings → Shell integration.</p>
      {:else}
        <p>Login shell: <strong>{found.shell}</strong>. The lines go into <code class="rounded bg-base px-1">{found.file}</code>
          {#if found.installed}and are <strong>already there</strong>.{:else}and nothing else changes; a copy of the file is kept next to it.{/if}</p>
        <div>
          <div class="mb-1 text-xs font-medium text-fg-muted">Exactly what is added</div>
          <pre class="max-h-56 overflow-auto rounded-md border border-line bg-base p-3 font-mono text-[11px] leading-5 whitespace-pre-wrap" data-testid="integration-block">{blockFor(found.shell)}</pre>
        </div>
      {/if}
    {/if}
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Close</button>
    {#if found?.shell}
      {#if found.installed}
        <button class="btn-secondary" disabled={busy} onclick={() => void change(false)}>Remove it</button>
      {:else}
        <button class="btn-primary" disabled={busy} onclick={() => void change(true)}>
          {#if busy}<Loader2 size={14} class="animate-spin" />{/if} Add it
        </button>
      {/if}
    {/if}
  {/snippet}
</Modal>
