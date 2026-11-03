<script lang="ts">
  import { tick } from "svelte";
  import { Plus, Search, ShieldAlert, X } from "lucide-svelte";
  import { addIds, groupsOf, inGroup, removeId, searchHosts, tagsOf, type PickHost } from "$lib/runbookrun";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let { hosts, chosen, onchange }: { hosts: PickHost[]; chosen: string[]; onchange: (ids: string[]) => void } = $props();

  let open = $state(false);
  let query = $state("");
  let group = $state("");
  let tag = $state("");
  let button = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();
  let search = $state<HTMLInputElement>();
  let pos = $state<{ left: number; top: number; width: number; maxHeight: number } | null>(null);
  let active = $state(0);

  const SHOWN = 200;
  const MAX_CHIPS = 4;
  const groups = $derived(groupsOf(hosts));
  const tags = $derived(tagsOf(hosts));
  const matches = $derived(searchHosts(hosts, query).filter((h) => (!group || inGroup(h, group)) && (!tag || h.tags.includes(tag))));
  const shown = $derived(matches.slice(0, SHOWN));
  const picked = $derived(new Set(chosen));
  const chips = $derived(chosen.map((id) => hosts.find((h) => h.id === id)).filter((h): h is PickHost => !!h));
  const missing = $derived(chosen.length - chips.length);
  const selectable = $derived(matches.filter((h) => h.unattended));
  const allPicked = $derived(selectable.length > 0 && selectable.every((h) => picked.has(h.id)));

  /** What is known of a host's reachability: the stream's own state while it runs, else the last health check. Never assumed. */
  function status(id: string): { tone: "good" | "warn" | "bad" | "idle"; text: string } {
    const s = opsLogs.hostStates[id];
    if (s && opsLogs.running) {
      if (s.state === "streaming") return { tone: "good", text: "Connected, streaming" };
      if (s.state === "connecting") return { tone: "warn", text: "Connecting" };
      if (s.state === "reconnecting") return { tone: "warn", text: "Connection lost, reconnecting" };
      if (s.state === "failed") return { tone: "bad", text: s.message ?? "Failed" };
    }
    const h = vaultStore.health[id];
    if (h?.state === "up") return { tone: "good", text: "Answered the last check" };
    if (h?.state === "down") return { tone: "bad", text: "Did not answer the last check" };
    return { tone: "idle", text: "Not checked" };
  }
  const dot = { good: "bg-success", warn: "bg-warning", bad: "bg-danger", idle: "bg-fg-muted/40" } as const;

  const toggle = (h: PickHost) => {
    if (!h.unattended) return;
    onchange(picked.has(h.id) ? removeId(chosen, h.id) : addIds(chosen, [h.id]));
  };
  const selectFiltered = () => onchange(addIds(chosen, selectable.map((h) => h.id)));

  function place() {
    if (!button) return;
    const r = button.getBoundingClientRect();
    const width = Math.min(420, window.innerWidth - 16);
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    const below = window.innerHeight - r.bottom - 12;
    pos = { left, top: r.bottom + 6, width, maxHeight: Math.max(220, Math.min(520, below)) };
  }

  async function show() {
    open = true;
    active = 0;
    await tick();
    place();
    search?.focus();
  }

  function hide(focus = true) {
    open = false;
    query = "";
    if (focus) button?.focus();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hide();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (shown.length) active = (active + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
      void tick().then(() => panel?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter" || (e.key === " " && document.activeElement !== search)) {
      e.preventDefault();
      if (shown[active]) toggle(shown[active]);
    }
  }

  $effect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!panel?.contains(t) && !button?.contains(t)) hide(false);
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("resize", place);
    };
  });
</script>

