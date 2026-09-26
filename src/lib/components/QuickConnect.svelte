<script lang="ts">
  import { Zap } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { parseAdhoc } from "$lib/ssh";
  import { ui } from "$lib/stores/ui.svelte";

  let { initial = "" }: { initial?: string } = $props();

  // svelte-ignore state_referenced_locally
  let address = $state(initial);
  let password = $state("");
  const parsed = $derived(parseAdhoc(address));

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!parsed) return;
    ui.modal = null;
    ui.openAdhoc({ ...parsed, password: password || undefined });
    password = "";
  }
</script>

<Modal title="Quick connect" onclose={() => (ui.modal = null)} width="max-w-md">
  <form id="quick-form" onsubmit={submit} class="space-y-4">
    <div>
      <label class="label" for="q-addr">Address</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input id="q-addr" class="input font-mono" placeholder="user@host:22" bind:value={address} autofocus required spellcheck="false" />
      {#if address && !parsed}
        <p class="mt-1 text-xs text-danger">Use the form user@host or user@host:port.</p>
      {/if}
    </div>
    <div>
      <label class="label" for="q-pw">Password</label>
      <input id="q-pw" class="input" type="password" bind:value={password} autocomplete="off" placeholder="Leave empty to use ssh-agent" />
    </div>
    <p class="text-xs text-fg-muted">Nothing is saved. To keep this server, add it as a host instead.</p>
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="quick-form" disabled={!parsed}><Zap size={14} /> Connect</button>
  {/snippet}
</Modal>
