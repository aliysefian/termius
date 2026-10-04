<script lang="ts">
  import { Loader2, LockKeyhole } from "lucide-svelte";
  import { secrets } from "$lib/secrets.svelte";
  import { pushLayer } from "$lib/layers";

  let password = $state("");
  const p = $derived(secrets.prompt);

  // A new request (e.g. after a wrong password) clears the field.
  $effect(() => {
    if (p && !p.busy) password = "";
  });

  // Escape cancels this prompt only; the form that asked stays open.
  $effect(() => {
    if (!p) return;
    return pushLayer(() => secrets.cancel());
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!password) return;
    secrets.submit(password);
  }
</script>

{#if p}
  <div class="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4" role="presentation">
    <div class="w-full max-w-sm" role="dialog" aria-modal="true" aria-label="Master password">
    <form onsubmit={submit} class="space-y-4 rounded-xl border border-line bg-panel p-6 shadow-2xl">
      <div class="flex items-center gap-3">
        <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-accent"><LockKeyhole size={18} /></div>
        <div>
          <div class="text-sm font-semibold">Confirm master password</div>
          <div class="text-xs text-fg-muted">{p.reason}</div>
        </div>
      </div>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input" type="password" bind:value={password} autocomplete="current-password" autofocus disabled={p.busy} aria-label="Master password" />
      {#if p.error}
        <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{p.error}</p>
      {/if}
      <p class="text-xs text-fg-muted">{p.note}</p>
      <div class="flex justify-end gap-2">
        <button class="btn-ghost" type="button" onclick={() => secrets.cancel()}>Cancel</button>
        <button class="btn-primary" type="submit" disabled={p.busy || !password}>
          {#if p.busy}<Loader2 size={14} class="animate-spin" /> Checking…{:else}{p.action}{/if}
        </button>
      </div>
    </form>
    </div>
  </div>
{/if}
