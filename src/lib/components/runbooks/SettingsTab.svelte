<script lang="ts">
  import { CATEGORIES, categoryChoices } from "$lib/runbooklib";
  import { setDefault, setTop } from "$lib/runbookdoc";
  import { formOf, strategyOf } from "$lib/runbookrun";
  import type { Strategy } from "$lib/runbook";
  import StrategyPicker from "./StrategyPicker.svelte";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  const m = $derived(rb.loaded.model);
  const d = $derived((m?.defaults ?? {}) as { timeout_secs?: number; retries?: number; retry_delay_secs?: number; on_error?: string; strategy?: Strategy; hosts?: string[]; notify?: string });
  const blocked = $derived(!m || rb.loaded.unsupported.length > 0);
  const choices = $derived(categoryChoices(rb.items));
  let custom = $state("");
  // svelte-ignore state_referenced_locally
  let form = $state(formOf(d.strategy ?? null));
  // The strategy form follows the document when it changes from outside (the YAML view, another runbook).
  $effect(() => {
    form = formOf(d.strategy ?? null);
  });

  const num = (v: string) => (v.trim() === "" || !Number.isFinite(Number(v)) ? undefined : Number(v));
  const top = (key: string, value: string) => rb.apply(setTop(rb.text, key, value.trim() === "" ? undefined : value.trim()));
  const def = (key: string, value: unknown) => rb.apply(setDefault(rb.text, key, value as never));
  const hostLabels = $derived((d.hosts ?? []).map((id) => rb.hosts.find((h) => h.id === id)?.label ?? "a host that no longer exists"));
</script>

