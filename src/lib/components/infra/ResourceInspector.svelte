<script lang="ts">
  import { AlertTriangle, ChevronRight, ExternalLink, Info, X } from "lucide-svelte";
  import { ageText, statusLine } from "$lib/infra/inventory";
  import { TYPE_LABEL } from "$lib/infra/model";
  import { infra } from "$lib/stores/infra.svelte";
  import { FRESH_LABEL, FRESH_TONE, STATUS_TONE, TYPE_ICON, show } from "./style";

  let { onclose }: { onclose?: () => void } = $props();
  /** How many properties the inspector shows; the whole list is in the workspace's Overview. */
  const FACTS = 6;
  /** How many individual related resources are named before the rest are behind a group. */
  const NAMED = 3;
  const r = $derived(infra.selected);
  const Icon = $derived(r ? TYPE_ICON[r.type] : null);
  const rows = $derived(r ? Object.entries(r.attributes).filter(([, v]) => v !== null && v !== "").slice(0, FACTS) : []);
  const groupLabel = (type: keyof typeof TYPE_LABEL, n: number) => `${TYPE_LABEL[type]}${n === 1 ? "" : "s"}`;
</script>

<aside class="flex h-full min-h-0 min-w-0 flex-col bg-base" aria-label="Resource inspector" data-testid="infra-inspector">
  {#if !r || !Icon}
    <div class="m-auto max-w-[16rem] p-6 text-center text-xs text-fg-muted"><Info size={20} class="mx-auto mb-2" />Select a resource to see its details and where it belongs.</div>
  {:else}
    <header class="flex items-start gap-3 border-b border-line p-3">
      <span class="grid h-10 w-10 shrink-0 place-items-center rounded-lg bg-panel text-fg-muted"><Icon size={20} /></span>
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-sm font-semibold" title={r.name}>{r.name}</h3>
        <p class="truncate text-xs text-fg-muted">{TYPE_LABEL[r.type]}{#if r.hostId}{" · "}{infra.hostLabel(r.hostId)}{/if}</p>
        <p class="mt-1 flex items-start gap-1.5 text-xs"><span class="mt-1 h-2 w-2 shrink-0 rounded-full {STATUS_TONE[r.status]}"></span><span class="min-w-0 break-words">{statusLine(r, infra.now)}</span></p>
        <p class="text-[11px] {FRESH_TONE[r.freshness]}">{FRESH_LABEL[r.freshness]}{r.observedAt ? ` · updated ${ageText(r.observedAt, infra.now)}` : ""}</p>
      </div>
      {#if onclose}<button class="icon-btn h-7 w-7" aria-label="Close inspector" onclick={onclose}><X size={14} /></button>{/if}
    </header>
    <div class="min-h-0 flex-1 space-y-5 overflow-y-auto p-3 text-xs">
      {#if r.lastError}
        <p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-2 py-1.5 text-warning" role="alert"><AlertTriangle size={13} class="mt-0.5 shrink-0" /><span>The last read failed ({r.lastError}). This is the last known state.</span></p>
      {/if}

      <section aria-labelledby="insp-overview">
        <h4 id="insp-overview" class="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Overview</h4>
        <dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-2 gap-y-1.5">
          {#each rows as [k, v] (k)}<dt class="text-fg-muted">{k}</dt><dd class="truncate" title={show(k, v)}>{show(k, v)}</dd>{/each}
        </dl>
        {#if Object.keys(r.attributes).length > FACTS}
          <button class="mt-1.5 text-accent hover:underline" onclick={() => (infra.tab = "overview")}>All properties</button>
        {/if}
      </section>

      <section aria-labelledby="insp-actions">
        <h4 id="insp-actions" class="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Quick actions</h4>
        <div class="flex flex-col gap-1">
          {#each infra.actions as a (a.id)}
            <button class="btn-secondary justify-start py-1.5 text-xs" disabled={!a.enabled} title={a.reason ?? a.label} onclick={() => infra.act(a.id)}><ExternalLink size={13} /> {a.label}</button>
          {/each}
          {#if infra.actions.length === 0}<span class="text-fg-muted">Nothing to open from here.</span>{/if}
        </div>
        <p class="mt-1.5 text-[11px] text-fg-muted">These open the tool that owns it. Starting, stopping and removing are done there.</p>
      </section>

      {#if infra.parents.length || infra.relatedGroups.length}
        <section aria-labelledby="insp-related">
          <h4 id="insp-related" class="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted" title="Only what the owning module reported; nothing is guessed from names">Related resources</h4>
          {#each infra.parents as p (p.id)}
            {@const PI = TYPE_ICON[p.type]}
            <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" onclick={() => infra.reveal(p.id)}>
              <PI size={13} class="shrink-0 text-fg-muted" /><span class="min-w-0 flex-1 truncate" title={p.name}>{p.name}</span><span class="shrink-0 text-fg-muted">{TYPE_LABEL[p.type]} · runs it</span>
            </button>
          {/each}
          {#each infra.relatedGroups as g (g.type)}
            {@const GI = TYPE_ICON[g.type]}
            <button class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-panel-hover" onclick={() => infra.openRelated(g.type)} aria-label="Show {g.count} {groupLabel(g.type, g.count)}">
              <GI size={13} class="shrink-0 text-fg-muted" /><span class="min-w-0 flex-1 truncate">{groupLabel(g.type, g.count)}</span><span class="font-semibold tabular-nums">{g.count}</span><ChevronRight size={13} class="shrink-0 text-fg-muted" />
            </button>
          {/each}
          {#if infra.children.length > infra.relatedGroups.length}
            <ul class="mt-1 space-y-0.5 border-t border-line pt-1.5">
              {#each infra.children.slice(0, NAMED) as c (c.id)}<li><button class="block w-full truncate rounded px-2 py-0.5 text-left text-accent hover:underline" title={c.name} onclick={() => infra.reveal(c.id)}>{c.name}</button></li>{/each}
            </ul>
            {#if infra.children.length > NAMED}
              <button class="mt-1 px-2 text-accent hover:underline" onclick={() => { infra.relatedType = null; infra.view = "list"; infra.tab = "relationships"; }}>View all {infra.children.length} →</button>
            {/if}
          {/if}
        </section>
      {/if}
      {#if r.type === "port" && r.attributes.Protocol === "udp"}
        <p class="flex gap-1 text-fg-muted"><Info size={12} class="mt-0.5 shrink-0" />A published UDP port is a mapping; UDP has no listening state to confirm.</p>
      {/if}
    </div>
  {/if}
</aside>
