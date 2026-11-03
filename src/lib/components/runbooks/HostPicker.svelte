<script lang="ts">
  import { Search, ShieldAlert, X } from "lucide-svelte";
  import { addIds, chosenSummary, groupsOf, inGroup, removeId, searchHosts, tagsOf, type PickHost } from "$lib/runbookrun";

  let { hosts, chosen, onchange, disabled = false }: { hosts: PickHost[]; chosen: string[]; onchange: (ids: string[]) => void; disabled?: boolean } = $props();

  let query = $state("");
  let group = $state("");
  let tag = $state("");
  /** At most this many matches are drawn; refine the search to see the rest. */
  const SHOWN = 100;

  const groups = $derived(groupsOf(hosts));
  const tags = $derived(tagsOf(hosts));
  const matches = $derived(searchHosts(hosts, query).filter((h) => (!group || inGroup(h, group)) && (!tag || h.tags.includes(tag))));
  const shown = $derived(matches.slice(0, SHOWN));
  const picked = $derived(new Set(chosen));
  const chips = $derived(chosen.map((id) => hosts.find((h) => h.id === id)).filter((h): h is PickHost => !!h));
  const missing = $derived(chosen.length - chips.length);
  const allVisible = $derived(matches.length > 0 && matches.every((h) => picked.has(h.id)));

  const toggle = (id: string) => onchange(picked.has(id) ? removeId(chosen, id) : addIds(chosen, [id]));
  const selectVisible = () => onchange(addIds(chosen, matches.map((h) => h.id)));

  function key(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle(id);
    }
  }
</script>

<section aria-label="Target hosts" class="space-y-2" data-testid="host-picker">
  <p class="text-xs font-medium" aria-live="polite" data-testid="host-count">{chosenSummary(chips)}{#if missing}<span class="text-danger"> · {missing} no longer exist{missing === 1 ? "s" : ""}</span>{/if}</p>

  {#if chosen.length}
    <ul class="flex max-h-24 flex-wrap gap-1 overflow-y-auto" aria-label="Selected hosts">
      {#each chips as h (h.id)}
        <li class="inline-flex items-center gap-1 rounded-md border border-line bg-base px-1.5 py-0.5 text-xs {h.production ? 'border-danger/40' : ''}">
          {#if h.production}<ShieldAlert size={11} class="text-danger" aria-label="Production" />{/if}
          <span class="max-w-32 truncate">{h.label}</span>
          <button class="rounded text-fg-muted hover:text-fg focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent" aria-label="Remove {h.label}" {disabled} onclick={() => onchange(removeId(chosen, h.id))}><X size={11} /></button>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="relative">
    <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
    <input class="input h-8 w-full pl-8 text-xs" type="search" placeholder="Search hosts, groups, tags" aria-label="Search hosts" bind:value={query} {disabled} />
  </div>
  {#if groups.length || tags.length}
    <div class="flex gap-1.5">
      {#if groups.length}
        <select class="input h-7 min-w-0 flex-1 py-0 text-xs" aria-label="Show a group" bind:value={group} {disabled}>
          <option value="">All groups</option>
          {#each groups as g (g.name)}<option value={g.name}>{g.name} ({g.count})</option>{/each}
        </select>
      {/if}
      {#if tags.length}
        <select class="input h-7 min-w-0 flex-1 py-0 text-xs" aria-label="Show a tag" bind:value={tag} {disabled}>
          <option value="">All tags</option>
          {#each tags as t (t.name)}<option value={t.name}>{t.name} ({t.count})</option>{/each}
        </select>
      {/if}
    </div>
  {/if}

  <div class="flex items-center gap-2 text-xs">
    <button class="text-accent hover:underline disabled:opacity-40 disabled:no-underline" disabled={disabled || matches.length === 0 || allVisible} onclick={selectVisible}>Select all visible ({matches.length})</button>
    <span class="text-fg-muted">·</span>
    <button class="text-accent hover:underline disabled:opacity-40 disabled:no-underline" disabled={disabled || chosen.length === 0} onclick={() => onchange([])}>Clear selection</button>
  </div>

  <ul class="max-h-44 overflow-y-auto rounded-md border border-line" role="listbox" aria-multiselectable="true" aria-label="Hosts" data-testid="host-list">
    {#each shown as h (h.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        role="option"
        aria-selected={picked.has(h.id)}
        tabindex="0"
        class="flex cursor-pointer items-center gap-2 px-2 py-1 text-xs hover:bg-panel-hover focus-visible:bg-panel-hover focus-visible:outline-none {picked.has(h.id) ? 'bg-accent/10' : ''}"
        onclick={() => !disabled && toggle(h.id)}
        onkeydown={(e) => !disabled && key(e, h.id)}
      >
        <input type="checkbox" class="accent-input" tabindex="-1" checked={picked.has(h.id)} aria-hidden="true" {disabled} />
        <span class="min-w-0 flex-1 truncate">{h.label}</span>
        {#if h.group}<span class="max-w-24 shrink-0 truncate text-[11px] text-fg-muted">{h.group}</span>{/if}
        {#if h.production}<span class="shrink-0 rounded bg-danger/15 px-1.5 text-[10px] text-danger">production</span>{/if}
        {#if !h.unattended}<span class="shrink-0 text-[10px] text-warning" title="No saved credentials, so it can't connect on its own">needs sign-in</span>{/if}
      </li>
    {:else}
      <li class="px-3 py-4 text-center text-xs text-fg-muted">{hosts.length === 0 ? "No SSH hosts yet. Add one under Hosts." : "No host matches."}</li>
    {/each}
    {#if matches.length > SHOWN}<li class="px-3 py-1.5 text-center text-[11px] text-fg-muted">{matches.length - SHOWN} more. Narrow the search to see them.</li>{/if}
  </ul>
</section>
