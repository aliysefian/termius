<script lang="ts">
  import { AlertTriangle, ExternalLink, Info, X } from "lucide-svelte";
  import { ageText, statusLine } from "$lib/infra/inventory";
  import { TYPE_LABEL } from "$lib/infra/model";
  import { infra } from "$lib/stores/infra.svelte";
  import { FRESH_LABEL, FRESH_TONE, STATUS_TONE, TYPE_ICON, show } from "./style";

  let { onclose }: { onclose?: () => void } = $props();
  const r = $derived(infra.selected);
  const Icon = $derived(r ? TYPE_ICON[r.type] : null);
  const rows = $derived(r ? Object.entries(r.attributes) : []);
</script>

<aside class="flex h-full min-h-0 flex-col border-l border-line bg-base" aria-label="Resource details" data-testid="infra-inspector">
  {#if !r || !Icon}
    <div class="m-auto max-w-[16rem] p-6 text-center text-xs text-fg-muted"><Info size={20} class="mx-auto mb-2" />Select a resource to see its details and where it belongs.</div>
  {:else}
    <header class="flex items-start gap-2 border-b border-line p-3">
      <Icon size={18} class="mt-0.5 shrink-0 text-fg-muted" />
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-sm font-semibold" title={r.name}>{r.name}</h3>
        <p class="text-[11px] text-fg-muted">{TYPE_LABEL[r.type]}{#if r.hostId} · {infra.hostLabel(r.hostId)}{/if}</p>
        <p class="mt-1 flex items-center gap-1.5 text-xs"><span class="h-2 w-2 rounded-full {STATUS_TONE[r.status]}"></span>{statusLine(r, infra.now)}</p>
        <p class="text-[11px] {FRESH_TONE[r.freshness]}">{FRESH_LABEL[r.freshness]}{r.observedAt ? ` · read ${ageText(r.observedAt, infra.now)}` : ""}</p>
      </div>
      {#if onclose}<button class="icon-btn h-7 w-7" aria-label="Close details" onclick={onclose}><X size={14} /></button>{/if}
    </header>
    <div class="min-h-0 flex-1 space-y-4 overflow-auto p-3 text-xs">
      {#if r.lastError}
        <p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-2 py-1.5 text-warning" role="alert"><AlertTriangle size={13} class="mt-0.5 shrink-0" /><span>The last read failed ({r.lastError}). This is the last known state.</span></p>
      {/if}
      <section>
        <h4 class="mb-1 font-medium text-fg-muted">Properties</h4>
        <dl class="grid grid-cols-[6.5rem_1fr] gap-x-2 gap-y-1">
          {#each rows as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="min-w-0 break-words {v === null ? 'text-fg-muted' : ''}">{show(k, v)}</dd>{/each}
        </dl>
      </section>
      {#if infra.parents.length || infra.children.length}
        <section>
          <h4 class="mb-1 font-medium text-fg-muted" title="Only what the owning module reported; nothing is guessed from names">Related</h4>
          <ul class="space-y-0.5">
            {#each infra.parents as p (p.id)}<li><button class="text-accent hover:underline" onclick={() => infra.reveal(p.id)}>{p.name}</button> <span class="text-fg-muted">· {TYPE_LABEL[p.type]} (runs it)</span></li>{/each}
            {#each infra.children.slice(0, 30) as c (c.id)}<li><button class="text-accent hover:underline" onclick={() => infra.reveal(c.id)}>{c.name}</button> <span class="text-fg-muted">· {TYPE_LABEL[c.type]}</span></li>{/each}
            {#if infra.children.length > 30}<li class="text-fg-muted">and {infra.children.length - 30} more</li>{/if}
          </ul>
        </section>
      {/if}
      {#if r.type === "port" && r.attributes.Protocol === "udp"}
        <p class="flex gap-1 text-fg-muted"><Info size={12} class="mt-0.5 shrink-0" />A published UDP port is a mapping; UDP has no listening state to confirm.</p>
      {/if}
      <section>
        <h4 class="mb-1 font-medium text-fg-muted">Open in</h4>
        <div class="flex flex-wrap gap-1.5">
          {#each infra.actions as a (a.id)}
            <button class="btn-secondary py-1 text-xs" disabled={!a.enabled} title={a.reason ?? a.label} onclick={() => infra.act(a.id)}><ExternalLink size={12} /> {a.label}</button>
          {/each}
          {#if infra.actions.length === 0}<span class="text-fg-muted">Nothing to open from here.</span>{/if}
        </div>
        <p class="mt-1.5 text-[11px] text-fg-muted">These open the tool that owns it. Starting, stopping and removing are done there, with its own confirmations.</p>
      </section>
    </div>
  {/if}
</aside>
