<script lang="ts">
  import { Plus, TriangleAlert } from "lucide-svelte";
  import StepCard from "./StepCard.svelte";
  import StepEditor from "./StepEditor.svelte";
  import { KIND_ICON } from "./kindicon";
  import type { Obj, StepKind } from "$lib/runbookdoc";
  import { KINDS } from "$lib/runbookschema";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  const model = $derived(rb.loaded.model);
  const steps = $derived(model?.steps ?? []);
  const rollback = $derived(model?.rollback ?? []);
  const blocked = $derived(rb.loaded.unsupported);
  let adding = $state<{ rollback: boolean } | null>(null);
  let showRollback = $state(false);

  /** The check's messages about one step. */
  const problemsAt = (i: number, rollbackList: boolean) =>
    (rb.check?.problems ?? []).filter((p) => (rollbackList ? p.message.startsWith(`Rollback step ${i + 1}:`) : p.step === i)).map((p) => (rollbackList ? p.message.replace(`Rollback step ${i + 1}: `, "") : p.message));
  const general = $derived((rb.check?.problems ?? []).filter((p) => p.step === null && !p.message.startsWith("Rollback step")));

  function add(kind: StepKind) {
    rb.addStepOf(kind, undefined, adding?.rollback ?? false);
    adding = null;
  }

  $effect(() => {
    if (rollback.length > 0) showRollback = true;
  });
</script>

{#snippet picker(isRollback: boolean)}
  <div class="relative">
    <button class="btn-secondary h-8 w-full justify-center border-dashed text-xs" aria-expanded={adding?.rollback === isRollback} aria-haspopup="menu" onclick={() => (adding = adding?.rollback === isRollback ? null : { rollback: isRollback })} data-testid={isRollback ? "add-rollback-step" : "add-step"}><Plus size={13} /> Add {isRollback ? "rollback " : ""}step</button>
    {#if adding?.rollback === isRollback}
      <div class="mt-2 grid gap-1.5 rounded-xl border border-line bg-panel p-2 sm:grid-cols-2" role="menu" aria-label="Step types">
        {#each KINDS as k (k.kind)}
          {@const Icon = KIND_ICON[k.kind]}
          <button class="flex items-start gap-2.5 rounded-lg px-2.5 py-2 text-left hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none" role="menuitem" onclick={() => add(k.kind)} data-testid="add-{k.kind}">
            <Icon size={16} class="mt-0.5 shrink-0 text-accent" />
            <span><span class="block text-xs font-medium">{k.label}</span><span class="block text-[11px] text-fg-muted">{k.about}</span></span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

{#snippet list(items: Obj[], isRollback: boolean)}
  <ol class="space-y-2" aria-label={isRollback ? "Rollback steps" : "Steps"}>
    {#each items as step, i (i)}
      <li>
        <StepCard
          {step}
          index={i}
          total={items.length}
          rollback={isRollback}
          selected={rb.editing?.index === i && rb.editing.rollback === isRollback}
          problems={problemsAt(i, isRollback)}
          onselect={() => (rb.editing = rb.editing?.index === i && rb.editing.rollback === isRollback ? null : { index: i, rollback: isRollback })}
          onmove={(d) => rb.move(i, d, isRollback)}
          onduplicate={() => rb.duplicateStepAt(i, isRollback)}
          onremove={() => rb.removeStepAt(i, isRollback)}
          ontoggle={() => rb.toggleStep(i, isRollback)}
          ondrop={(from) => rb.move(from, i - from, isRollback)}
        />
        {#if rb.editing?.index === i && rb.editing.rollback === isRollback}
          <div class="mt-2"><StepEditor index={i} rollback={isRollback} problems={problemsAt(i, isRollback)} onclose={() => (rb.editing = null)} /></div>
        {/if}
      </li>
    {/each}
  </ol>
{/snippet}

<div class="mx-auto max-w-3xl space-y-4 p-4" data-testid="builder">
  {#if !model}
    <div class="rounded-lg border border-danger/30 bg-danger/10 p-4 text-sm" role="alert">
      <p class="flex items-center gap-2 font-medium text-danger"><TriangleAlert size={15} /> The Builder can't show this document.</p>
      <p class="mt-1 text-xs text-fg-muted">{rb.loaded.error ? `${rb.loaded.error.message}${rb.loaded.error.line ? ` (line ${rb.loaded.error.line})` : ""}` : "It isn't a runbook."} Nothing was changed. Fix it in the YAML view.</p>
      <button class="btn-secondary mt-3 text-xs" onclick={() => (rb.mode = "yaml")}>Open the YAML view</button>
    </div>
  {:else}
    {#if blocked.length}
      <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning" role="status" data-testid="builder-blocked">{blocked[0]} Steps can be read here but not changed; edit it in the YAML view. Nothing is lost.</p>
    {/if}
    {#if model.unknownKeys.length}
      <p class="rounded-md border border-line bg-panel px-3 py-2 text-xs text-fg-muted" data-testid="builder-unknown">The document has fields the Builder doesn't show ({model.unknownKeys.join(", ")}). They are kept as written; the check below says whether the runbook accepts them.</p>
    {/if}
    {#each general as p (p.message)}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{p.message}</p>{/each}

    <div class="flex items-center gap-2">
      <h3 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">Steps ({steps.length})</h3>
      <span class="text-[11px] text-fg-muted">Run in this order on every host. Alt+↑/↓ moves a step.</span>
    </div>
    {#if steps.length === 0}
      <p class="rounded-lg border border-dashed border-line px-4 py-6 text-center text-sm text-fg-muted">This runbook has no steps yet.</p>
    {:else if blocked.length}
      <ol class="space-y-2 opacity-80">{#each steps as s, i (i)}<li class="rounded-[10px] border border-line bg-panel px-3 py-2 text-sm">{i + 1}. {typeof s.name === "string" ? s.name : "Step"}</li>{/each}</ol>
    {:else}
      {@render list(steps, false)}
    {/if}
    {#if !blocked.length}{@render picker(false)}{/if}

    {#if !blocked.length}
      <section class="space-y-2 pt-2" aria-label="Rollback">
        <button class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-fg-muted hover:text-fg" aria-expanded={showRollback} onclick={() => (showRollback = !showRollback)}>Rollback steps ({rollback.length})</button>
        {#if showRollback}
          <p class="text-[11px] text-fg-muted">Run only on a host where the run fails, and only when "Roll back a host that fails" is on for that run. They change the host again.</p>
          {#if rollback.length}{@render list(rollback, true)}{/if}
          {@render picker(true)}
        {/if}
      </section>
    {/if}
  {/if}
</div>
