<script lang="ts">
  import { BellRing, Pencil, Plus, Trash2 } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { CHANNEL_NAMES, METRICS, PRESETS, describeLimits, describeScope, ruleFromPreset, type AlertRule } from "$lib/monitoring";
  import { errorMessage, type VaultRecord } from "$lib/types";

  let busy = $state(false);
  const rules = $derived(monitoring.rules.filter((r) => r.data));

  async function run(f: () => Promise<unknown>) {
    busy = true;
    try {
      await f();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = false;
    }
  }
  async function remove(rec: VaultRecord<AlertRule>) {
    if (!(await ask(`Delete the rule "${rec.data?.name}"?`, { title: "Delete rule", confirm: "Delete", danger: true }))) return;
    await run(() => monitoring.deleteRule(rec.id, rec.rev));
  }
  const toggle = (rec: VaultRecord<AlertRule>) => rec.data && run(() => monitoring.saveRule(rec.id, rec.rev, { ...$state.snapshot(rec.data!), enabled: !rec.data!.enabled }));
  const line = (r: AlertRule) =>
    [describeLimits(r), `after ${r.attempts} in a row`, describeScope(r.scope, monitoring.targetName), r.channels.length ? `only ${r.channels.map((c) => CHANNEL_NAMES[c]).join(", ")}` : ""].filter(Boolean).join(" · ");
</script>

<div class="space-y-3">
  <p class="text-sm text-fg-muted">A rule says when a reading is a problem. A state changes only after several checks in a row agree, and you are told once when it changes, not on every check.</p>
  <div class="flex flex-wrap items-center gap-2">
    <span class="text-xs text-fg-muted">Add:</span>
    {#each PRESETS as p (p.name)}
      <button class="btn-secondary py-1 text-xs" disabled={busy} onclick={() => void run(() => monitoring.saveRule(null, null, ruleFromPreset(p.name)))}>{p.name}</button>
    {/each}
    <button class="btn-primary ml-auto" data-testid="rule-new" onclick={() => (ui.modal = { kind: "alert-rule", id: null })}><Plus size={14} /> New rule</button>
  </div>
  {#if rules.length === 0}
    <EmptyState icon={BellRing} text="No rules yet. Add one above to be told when something goes wrong." />
  {:else}
    <div class="divide-y divide-line rounded-xl border border-line bg-panel">
      {#each rules as rec (rec.id)}
        {@const r = rec.data!}
        <div class="flex items-center gap-3 px-4 py-2.5 {r.enabled ? '' : 'opacity-60'}">
          <input type="checkbox" class="accent-input" checked={r.enabled} disabled={busy} aria-label="Use {r.name}" onchange={() => void toggle(rec)} />
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-medium">{r.name}{#if r.name !== METRICS[r.metric].label} <span class="text-xs font-normal text-fg-muted">{METRICS[r.metric].label}</span>{/if}</div>
            <div class="truncate text-xs text-fg-muted">{line(r)}</div>
          </div>
          <button class="icon-btn" title="Edit" aria-label="Edit {r.name}" onclick={() => (ui.modal = { kind: "alert-rule", id: rec.id })}><Pencil size={14} /></button>
          <button class="icon-btn" title="Delete" aria-label="Delete {r.name}" onclick={() => void remove(rec)}><Trash2 size={14} /></button>
        </div>
      {/each}
    </div>
  {/if}
</div>
