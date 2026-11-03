<script lang="ts">
  import { Globe, HardDriveUpload, Loader2, MinusCircle, Play, ShieldAlert } from "lucide-svelte";
  import Modal from "../Modal.svelte";
  import { strategyLabel, strategyOf } from "$lib/runbookrun";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const hosts = $derived(rb.chosenHosts);
  const prod = $derived(hosts.filter((h) => h.production));
  const first = $derived(rb.plans?.[0]);
  /** Steps are the same for every host except where the host's own name or address is written in. */
  const differs = $derived((rb.plans ?? []).slice(1).some((p) => JSON.stringify(p.steps.map((s) => s.text)) !== JSON.stringify(first?.steps.map((s) => s.text))));
  const destructive = $derived(new Set(rb.destructive()));
  const files = $derived(first?.steps.filter((s) => s.kind === "upload" || s.kind === "download") ?? []);
  const shownValues = $derived(Object.entries(rb.textParams()).filter(([k]) => rb.check?.params.find((p) => p.name === k)?.kind !== "secret"));
  const secrets = $derived((rb.check?.params ?? []).filter((p) => p.kind === "secret"));
  let confirming = $state(false);

  async function go() {
    confirming = true;
    try {
      await rb.confirmRun();
    } finally {
      confirming = false;
    }
  }
</script>

<Modal title="Preview execution plan" width="max-w-3xl" {onclose}>
  <div class="space-y-4 text-sm" data-testid="plan-dialog">
    <p class="rounded-md border border-line bg-base/50 px-3 py-2 text-xs text-fg-muted">This is the plan, built from the document and the values as they are now. <strong class="text-fg">Nothing has run.</strong> It cannot say what a command will do: a command that deletes still deletes. Variables of kind secret appear as &lt;name&gt;.</p>

    <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-xs">
      <dt class="text-fg-muted">Runbook</dt><dd class="font-medium">{rb.name}</dd>
      <dt class="text-fg-muted">Hosts</dt>
      <dd>{hosts.length} host{hosts.length === 1 ? "" : "s"}: <span class="text-fg-muted">{hosts.slice(0, 12).map((h) => h.label).join(", ")}{hosts.length > 12 ? ` and ${hosts.length - 12} more` : ""}</span></dd>
      <dt class="text-fg-muted">Strategy</dt><dd>{strategyLabel(strategyOf(rb.strategy))}. Each host runs the steps in order.</dd>
      {#if shownValues.length}
        <dt class="text-fg-muted">Values</dt>
        <dd class="font-mono text-[11px]">{shownValues.map(([k, v]) => `${k} = ${v}`).join(", ")}{#if secrets.length}, {secrets.map((s) => `${s.name} = <hidden>`).join(", ")}{/if}</dd>
      {/if}
      {#if rb.rollbackOn && (rb.check?.rollback_steps ?? 0) > 0}<dt class="text-fg-muted">Rollback</dt><dd class="text-warning">On: a host that fails gets its {rb.check?.rollback_steps} rollback step{rb.check?.rollback_steps === 1 ? "" : "s"}.</dd>{/if}
    </dl>

    {#if prod.length}
      <p class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger"><ShieldAlert size={14} class="mt-0.5 shrink-0" /> {prod.length} production host{prod.length === 1 ? "" : "s"}: {prod.slice(0, 8).map((h) => h.label).join(", ")}{prod.length > 8 ? "…" : ""}</p>
    {/if}

    {#if rb.planning}
      <p class="flex items-center gap-2 text-xs text-fg-muted"><Loader2 size={13} class="animate-spin" /> Building the plan…</p>
    {:else if first}
      <section aria-label="Steps">
        <h3 class="mb-1.5 text-xs font-medium">Steps{differs ? ` (shown for ${first.label}; the others differ where the host's name or address is used)` : " (the same on every host)"}</h3>
        {#if first.error}<p class="text-xs text-danger" role="alert">{first.error}</p>{/if}
        <ol class="space-y-1.5 text-xs">
          {#each first.steps as s (s.index)}
            <li class="rounded-md border border-line px-3 py-2 {s.disabled ? 'opacity-60' : ''}">
              <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
                <span class="font-medium">{s.index + 1}. {s.name}</span>
                <span class="rounded bg-fg-muted/15 px-1.5 text-[10px] uppercase tracking-wide">{s.kind}</span>
                {#if s.phase === "rollback"}<span class="rounded bg-warning/15 px-1.5 text-[10px] text-warning">rollback</span>{/if}
                {#if s.local}<span class="inline-flex items-center gap-1 text-[11px] text-fg-muted" title="This step acts on this computer, not on the host">{#if s.kind === "http" || s.kind === "tcp"}<Globe size={11} />{:else}<HardDriveUpload size={11} />{/if} from this computer</span>{/if}
                {#if s.disabled}<span class="inline-flex items-center gap-1 text-[11px] text-fg-muted"><MinusCircle size={11} /> switched off, will be skipped</span>{/if}
                {#if destructive.has(s.name)}<span class="inline-flex items-center gap-1 rounded bg-danger/15 px-1.5 text-[10px] text-danger"><ShieldAlert size={10} /> matches a destructive pattern</span>{/if}
                {#if s.condition}<span class="text-[11px] text-fg-muted">· {s.condition}</span>{/if}
                {#if s.on_error === "continue"}<span class="text-[11px] text-fg-muted">· carries on if it fails</span>{/if}
              </div>
              <code class="mt-1 block break-all rounded bg-base px-2 py-1 font-mono text-[11px]">{s.text}</code>
            </li>
          {/each}
        </ol>
      </section>
      {#if files.length}
        <section aria-label="Files">
          <h3 class="mb-1 text-xs font-medium">Files</h3>
          <ul class="space-y-0.5 text-xs text-fg-muted">{#each files as f (f.index)}<li>{f.text}</li>{/each}</ul>
        </section>
      {/if}
    {/if}

    <section aria-label="Not verified">
      <h3 class="mb-1 text-xs font-medium">Not verified</h3>
      <ul class="list-disc space-y-0.5 pl-4 text-[11px] text-fg-muted">
        <li>{rb.connected ? "Connections were tested at the time shown in Validation; a host can go away since." : "Whether the hosts answer. Use Test connections to really connect."}</li>
        <li>Permissions and free space on the hosts, and whether a path exists.</li>
        <li>What the commands do. They run as written.</li>
      </ul>
    </section>
  </div>
  {#snippet footer()}
    <button class="btn-secondary" onclick={onclose}>Back</button>
    <button class="btn-primary" disabled={rb.planning || confirming || !!first?.error} onclick={go} data-testid="confirm-run">
      {#if confirming}<Loader2 size={14} class="animate-spin" />{:else}<Play size={14} />{/if} Run on {hosts.length} host{hosts.length === 1 ? "" : "s"}
    </button>
  {/snippet}
</Modal>
