<script lang="ts">
  import { onMount } from "svelte";
  import { ShieldAlert, ShieldQuestion } from "lucide-svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type HostKeyPrompt } from "$lib/types";

  // Questions from the SSH engine, answered one at a time.
  let queue = $state<HostKeyPrompt[]>([]);
  let details = $state(false);
  let typed = $state("");
  const q = $derived(queue[0]);
  const changed = $derived(!!q?.previous);
  const where = $derived(q ? (q.port === 22 ? q.host : `${q.host}:${q.port}`) : "");

  onMount(() => {
    const un = api.knownHosts.onPrompt((p) => queue.push(p));
    return () => void un.then((f) => f());
  });

  async function answer(trust: boolean) {
    const cur = queue.shift();
    details = false;
    typed = "";
    if (!cur) return;
    try {
      await api.knownHosts.answer(cur.request_id, trust);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#if q}
  <div class="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 p-4" role="presentation">
    <div
      class="w-full max-w-lg rounded-xl border bg-panel p-6 shadow-2xl {changed ? 'border-danger/60' : 'border-line'}"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="hk-title"
    >
      <div class="mb-4 flex items-start gap-3">
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg {changed ? 'bg-danger/15 text-danger' : 'bg-accent/15 text-accent'}">
          {#if changed}<ShieldAlert size={20} />{:else}<ShieldQuestion size={20} />{/if}
        </div>
        <div>
          <h2 id="hk-title" class="text-sm font-semibold {changed ? 'text-danger' : ''}">
            {changed ? "HOST KEY CHANGED" : "Unknown host"}: {where}
          </h2>
          <p class="mt-1 text-xs text-fg-muted">
            {#if changed}
              The server's identity is different from the one trusted before. This happens when a server is reinstalled or
              its keys are rotated, but it's also exactly what a man-in-the-middle attack looks like. Don't continue unless
              you know why it changed.
            {:else}
              You haven't connected to this server before. Check that the fingerprint matches the one your administrator or
              hosting provider shows (for example with <code>ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub</code> on the server).
            {/if}
          </p>
        </div>
      </div>

      <div class="space-y-2 rounded-md border border-line bg-base p-3 font-mono text-xs">
        {#if changed && q.previous}
          <div>
            <div class="text-[11px] uppercase tracking-wide text-fg-muted">Previously trusted</div>
            <div class="break-all">{q.previous.algorithm} {q.previous.fingerprint || "(unknown)"}</div>
          </div>
        {/if}
        <div>
          <div class="text-[11px] uppercase tracking-wide text-fg-muted">{changed ? "Now presented" : "Fingerprint"}</div>
          <div class="break-all {changed ? 'text-danger' : ''}">{q.algorithm} {q.fingerprint}</div>
        </div>
      </div>

      {#if details}
        <ul class="mt-3 list-disc space-y-1 pl-5 text-xs text-fg-muted">
          <li>Trusted keys are stored encrypted in the vault, so every device you use agrees on them.</li>
          <li>{changed ? "Replacing keeps the old fingerprint in the host's history (Known Hosts screen)." : "Once trusted, you won't be asked again unless the key changes."}</li>
          <li>Cancelling stops the connection; nothing is sent to the server.</li>
          <li>This question expires after 5 minutes.</li>
        </ul>
      {/if}

      {#if changed}
        <label class="mt-4 block text-xs">
          <span class="text-fg-muted">To replace the trusted key, type <strong>replace</strong>:</span>
          <input class="input mt-1" bind:value={typed} autocomplete="off" spellcheck="false" />
        </label>
      {/if}

      <div class="mt-5 flex items-center justify-between">
        <button class="btn-ghost text-xs" onclick={() => (details = !details)}>{details ? "Hide details" : "Details"}</button>
        <div class="flex gap-2">
          <!-- svelte-ignore a11y_autofocus -->
          <button class="btn-primary" autofocus onclick={() => answer(false)}>Cancel</button>
          {#if changed}
            <button class="btn-ghost border border-danger/50 text-danger" disabled={typed.trim().toLowerCase() !== "replace"} onclick={() => answer(true)}>
              Replace key
            </button>
          {:else}
            <button class="btn-secondary" onclick={() => answer(true)}>Trust and connect</button>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}