<div class="flex min-w-0 flex-wrap items-center gap-1.5" data-testid="ops-host-selector">
  <span class="text-xs font-medium text-fg-muted" aria-live="polite">Hosts <span class="text-fg">({chosen.length} selected)</span></span>
  {#each chips.slice(0, MAX_CHIPS) as h (h.id)}
    {@const st = status(h.id)}
    <span class="inline-flex max-w-44 items-center gap-1 rounded-md border bg-base px-1.5 py-0.5 text-xs {h.production ? 'border-danger/40' : 'border-line'} {h.unattended ? '' : 'opacity-60'}" title="{h.label} · {h.unattended ? st.text : 'No saved credentials, so it cannot be followed'}">
      <span class="h-2 w-2 shrink-0 rounded-full {dot[st.tone]}" aria-hidden="true"></span>
      {#if h.production}<ShieldAlert size={10} class="shrink-0 text-danger" aria-label="Production" />{/if}
      <span class="truncate">{h.label}</span>
      <button class="rounded text-fg-muted hover:text-fg focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent" aria-label="Remove {h.label}" onclick={() => onchange(removeId(chosen, h.id))}><X size={11} /></button>
    </span>
  {/each}
  {#if chips.length > MAX_CHIPS}
    <button class="rounded-md border border-line bg-base px-1.5 py-0.5 text-xs text-fg-muted hover:text-fg" onclick={() => void show()} title={chips.slice(MAX_CHIPS).map((h) => h.label).join(", ")}>+{chips.length - MAX_CHIPS} more</button>
  {/if}
  {#if missing > 0}<span class="text-[11px] text-danger">{missing} no longer exist{missing === 1 ? "s" : ""}</span>{/if}
  <button
    bind:this={button}
    class="inline-flex items-center gap-1 rounded-md border border-dashed border-line px-2 py-0.5 text-xs text-accent hover:border-accent hover:bg-accent/10 focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent"
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={() => (open ? hide() : void show())}
  ><Plus size={12} /> Add Hosts</button>
  {#if chosen.length > 0}<button class="text-[11px] text-fg-muted hover:text-fg hover:underline" onclick={() => onchange([])}>Clear</button>{/if}
</div>

{#if open && pos}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={panel}
    class="fixed z-50 flex flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-2xl"
    style="left: {pos.left}px; top: {pos.top}px; width: {pos.width}px; max-height: {pos.maxHeight}px"
    onkeydown={keydown}
    data-testid="ops-host-popover"
  >
    <div class="space-y-2 border-b border-line p-2">
      <div class="relative">
        <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input bind:this={search} class="input h-8 w-full pl-8 text-xs" type="search" placeholder="Search by name, address, group or tag" aria-label="Search hosts" bind:value={query} oninput={() => (active = 0)} />
      </div>
      {#if groups.length || tags.length}
        <div class="flex gap-1.5">
          {#if groups.length}
            <select class="input h-7 min-w-0 flex-1 py-0 text-xs" aria-label="Show a group" bind:value={group}>
              <option value="">All groups</option>
              {#each groups as g (g.name)}<option value={g.name}>{g.name} ({g.count})</option>{/each}
            </select>
          {/if}
          {#if tags.length}
            <select class="input h-7 min-w-0 flex-1 py-0 text-xs" aria-label="Show a tag" bind:value={tag}>
              <option value="">All tags</option>
              {#each tags as t (t.name)}<option value={t.name}>{t.name} ({t.count})</option>{/each}
            </select>
          {/if}
        </div>
      {/if}
      <div class="flex items-center gap-2 text-xs">
        <button class="text-accent hover:underline disabled:opacity-40 disabled:no-underline" disabled={selectable.length === 0 || allPicked} onclick={selectFiltered}>Select all filtered ({selectable.length})</button>
        <span class="text-fg-muted">·</span>
        <button class="text-accent hover:underline disabled:opacity-40 disabled:no-underline" disabled={chosen.length === 0} onclick={() => onchange([])}>Clear selection</button>
        <span class="ml-auto text-fg-muted">{chosen.length} selected</span>
      </div>
    </div>
    <ul class="min-h-0 flex-1 overflow-y-auto" role="listbox" aria-multiselectable="true" aria-label="Hosts">
      {#each shown as h, i (h.id)}
        {@const st = status(h.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          data-i={i}
          aria-selected={picked.has(h.id)}
          aria-disabled={!h.unattended}
          class="flex items-center gap-2 px-2.5 py-1.5 text-xs {h.unattended ? 'cursor-pointer hover:bg-panel-hover' : 'cursor-not-allowed opacity-55'} {i === active ? 'bg-panel-hover' : ''} {picked.has(h.id) ? 'bg-accent/10' : ''}"
          onclick={() => toggle(h)}
        >
          <input type="checkbox" class="accent-input" tabindex="-1" aria-hidden="true" checked={picked.has(h.id)} disabled={!h.unattended} />
          <span class="h-2 w-2 shrink-0 rounded-full {dot[st.tone]}" title={st.text}></span>
          <span class="min-w-0 flex-1">
            <span class="block truncate">{h.label}</span>
            <span class="block truncate text-[11px] text-fg-muted">{h.hostname}{h.group ? ` · ${h.group}` : ""}</span>
          </span>
          {#if h.production}<span class="shrink-0 rounded bg-danger/15 px-1.5 text-[10px] text-danger">production</span>{/if}
          {#if !h.unattended}<span class="shrink-0 text-[10px] text-warning" title="No saved credentials. Logs are read over a second connection that never asks for a password.">needs saved sign-in</span>{/if}
        </li>
      {:else}
        <li class="px-3 py-6 text-center text-xs text-fg-muted">{hosts.length === 0 ? "No SSH hosts yet. Add one under Hosts." : "No host matches."}</li>
      {/each}
      {#if matches.length > SHOWN}<li class="px-3 py-1.5 text-center text-[11px] text-fg-muted">{matches.length - SHOWN} more. Narrow the search to see them.</li>{/if}
    </ul>
    <div class="border-t border-line px-2.5 py-1.5 text-right"><button class="btn-secondary h-7 px-3 text-xs" onclick={() => hide()}>Done</button></div>
  </div>
{/if}
