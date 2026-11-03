<script lang="ts">
  import { explainStrategy, MAX_CONCURRENCY, STRATEGIES, strategyProblem, type StrategyForm } from "$lib/runbookrun";

  let { form = $bindable(), hosts, disabled = false }: { form: StrategyForm; hosts: number; disabled?: boolean } = $props();

  const problem = $derived(strategyProblem(form, hosts));
  const about = $derived(STRATEGIES.find((s) => s.kind === form.kind)?.about ?? "");
</script>

<section aria-label="Execution strategy" class="space-y-2" data-testid="strategy">
  <label class="block text-xs">
    <span class="mb-1 block font-medium">How hosts are taken</span>
    <select class="input h-8 w-full text-xs" bind:value={form.kind} {disabled} data-testid="strategy-kind">
      {#each STRATEGIES as s (s.kind)}<option value={s.kind}>{s.label}</option>{/each}
    </select>
  </label>
  <p class="text-[11px] text-fg-muted">{about}</p>

  {#if form.kind === "concurrent"}
    <label class="flex items-center gap-2 text-xs">Hosts at a time
      <input class="input h-7 w-20 text-xs" type="number" min="1" max={MAX_CONCURRENCY} bind:value={form.limit} {disabled} aria-label="Hosts at a time" />
    </label>
  {:else if form.kind === "rolling"}
    <div class="flex flex-wrap items-center gap-3 text-xs">
      <label class="flex items-center gap-2">Batch size
        <input class="input h-7 w-16 text-xs" type="number" min="1" max={MAX_CONCURRENCY} bind:value={form.batch} {disabled} aria-label="Batch size" />
      </label>
      <label class="flex items-center gap-2">Max concurrency
        <input class="input h-7 w-16 text-xs" type="number" min="1" max={form.batch} bind:value={form.concurrency} {disabled} aria-label="Maximum concurrency inside a batch" />
      </label>
    </div>
  {/if}
  {#if form.kind !== "parallel"}
    <label class="flex items-start gap-2 text-xs">
      <input type="checkbox" class="mt-0.5 accent-input" bind:checked={form.stopOnFailure} {disabled} />
      <span>{form.kind === "rolling" ? "Stop after a failed batch" : form.kind === "sequential" ? "Stop at the first failure" : "Stop everything at the first failure"}</span>
    </label>
  {/if}
  {#if problem}<p class="text-xs text-danger" role="alert">{problem}</p>{/if}
  <p class="rounded-md bg-base/50 px-2 py-1.5 text-[11px] text-fg-muted" data-testid="strategy-explained">{explainStrategy(form, hosts)}</p>
</section>
