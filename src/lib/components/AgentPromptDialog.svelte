<script lang="ts">
  import { onMount } from "svelte";
  import { KeyRound } from "lucide-svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type AgentPrompt } from "$lib/types";

  // Signature requests from programs using the agent, one at a time.
  let queue = $state<AgentPrompt[]>([]);
  const q = $derived(queue[0]);

  onMount(() => {
    const un = api.agent.onPrompt((p) => {
      queue.push(p);
      // Bring the window's attention: the request came from another program.
      void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => getCurrentWindow().requestUserAttention(2)).catch(() => {});
    });
    return () => void un.then((f) => f());
  });

  async function answer(allow: boolean) {
    const cur = queue.shift();
    if (!cur) return;
    try {
      await api.agent.answer(cur.request_id, allow);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#if q}
  <div class="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-4" role="presentation">
    <div class="w-full max-w-md rounded-xl border border-line bg-panel p-6 shadow-2xl" role="alertdialog" aria-modal="true" aria-labelledby="ag-title">
      <div class="mb-3 flex items-start gap-3">
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-accent/15 text-accent"><KeyRound size={20} /></div>
        <div>
          <h2 id="ag-title" class="text-sm font-semibold">Use "{q.key_name}"?</h2>
          {#if q.origin}
            <p class="mt-1 text-xs text-fg-muted">
              <strong class="text-fg">{q.origin}</strong> asked to sign with this key through agent forwarding. Allow it only
              if you just ran something there that needs it (for example <code>git pull</code>). Anyone with root on that
              server could make the same request.
            </p>
          {:else}
            <p class="mt-1 text-xs text-fg-muted">
              A program on this computer (for example <code>ssh</code> or <code>git</code>) asked the SSHVault agent to sign
              with this key. Allow it only if you just started something that needs it.
            </p>
          {/if}
        </div>
      </div>
      <div class="rounded-md border border-line bg-base p-2 font-mono text-[11px] break-all">{q.fingerprint}</div>
      {#if queue.length > 1}<p class="mt-2 text-xs text-fg-muted">{queue.length - 1} more waiting.</p>{/if}
      <div class="mt-5 flex justify-end gap-2">
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn-secondary" autofocus onclick={() => answer(false)}>Deny</button>
        <button class="btn-primary" onclick={() => answer(true)}>Allow once</button>
      </div>
    </div>
  </div>
{/if}
