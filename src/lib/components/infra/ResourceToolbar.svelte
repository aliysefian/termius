<script lang="ts">
  import { SlidersHorizontal, X } from "lucide-svelte";
  import { TYPE_LABEL, STATUS_LABEL, type ResourceStatus, type ResourceType } from "$lib/infra/model";
  import { infra } from "$lib/stores/infra.svelte";
  import { TYPE_ICON } from "./style";

  let { compact = false, onclear }: { compact?: boolean; onclear: () => void } = $props();

  const TYPES: ResourceType[] = ["host", "container", "systemd_service", "tmux_session", "docker_engine", "port"];
  const STATUSES: ResourceStatus[] = ["running", "healthy", "stopped", "warning", "critical", "unknown"];
  const SHORT: Partial<Record<ResourceType, string>> = { systemd_service: "Services", tmux_session: "Tmux", docker_engine: "Engines", container: "Containers", host: "Hosts", port: "Ports" };

  let more = $state(false);
  const environments = $derived([...new Set(infra.hostIds.map((id) => infra.envOf(id)).filter(Boolean))].sort());
  const f = $derived(infra.filters);
  const active = $derived(!!(f.query || f.types.length || f.statuses.length || f.environment || f.hostId));
  const secondary = $derived((f.statuses.length ? 1 : 0) + (f.environment ? 1 : 0) + (f.hostId ? 1 : 0));

  function toggle(t: ResourceType) {
    infra.filters.types = f.types.includes(t) ? f.types.filter((x) => x !== t) : [...f.types, t];
  }
</script>

{#snippet secondaryFilters()}
  <select class="input h-8 w-auto min-w-28 py-0 text-xs" aria-label="Status" value={f.statuses[0] ?? ""} onchange={(e) => (infra.filters.statuses = e.currentTarget.value ? [e.currentTarget.value as ResourceStatus] : [])}>
    <option value="">Any status</option>{#each STATUSES as s (s)}<option value={s}>{STATUS_LABEL[s]}</option>{/each}
  </select>
  {#if environments.length}
    <select class="input h-8 w-auto min-w-32 py-0 text-xs" aria-label="Environment" bind:value={infra.filters.environment}><option value="">Any environment</option>{#each environments as e (e)}<option value={e}>{e}</option>{/each}</select>
  {/if}
  <select class="input h-8 w-auto min-w-32 max-w-48 py-0 text-xs" aria-label="Host" value={f.hostId ?? ""} onchange={(e) => (infra.filters.hostId = e.currentTarget.value || null)}>
    <option value="">Any host</option>{#each infra.hostIds as id (id)}<option value={id}>{infra.hostLabel(id)}</option>{/each}
  </select>
{/snippet}

<div class="flex flex-wrap items-center gap-2 text-xs" role="toolbar" aria-label="Resource filters">
  <div class="inline-flex flex-wrap items-center gap-1" role="group" aria-label="Resource types">
    <button class="h-8 rounded-md border px-2.5 {f.types.length === 0 ? 'border-accent bg-accent/15 text-fg' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}" aria-pressed={f.types.length === 0} onclick={() => (infra.filters.types = [])}>All</button>
    {#each TYPES as t (t)}
      {@const I = TYPE_ICON[t]}
      <button class="inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 {f.types.includes(t) ? 'border-accent bg-accent/15 text-fg' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}" aria-pressed={f.types.includes(t)} title={TYPE_LABEL[t]} onclick={() => toggle(t)}><I size={13} />{SHORT[t] ?? TYPE_LABEL[t]}</button>
    {/each}
  </div>
  <span class="hidden h-5 w-px bg-line sm:block"></span>
  {#if compact}
    <div class="relative">
      <button class="inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 {secondary ? 'border-accent text-fg' : 'border-line text-fg-muted'} hover:bg-panel-hover" aria-haspopup="true" aria-expanded={more} onclick={() => (more = !more)}><SlidersHorizontal size={13} />Filters{#if secondary}<span class="rounded-full bg-accent-solid px-1.5 text-[10px] text-white">{secondary}</span>{/if}</button>
      {#if more}
        <button class="fixed inset-0 z-30 cursor-default" aria-label="Close filters" tabindex="-1" onclick={() => (more = false)}></button>
        <div class="absolute left-0 top-full z-40 mt-1.5 flex w-64 flex-col gap-2 rounded-lg border border-line bg-panel p-3 shadow-2xl" role="dialog" aria-label="More filters">{@render secondaryFilters()}</div>
      {/if}
    </div>
  {:else}
    {@render secondaryFilters()}
  {/if}
  {#if active}
    <button class="inline-flex h-8 items-center gap-1 rounded-md px-2 text-accent hover:bg-panel-hover" onclick={onclear}><X size={12} /> Reset filters</button>
  {/if}
</div>
