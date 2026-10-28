<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { CHANNEL_NAMES, METRICS, checkRule, ruleFromPreset, splitList, type AlertRule, type ChannelKind, type Metric, type Scope } from "$lib/monitoring";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // Remounted via {#key} when id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existingRec = id ? monitoring.rules.find((r) => r.id === id) : undefined;
  const baseRev = existingRec?.rev ?? null;
  const base: AlertRule = existingRec?.data ? structuredClone($state.snapshot(existingRec.data)) : ruleFromPreset("Ping packet loss");

  let name = $state(base.name);
  let metric = $state<Metric>(base.metric);
  let op = $state(base.op);
  let warn = $state<number | null>(base.warn);
  let crit = $state<number | null>(base.crit);
  let attempts = $state(base.attempts);
  let enabled = $state(base.enabled);
  let scopeType = $state<Scope["type"]>(base.scope.type);
  let scopeTags = $state(base.scope.type === "tags" ? base.scope.tags.join(", ") : "");
  let scopeTarget = $state(base.scope.type === "target" ? base.scope.id : "");
  let channels = $state<ChannelKind[]>([...base.channels]);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const targetChoices = $derived(
    monitoring.targets.filter((t) => t.data).map((t) => ({ value: t.id, label: t.data!.label, detail: t.data!.address })),
  );
  const unit = $derived(METRICS[metric].unit.trim() ? ` (${METRICS[metric].unit.trim()})` : "");
  const fingerprint = () => JSON.stringify([name, metric, op, warn, crit, attempts, enabled, scopeType, scopeTags, scopeTarget, channels]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);

  // Empty number boxes arrive as null or ""; both mean "no limit".
  const limit = (v: number | null | string) => (v === null || v === "" || Number.isNaN(Number(v)) ? null : Number(v));

  function toggle(k: ChannelKind) {
    channels = channels.includes(k) ? channels.filter((x) => x !== k) : [...channels, k];
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    const scope: Scope = scopeType === "tags" ? { type: "tags", tags: splitList(scopeTags) } : scopeType === "target" ? { type: "target", id: scopeTarget } : { type: "all" };
    const rule: AlertRule = { name: name.trim(), enabled, metric, op, warn: limit(warn), crit: limit(crit), attempts: Number(attempts), scope, channels };
    const problem = checkRule(rule) ?? (scope.type === "target" && !scope.id ? "Choose the host this rule is for." : null);
    if (problem) {
      error = problem;
      return;
    }
    busy = true;
    try {
      await monitoring.saveRule(id, baseRev, rule);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit alert rule" : "New alert rule"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null} width="max-w-xl">
  <form id="rule-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="r-name">Name</label>
        <input id="r-name" class="input" bind:value={name} required />
      </div>
      <div>
        <label class="label" for="r-metric">Reading</label>
        <select id="r-metric" class="input" bind:value={metric}>
          {#each Object.entries(METRICS) as [k, m] (k)}<option value={k}>{m.label}</option>{/each}
        </select>
      </div>
    </div>
    <p class="-mt-2 text-xs text-fg-muted">{METRICS[metric].hint}</p>

      <div class="grid grid-cols-3 gap-3">
        <div>
          <label class="label" for="r-op">Problem when it is</label>
          <select id="r-op" class="input" bind:value={op}>
            <option value="gt">above</option>
            <option value="lt">below</option>
          </select>
        </div>
        <div>
          <label class="label" for="r-warn">Warning limit{unit}</label>
          <input id="r-warn" class="input font-mono" type="number" step="any" bind:value={warn} placeholder="none" />
        </div>
        <div>
          <label class="label" for="r-crit">Critical limit{unit}</label>
          <input id="r-crit" class="input font-mono" type="number" step="any" bind:value={crit} placeholder="none" />
        </div>
      </div>

    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="r-attempts">Checks in a row before it counts</label>
        <input id="r-attempts" class="input font-mono" type="number" min="1" max="20" bind:value={attempts} required />
      </div>
      <div>
        <label class="label" for="r-scope">Applies to</label>
        <select id="r-scope" class="input" bind:value={scopeType}>
          <option value="all">All watched hosts</option>
          <option value="tags">Hosts with a tag</option>
          <option value="target">One host</option>
        </select>
      </div>
    </div>
    {#if scopeType === "tags"}
      <div>
        <label class="label" for="r-tags">Tags</label>
        <input id="r-tags" class="input" bind:value={scopeTags} placeholder="prod, web" />
      </div>
    {:else if scopeType === "target"}
      <div>
        <label class="label" for="r-target">Host</label>
        <Combobox id="r-target" options={targetChoices} bind:value={scopeTarget} placeholder="Search watched hosts…" emptyText="No watched host matches" />
      </div>
    {/if}

    <div>
      <span class="label">Send to</span>
      <div class="flex flex-wrap gap-4 text-sm">
        {#each Object.entries(CHANNEL_NAMES) as [k, text] (k)}
          <label class="flex items-center gap-2"><input type="checkbox" class="accent-input" checked={channels.includes(k as ChannelKind)} onchange={() => toggle(k as ChannelKind)} /> {text}</label>
        {/each}
      </div>
      <p class="mt-1 text-xs text-fg-muted">Leave all unticked to use every channel that is switched on in Settings.</p>
    </div>

    <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={enabled} /> Use this rule</label>

    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="rule-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
