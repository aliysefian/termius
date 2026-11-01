<script lang="ts">
  import { Loader2 } from "lucide-svelte";
  import TunnelFields from "./TunnelFields.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { tunnels } from "$lib/stores/tunnels.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { draftFromRule, needsRestart, ruleFromDraft, validateTunnel, type TunnelRow } from "$lib/tunnels";
  import { errorMessage } from "$lib/types";

  let { row }: { row: TunnelRow } = $props();

  // The draft follows the tunnel that is selected; edits to it are kept until saved or discarded.
  // svelte-ignore state_referenced_locally
  let draft = $state(draftFromRule(row.rule));
  // svelte-ignore state_referenced_locally
  let shownFor = row.id;
  // svelte-ignore state_referenced_locally
  let shownJson = JSON.stringify(draftFromRule(row.rule));
  $effect(() => {
    const now = JSON.stringify(draftFromRule(row.rule));
    if (row.id !== shownFor) {
      draft = draftFromRule(row.rule);
      shownFor = row.id;
      shownJson = now;
      serverError = null;
    } else if (now !== shownJson && JSON.stringify(draft) === shownJson) {
      // Changed elsewhere (another device synced it) while nothing was being edited here.
      draft = draftFromRule(row.rule);
      shownJson = now;
    }
  });

  let busy = $state(false);
  let serverError = $state<string | null>(null);
  const dirty = $derived(JSON.stringify(draft) !== shownJson);
  const errors = $derived(
    validateTunnel(
      ruleFromDraft(draft),
      new Set(vaultStore.hosts.map((h) => h.id)),
      row.id,
      vaultStore.forwards.filter((f) => f.data).map((f) => ({ id: f.id, rule: f.data! })),
    ),
  );
  const invalid = $derived(Object.keys(errors).length > 0);
  const running = $derived(row.status === "active" || row.status === "connecting");
  const restart = $derived(running && needsRestart(row.rule, ruleFromDraft(draft)));

  async function save(restartNow: boolean) {
    if (invalid || busy) return;
    serverError = null;
    if (restart) {
      const text = restartNow
        ? `"${row.rule.label}" is running. These changes take effect after a restart.\n\nSave and restart it now? It is interrupted for a moment, and connections through it are closed.`
        : `"${row.rule.label}" is running. These changes are saved now but only take effect after the next restart.\n\nSave without restarting?`;
      if (!(await ask(text, { title: restartNow ? "Save and restart" : "Save without restarting", confirm: restartNow ? "Save and restart" : "Save" }))) return;
    }
    busy = true;
    try {
      await vaultStore.saveForward(row.id, ruleFromDraft(draft));
      shownJson = JSON.stringify(draft);
      ui.notify("info", `Saved "${draft.label.trim()}".`);
      if (restart && restartNow) {
        const err = await tunnels.restart(row.id);
        if (err) serverError = `Saved, but the restart failed: ${err}`;
      }
    } catch (e) {
      serverError = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function discard() {
    draft = draftFromRule(row.rule);
    shownJson = JSON.stringify(draft);
    serverError = null;
  }
</script>

<form class="space-y-4" onsubmit={(e) => { e.preventDefault(); void save(restart); }} aria-label="Tunnel configuration">
  <TunnelFields bind:draft {errors} idPrefix="cfg" />

  {#if restart}
    <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning" role="status">This tunnel is running. Changes to its route take effect after a restart.</p>
  {/if}
  {#if serverError}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{serverError}</p>{/if}

  <div class="sticky bottom-0 -mx-4 flex flex-wrap items-center justify-end gap-2 border-t border-line bg-panel px-4 py-3">
    <button type="button" class="btn-ghost" disabled={!dirty || busy} onclick={discard}>Discard</button>
    {#if restart}
      <button type="button" class="btn-secondary" disabled={!dirty || invalid || busy} onclick={() => void save(false)}>Save, restart later</button>
    {/if}
    <button type="submit" class="btn-primary" disabled={!dirty || invalid || busy}>
      {#if busy}<Loader2 size={14} class="animate-spin" />{/if}
      {restart ? "Save and restart" : "Save changes"}
    </button>
  </div>
</form>
