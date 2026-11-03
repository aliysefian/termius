<script lang="ts">
  import { CheckCheck, CircleStop, Eye, FolderOpen, FileUp, Loader2, Play, PlugZap, ShieldAlert, X } from "lucide-svelte";
  import HostPicker from "./HostPicker.svelte";
  import StrategyPicker from "./StrategyPicker.svelte";
  import ValidationList from "./ValidationList.svelte";
  import { canStart, RUN_LABEL, valueProblem } from "$lib/runbookrun";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let { oncollapse, onstep }: { oncollapse?: () => void; onstep?: (step: number) => void } = $props();

  const params = $derived(rb.check?.params ?? []);
  const busy = $derived(!canStart(rb.phase));
  const going = $derived(rb.phase === "running" || rb.phase === "starting");
  const hasProblems = $derived((rb.check?.problems.length ?? 0) > 0);
  const destructive = $derived(rb.loaded.model ? rb.destructive() : []);
  const production = $derived(rb.chosenHosts.filter((h) => h.production).length);
  const rollbackSteps = $derived(rb.check?.rollback_steps ?? 0);
</script>

<aside class="flex h-full min-h-0 flex-col bg-panel" aria-label="Execution" data-testid="execution-panel">
  <header class="flex items-center gap-2 border-b border-line px-3 py-2.5">
    <h2 class="flex-1 text-xs font-semibold uppercase tracking-wide text-fg-muted">Run this runbook</h2>
    {#if oncollapse}<button class="icon-btn h-6 w-6" title="Hide this panel" aria-label="Hide the execution panel" onclick={oncollapse}><X size={14} /></button>{/if}
  </header>

  <div class="min-h-0 flex-1 space-y-5 overflow-y-auto p-3">
    {#if !rb.text}
      <p class="py-6 text-center text-xs text-fg-muted">Choose or create a runbook to run it.</p>
    {:else}
      <section>
        <h3 class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Target hosts</h3>
        <HostPicker hosts={rb.hosts} chosen={rb.chosen} onchange={(ids) => { rb.chosen = ids; rb.connected = null; }} disabled={busy} />
      </section>

      <section>
        <h3 class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Execution strategy</h3>
        <StrategyPicker bind:form={rb.strategy} hosts={rb.chosen.length} disabled={busy} />
      </section>

      {#if params.length}
        <section aria-label="Values for this run" data-testid="run-values">
          <h3 class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Values for this run</h3>
          <div class="space-y-2.5">
            {#each params as p (p.name)}
              {@const bad = p.kind === "file" || p.kind === "dir" ? null : valueProblem(p, rb.values[p.name] ?? "")}
              <div>
                <label class="mb-0.5 block text-xs" for="rv-{p.name}">{p.label || p.name}{#if p.optional}<span class="text-fg-muted"> (optional)</span>{/if}</label>
                {#if p.kind === "file" || p.kind === "dir"}
                  <div class="flex items-center gap-2">
                    <button id="rv-{p.name}" class="btn-secondary h-7 shrink-0 px-2 text-xs" disabled={busy} onclick={() => rb.chooseFile(p.name, p.kind as "file" | "dir")}>
                      {#if p.kind === "dir"}<FolderOpen size={12} /> Choose a folder…{:else}<FileUp size={12} /> Choose a file…{/if}
                    </button>
                    <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-muted" title={rb.chosenPath(p.name, p.kind)}>{rb.chosenPath(p.name, p.kind) || "none chosen"}</span>
                  </div>
                {:else if p.kind === "secret"}
                  <input id="rv-{p.name}" type="password" class="input h-8 w-full font-mono text-xs" bind:value={rb.values[p.name]} autocomplete="off" spellcheck="false" disabled={busy} data-testid="run-secret" />
                  <p class="mt-0.5 text-[11px] text-fg-muted">Asked each time. Hidden in the log and the record, and never saved.</p>
                {:else if p.kind === "boolean"}
                  <select id="rv-{p.name}" class="input h-8 w-full text-xs" bind:value={rb.values[p.name]} disabled={busy}>
                    <option value="">{p.default ? `Default (${p.default})` : "Choose…"}</option><option value="true">true</option><option value="false">false</option>
                  </select>
                {:else if p.choices.length}
                  <select id="rv-{p.name}" class="input h-8 w-full text-xs" bind:value={rb.values[p.name]} disabled={busy}>{#each p.choices as c (c)}<option value={c}>{c}</option>{/each}</select>
                {:else}
                  <input id="rv-{p.name}" class="input h-8 w-full font-mono text-xs {bad ? 'border-danger' : ''}" type={p.kind === "number" ? "text" : "text"} inputmode={p.kind === "number" ? "decimal" : undefined} bind:value={rb.values[p.name]} spellcheck="false" autocomplete="off" aria-invalid={!!bad} disabled={busy} />
                {/if}
                {#if p.description}<p class="mt-0.5 text-[11px] text-fg-muted">{p.description}</p>{/if}
                {#if bad}<p class="mt-0.5 text-[11px] text-danger" role="alert">{bad}</p>{/if}
              </div>
            {/each}
          </div>
        </section>
      {/if}

      <section aria-label="Safety options">
        <h3 class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Safety options</h3>
        <div class="space-y-2 text-xs">
          {#if rollbackSteps > 0}
            <label class="flex items-start gap-2" data-testid="rollback-option">
              <input type="checkbox" class="mt-0.5 accent-input" bind:checked={rb.rollbackOn} disabled={busy} />
              <span><span class="font-medium">Roll back a host that fails</span><span class="block text-[11px] text-fg-muted">Runs its {rollbackSteps} rollback step{rollbackSteps === 1 ? "" : "s"} on that host. They change it again, so read them in the plan first.</span></span>
            </label>
          {/if}
          {#if production}
            <p class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 px-2 py-1.5 text-danger"><ShieldAlert size={13} class="mt-0.5 shrink-0" /> {production} production host{production === 1 ? "" : "s"}. You will be asked to type the runbook's name before it starts.</p>
          {/if}
          {#if destructive.length}
            <p class="rounded-md border border-warning/30 bg-warning/10 px-2 py-1.5 text-warning">Matches a destructive-command pattern: {destructive.join(", ")}. You will be asked to confirm.</p>
          {/if}
          <p class="text-[11px] text-fg-muted">Commands run with the SSH user's own rights. Nothing is elevated unless a step says sudo. Cancelling closes the connection and hangs up the command, which ends most remote commands but is not a guarantee.</p>
        </div>
      </section>

      <section aria-label="Validation">
        <h3 class="mb-2 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Validation
          <button class="ml-auto inline-flex items-center gap-1 rounded text-[11px] font-normal normal-case tracking-normal text-accent hover:underline disabled:opacity-40" disabled={busy || rb.chosen.length === 0 || rb.testing} onclick={() => rb.testConnections()} data-testid="test-connections">
            {#if rb.testing}<Loader2 size={11} class="animate-spin" />{:else}<PlugZap size={11} />{/if} Test connections
          </button>
        </h3>
        {#if rb.report}
          <ValidationList items={rb.report} {onstep} />
        {:else}
          <p class="text-xs text-fg-muted">{hasProblems ? `${rb.check?.problems.length} problem${rb.check?.problems.length === 1 ? "" : "s"} in the document. ` : ""}Validate to check the document, the values and the hosts.</p>
        {/if}
      </section>
    {/if}
  </div>

  <footer class="space-y-2 border-t border-line p-3">
    <div class="flex gap-2">
      <button class="btn-secondary h-8 flex-1 justify-center text-xs" disabled={!rb.text || busy} onclick={() => void rb.validateNow()} data-testid="validate" title="Check the document, the values and the hosts (Ctrl+Enter)"><CheckCheck size={13} /> Validate</button>
      <button class="btn-secondary h-8 flex-1 justify-center text-xs" disabled={!rb.text || busy || hasProblems || rb.chosen.length === 0} onclick={async () => { await rb.preview(); rb.previewOpen = true; }} data-testid="preview-plan" title="Show the commands each host would get, without running anything"><Eye size={13} /> Preview plan</button>
    </div>
    {#if going || rb.phase === "cancelling"}
      <button class="btn-danger h-9 w-full justify-center border border-danger/40 text-sm" disabled={rb.phase === "cancelling"} onclick={() => rb.cancel()} data-testid="stop-run">
        {#if rb.phase === "cancelling"}<Loader2 size={14} class="animate-spin" />{:else}<CircleStop size={14} />{/if} {rb.phase === "cancelling" ? "Cancelling…" : "Stop the run"}
      </button>
    {:else}
      <button class="btn-primary h-9 w-full justify-center text-sm" disabled={!rb.text || !canStart(rb.phase) || rb.chosen.length === 0 || hasProblems} onclick={() => rb.runNow()} data-testid="run-now" title="Validate, review the plan, then run">
        {#if rb.phase === "validating" || rb.phase === "starting"}<Loader2 size={14} class="animate-spin" />{:else}<Play size={14} />{/if}
        {rb.phase === "idle" || canStart(rb.phase) ? `Run now${rb.chosen.length ? ` on ${rb.chosen.length} host${rb.chosen.length === 1 ? "" : "s"}` : ""}` : RUN_LABEL[rb.phase]}
      </button>
      {#if rb.phase === "completed" || rb.phase === "failed" || rb.phase === "cancelled"}
        <p class="text-center text-[11px] {rb.phase === 'completed' ? 'text-success' : 'text-danger'}" role="status">{RUN_LABEL[rb.phase]}. See the output below.</p>
      {/if}
    {/if}
  </footer>
</aside>
