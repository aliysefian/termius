<script lang="ts">
  import { onMount } from "svelte";
  import { ShieldAlert, TerminalSquare } from "lucide-svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type CliPrompt } from "$lib/types";

  let queue = $state<CliPrompt[]>([]);
  let trust = $state(false);
  const q = $derived(queue[0]);

  onMount(() => {
    const offPrompt = api.cli.onPrompt((p) => queue.push(p));
    // `sshvault connect web-01` opens the tab here.
    const offOpen = api.cli.onOpen(({ host_id, label }) => ui.openTerminal(host_id, label));
    return () => {
      void offPrompt.then((f) => f());
      void offOpen.then((f) => f());
    };
  });

  async function answer(allow: boolean) {
    const cur = queue.shift();
    const minutes = allow && trust ? 10 : null;
    trust = false;
    if (!cur) return;
    try {
      await api.cli.answer(cur.request_id, allow, minutes);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#if q}
  <div class="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-4" role="presentation">
    <div class="w-full max-w-lg rounded-xl border {q.production ? 'border-danger/50' : 'border-line'} bg-panel p-6 shadow-2xl" role="alertdialog" aria-modal="true" aria-labelledby="cli-title">
      <div class="mb-3 flex items-start gap-3">
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg {q.production ? 'bg-danger/15 text-danger' : 'bg-accent/15 text-accent'}">
          {#if q.production}<ShieldAlert size={20} />{:else}<TerminalSquare size={20} />{/if}
        </div>
        <div>
          <h2 id="cli-title" class="text-sm font-semibold">Run a command from the command line?</h2>
          <p class="mt-1 text-xs text-fg-muted">
            <code>sshvault run</code> on this computer wants to run this on {q.hosts.length} host{q.hosts.length === 1 ? "" : "s"}, using your
            saved credentials. Allow it only if you just typed it.
          </p>
        </div>
      </div>
      <pre class="max-h-32 overflow-auto rounded-md border border-line bg-base px-3 py-2 font-mono text-xs whitespace-pre-wrap">{q.command}</pre>
      <p class="mt-2 text-xs text-fg-muted">
        {q.hosts.slice(0, 12).join(", ")}{q.hosts.length > 12 ? `, and ${q.hosts.length - 12} more` : ""}
      </p>
      {#if q.production}
        <p class="mt-2 text-xs font-medium text-danger">{q.production} of them {q.production === 1 ? "is a production host" : "are production hosts"}.</p>
      {/if}
      <label class="mt-3 flex items-center gap-2 text-xs text-fg-muted">
        <input type="checkbox" class="accent-[#7b61ff]" bind:checked={trust} />
        Don't ask again for 10 minutes (production hosts always ask)
      </label>
      <div class="mt-5 flex justify-end gap-2">
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn-primary" autofocus onclick={() => answer(false)}>Deny</button>
        <button class="btn-ghost border {q.production ? 'border-danger/50 text-danger' : 'border-line'}" onclick={() => answer(true)}>Run it</button>
      </div>
    </div>
  </div>
{/if}