<div class="mx-auto max-w-2xl space-y-6 p-4" data-testid="settings-tab">
  {#if blocked}<p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">{rb.loaded.unsupported[0] ?? "The document can't be read."} Edit these in the YAML view.</p>{/if}

  <section class="space-y-3" aria-labelledby="set-about">
    <h3 id="set-about" class="text-xs font-semibold uppercase tracking-wide text-fg-muted">About</h3>
    <label class="block text-xs"><span class="mb-1 block font-medium">Name</span>
      <input class="input h-8 w-full text-sm" value={m?.name ?? ""} disabled={blocked} onchange={(e) => top("name", e.currentTarget.value)} />
    </label>
    <label class="block text-xs"><span class="mb-1 block font-medium">Description</span>
      <textarea class="input w-full resize-y text-sm" rows="2" disabled={blocked} value={m?.description ?? ""} onchange={(e) => top("description", e.currentTarget.value)}></textarea>
    </label>
    <div class="flex flex-wrap items-end gap-3">
      <label class="block text-xs"><span class="mb-1 block font-medium">Category</span>
        <select class="input h-8 w-48 text-sm" disabled={blocked} value={m?.category || "Custom"} onchange={(e) => top("category", e.currentTarget.value === "Custom" ? "" : e.currentTarget.value)}>
          {#each choices as c (c)}<option value={c}>{c}</option>{/each}
          {#if m?.category && !choices.includes(m.category)}<option value={m.category}>{m.category}</option>{/if}
        </select>
      </label>
      <form class="flex items-end gap-2" onsubmit={(e) => { e.preventDefault(); if (custom.trim()) { top("category", custom); custom = ""; } }}>
        <label class="block text-xs"><span class="mb-1 block font-medium">Or your own</span><input class="input h-8 w-40 text-sm" placeholder="e.g. Team tools" bind:value={custom} disabled={blocked} maxlength="40" /></label>
        <button class="btn-secondary h-8 text-xs" type="submit" disabled={blocked || !custom.trim()}>Use it</button>
      </form>
    </div>
    <p class="text-[11px] text-fg-muted">The standard categories are {CATEGORIES.join(", ")}. Category only sorts the library.</p>
  </section>

  <section class="space-y-3 border-t border-line pt-5" aria-labelledby="set-steps">
    <h3 id="set-steps" class="text-xs font-semibold uppercase tracking-wide text-fg-muted">What a step does unless it says otherwise</h3>
    <div class="grid gap-3 sm:grid-cols-3">
      <label class="block text-xs"><span class="mb-1 block font-medium">Timeout (seconds)</span>
        <input class="input h-8 w-full text-sm" inputmode="numeric" placeholder="120" disabled={blocked} value={d.timeout_secs ?? ""} onchange={(e) => def("timeout_secs", num(e.currentTarget.value))} />
      </label>
      <label class="block text-xs"><span class="mb-1 block font-medium">Retries (0–5)</span>
        <input class="input h-8 w-full text-sm" inputmode="numeric" placeholder="none" disabled={blocked} value={d.retries ?? ""} onchange={(e) => def("retries", num(e.currentTarget.value))} />
      </label>
      <label class="block text-xs"><span class="mb-1 block font-medium">Between retries (s)</span>
        <input class="input h-8 w-full text-sm" inputmode="numeric" placeholder="5" disabled={blocked} value={d.retry_delay_secs ?? ""} onchange={(e) => def("retry_delay_secs", num(e.currentTarget.value))} />
      </label>
    </div>
    <label class="block text-xs"><span class="mb-1 block font-medium">When a step fails</span>
      <select class="input h-8 w-64 text-sm" disabled={blocked} value={d.on_error ?? ""} onchange={(e) => def("on_error", e.currentTarget.value || undefined)}>
        <option value="">Stop this host (the usual)</option><option value="continue">Carry on with the next step</option>
      </select>
    </label>
    {#if (d.retries ?? 0) > 0}<p class="text-[11px] text-warning">Every step that fails is run again, unless it says otherwise. Only use this when the steps are safe to repeat.</p>{/if}
    <p class="text-[11px] text-fg-muted">A step's own value always wins. A wait or a pause times itself and takes no retries.</p>
  </section>

  <section class="space-y-3 border-t border-line pt-5" aria-labelledby="set-run">
    <h3 id="set-run" class="text-xs font-semibold uppercase tracking-wide text-fg-muted">How it is usually run</h3>
    <StrategyPicker bind:form hosts={d.hosts?.length ?? 0} disabled={blocked} />
    <div class="flex gap-2">
      <button class="btn-secondary h-8 text-xs" disabled={blocked} onclick={() => def("strategy", form.kind === "parallel" ? undefined : strategyOf(form))}>Save as the default strategy</button>
      {#if d.strategy}<button class="btn-ghost h-8 text-xs" disabled={blocked} onclick={() => def("strategy", undefined)}>Clear it</button>{/if}
    </div>
    <div class="space-y-1.5 text-xs">
      <p><span class="font-medium">Default hosts:</span> {hostLabels.length ? hostLabels.join(", ") : "none"}</p>
      <div class="flex gap-2">
        <button class="btn-secondary h-8 text-xs" disabled={blocked || rb.chosen.length === 0} onclick={() => def("hosts", [...rb.chosen])}>Use the {rb.chosen.length} host{rb.chosen.length === 1 ? "" : "s"} chosen in the run panel</button>
        {#if d.hosts?.length}<button class="btn-ghost h-8 text-xs" disabled={blocked} onclick={() => def("hosts", undefined)}>Clear</button>{/if}
      </div>
      <p class="text-[11px] text-fg-muted">They are chosen again when you open the runbook. Opening it never runs it.</p>
    </div>
  </section>

  <section class="space-y-2 border-t border-line pt-5" aria-labelledby="set-notify">
    <h3 id="set-notify" class="text-xs font-semibold uppercase tracking-wide text-fg-muted">Notification</h3>
    <label class="block text-xs"><span class="mb-1 block font-medium">Tell me when a run ends</span>
      <select class="input h-8 w-64 text-sm" disabled={blocked} value={d.notify ?? "failure"} onchange={(e) => def("notify", e.currentTarget.value === "failure" ? undefined : e.currentTarget.value)}>
        <option value="failure">Only if something failed (the usual)</option><option value="always">Always</option><option value="never">Never</option>
      </select>
    </label>
    <p class="text-[11px] text-fg-muted">A message in this window, for runs started here. Telegram, Bale and email are for Monitoring alerts, not runbooks.</p>
  </section>
</div>
