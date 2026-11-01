<script lang="ts">
  // Three steps: the type, the details, a plain-words review. Creating and starting are separate: the box on the
  // last step says whether to start it once it is saved. Opened empty (New), from a copy (Duplicate), or on an
  // existing tunnel (Edit, which skips the type step).
  import { ArrowLeftRight, ArrowRight, Check, Globe2, Loader2, Server } from "lucide-svelte";
  import Modal, { DISCARD } from "../Modal.svelte";
  import TunnelFields from "./TunnelFields.svelte";
  import { tunnels } from "$lib/stores/tunnels.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { draftFromRule, newDraft, ruleFromDraft, summary, validateTunnel, type Draft } from "$lib/tunnels";
  import { errorMessage, type ForwardRule, type Uuid } from "$lib/types";

  let { id, template }: { id: Uuid | null; template?: ForwardRule } = $props();

  // Remounted via {#key} when id changes, so capturing the initial values is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.forwards.find((f) => f.id === id) : undefined;
  // svelte-ignore state_referenced_locally
  const start = existing?.data ?? template;
  // svelte-ignore state_referenced_locally
  let draft = $state<Draft>(start ? { ...draftFromRule(start), ...(template && !existing ? { label: `${start.label} copy` } : {}) } : newDraft(vaultStore.hosts[0]?.id ?? ""));
  // An edit and a copy already have a type; only a new tunnel is asked.
  let step = $state<1 | 2 | 3>(start ? 2 : 1);
  let startAfter = $state(true);
  let busy = $state(false);
  let serverError = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  const initial = JSON.stringify(draft);
  const dirty = $derived(JSON.stringify(draft) !== initial || step > 1);

  const errors = $derived(
    validateTunnel(
      ruleFromDraft(draft),
      new Set(vaultStore.hosts.map((h) => h.id)),
      id,
      vaultStore.forwards.filter((f) => f.data).map((f) => ({ id: f.id, rule: f.data! })),
    ),
  );
  const invalid = $derived(Object.keys(errors).length > 0);
  const hostLabel = $derived(vaultStore.hostById.get(draft.host_id)?.data?.label ?? "");
  const review = $derived(summary(ruleFromDraft(draft), hostLabel));
  const editing = $derived(!!existing);

  const TYPES = [
    { kind: "local", icon: ArrowRight, title: "Local Forwarding", text: "Access a remote service through a local port.", flow: "this computer → host" },
    { kind: "remote", icon: ArrowLeftRight, title: "Remote Forwarding", text: "Expose a local service through the remote SSH server.", flow: "host → this computer" },
    { kind: "dynamic", icon: Globe2, title: "Dynamic Forwarding", text: "Create a local SOCKS proxy over SSH.", flow: "any destination" },
  ] as const;

  function chooseType(kind: Draft["kind"]) {
    draft.kind = kind;
    // A new SOCKS listener starts on the usual port, a forward on a common web port, unless one was typed.
    if (!dirty || step === 1) draft.bind_port = kind === "dynamic" ? 1080 : 8080;
    step = 2;
  }

  async function save() {
    if (invalid || busy) return;
    busy = true;
    serverError = null;
    try {
      const rec = await vaultStore.saveForward(id, ruleFromDraft(draft));
      ui.modal = null;
      const savedId = rec.id;
      tunnels.select(savedId);
      ui.notify("info", editing ? `Saved "${draft.label.trim()}".` : `Created tunnel "${draft.label.trim()}".`);
      if (!editing && startAfter) await tunnels.start(savedId);
    } catch (e) {
      // The backend's own words: a port conflict, a bad address, a missing host.
      serverError = errorMessage(e);
      step = 2;
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={editing ? "Edit tunnel" : "Create tunnel"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null}>
  <div class="mb-5 flex items-center gap-2 text-xs text-fg-muted" aria-label="Step {step} of 3">
    {#each [["Type", 1], ["Details", 2], ["Review", 3]] as [label, n] (n)}
      <span class="flex items-center gap-1.5 {step === n ? 'font-medium text-fg' : ''}" aria-current={step === n ? "step" : undefined}>
        <span class="flex h-5 w-5 items-center justify-center rounded-full border text-[11px] {step > (n as number) ? 'border-accent bg-accent text-white' : step === n ? 'border-accent text-accent' : 'border-line'}">
          {#if step > (n as number)}<Check size={11} />{:else}{n}{/if}
        </span>
        {label}
      </span>
      {#if n !== 3}<span class="h-px flex-1 bg-line" aria-hidden="true"></span>{/if}
    {/each}
  </div>

  {#if vaultStore.hosts.length === 0}
    <div class="rounded-[10px] border border-warning/30 bg-warning/10 p-4 text-sm">
      <p class="flex items-center gap-2 font-medium text-warning"><Server size={16} /> Add an SSH host first</p>
      <p class="mt-1 text-xs text-fg-muted">A tunnel runs through one of your hosts, and there are none yet.</p>
      <button class="btn-primary mt-3" onclick={() => (ui.modal = { kind: "host", id: null })}>Add a host</button>
    </div>
  {:else if step === 1}
    <div class="grid gap-3" role="radiogroup" aria-label="Tunnel type">
      {#each TYPES as t (t.kind)}
        <button
          type="button"
          role="radio"
          aria-checked={draft.kind === t.kind}
          class="flex items-start gap-3 rounded-[10px] border bg-base/40 p-4 text-left transition-colors hover:border-accent/50 hover:bg-panel-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent {draft.kind === t.kind ? 'border-accent' : 'border-line'}"
          onclick={() => chooseType(t.kind)}
        >
          <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent/10 text-accent"><t.icon size={18} aria-hidden="true" /></span>
          <span class="min-w-0">
            <span class="block text-sm font-medium">{t.title}</span>
            <span class="mt-0.5 block text-xs text-fg-muted">{t.text}</span>
            <span class="mt-1.5 block font-mono text-[11px] text-fg-muted">{t.flow}</span>
          </span>
        </button>
      {/each}
    </div>
  {:else if step === 2}
    <TunnelFields bind:draft {errors} idPrefix="new" showType={editing || !!template} />
    {#if serverError}<p class="mt-4 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{serverError}</p>{/if}
  {:else}
    <div class="rounded-[10px] border border-line bg-base/50 p-4">
      <p class="text-sm font-semibold">{review.title}</p>
      <p class="mt-0.5 text-xs text-fg-muted">{draft.label.trim()}{draft.description.trim() ? ` · ${draft.description.trim()}` : ""}</p>
      <dl class="mt-3 space-y-2.5 text-sm">
        {#each review.lines as [k, v] (k)}
          <div><dt class="text-xs text-fg-muted">{k}</dt><dd class="font-mono tabular-nums">{v}</dd></div>
        {/each}
      </dl>
    </div>
    {#if !editing}
      <label class="mt-4 flex items-start gap-2.5 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={startAfter} />
        <span>Start the tunnel after creating it<span class="block text-xs text-fg-muted">Creating and starting are separate: leave this off to only save it.</span></span>
      </label>
    {/if}
    {#if serverError}<p class="mt-4 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{serverError}</p>{/if}
  {/if}

  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    {#if step > 1 && !(step === 2 && (editing || template))}
      <button class="btn-secondary" type="button" onclick={() => (step = (step - 1) as 1 | 2)}>Back</button>
    {/if}
    {#if step === 2}
      <button class="btn-primary" type="button" disabled={invalid || vaultStore.hosts.length === 0} onclick={() => (step = 3)}>Review</button>
    {:else if step === 3}
      <button class="btn-primary" type="button" disabled={invalid || busy} onclick={() => void save()}>
        {#if busy}<Loader2 size={14} class="animate-spin" />{/if}
        {editing ? "Save changes" : "Create Tunnel"}
      </button>
    {/if}
  {/snippet}
</Modal>
