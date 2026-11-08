<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Check, Eye, EyeOff, GripVertical, LayoutTemplate, Pencil, Plus, Redo2, RotateCcw, SlidersHorizontal, Trash2, Undo2, X } from "lucide-svelte";
  import Modal from "../Modal.svelte";
  import WidgetBody from "./WidgetBody.svelte";
  import { COLS, History, WIDGETS, WIDGET_TYPES, addWidget, defaultLayout, moveBy, moveTo, nameProblem, removeItem, resize, sameLayout, setHidden, setView, shown, type Item, type Layout, type WidgetType } from "$lib/dashlayout";
  import { ask, askText } from "$lib/dialogs.svelte";
  import type { Frame } from "$lib/hostframes";
  import type { HostSample, RateUnit } from "$lib/hostsample";
  import { dashboards } from "$lib/stores/dashboards.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Uuid } from "$lib/types";

  type Tab = "overview" | "cpu" | "memory" | "disks" | "network" | "gpu" | "temps" | "processes" | "ports" | "logins";
  let {
    hostId,
    s,
    frames,
    now,
    windowMs,
    rateUnit,
    goto,
    editing = $bindable(false),
  }: { hostId: Uuid; s: HostSample; frames: Frame[]; now: number; windowMs: number; rateUnit: RateUnit; goto: (t: Tab) => void; editing?: boolean } = $props();

  const label = $derived(vaultStore.hostById.get(hostId)?.data?.label ?? "host");

  /** What this host can show: a widget for hardware it does not have is not drawn at all. */
  function available(t: WidgetType): boolean {
    switch (t) {
      case "gpu":
        return s.gpus.length > 0;
      case "temperature":
        return s.capabilities.sensors;
      case "diskio":
        return s.disks.some((d) => !d.virtual_device);
      case "network":
        return s.network !== null;
      default:
        return true;
    }
  }

  let hist = $state.raw<History | null>(null);
  /** `History` is a plain class, so what the buttons depend on is mirrored here after every step. */
  let can = $state({ undo: false, redo: false });
  const sync = () => (can = { undo: !!hist?.canUndo, redo: !!hist?.canRedo });
  let draft = $state<Layout>({ items: [] });
  const layout = $derived<Layout>(editing ? draft : dashboards.layout(hostId));
  const visible = $derived(shown(layout, available));
  const hiddenItems = $derived(layout.items.filter((i) => i.hidden));
  const changed = $derived(editing && hist !== null && !sameLayout(draft, dashboards.layout(hostId)));
  let announce = $state("");
  let menu = $state<"add" | "templates" | null>(null);
  let applying = $state<{ id: string; name: string } | null>(null);
  let picked = $state<Record<string, boolean>>({});

  // -- editing -----------------------------------------------------------------------------------------------------------

  function start() {
    hist = new History(dashboards.layout(hostId));
    draft = hist.now;
    sync();
    editing = true;
    announce = "Editing the layout. Use the grip of a widget: arrow keys move it, Shift with the arrows resizes it.";
  }

  function commit(next: Layout) {
    if (hist) draft = hist.push(next);
    sync();
  }

  function undo() {
    if (hist) draft = hist.undo();
    sync();
  }
  function redo() {
    if (hist) draft = hist.redo();
    sync();
  }

  function save() {
    // A layout equal to the default is "no layout of its own": the host follows the default, as it did before.
    if (sameLayout(draft, defaultLayout())) dashboards.reset(hostId);
    else dashboards.save(hostId, draft);
    stop();
    ui.notify("info", `Dashboard layout saved for ${label}.`);
  }

  async function cancel() {
    if (changed && !(await ask("Discard the changes to this layout?", { title: "Discard changes", confirm: "Discard", danger: true }))) return;
    stop();
  }

  function stop() {
    editing = false;
    hist = null;
    menu = null;
    announce = "";
  }

  // The dashboard keeps no draft when it goes away, so a host switch or a closed window can only happen after a choice
  // (the window asks before closing while this is open, and the host and tab controls are off).
  onMount(() => () => {
    editing = false;
  });

  /** The grip keeps the keyboard focus through a move, which re-orders the elements. */
  async function refocus(key: string) {
    await tick();
    document.getElementById(`grip-${key}`)?.focus();
  }

  const titleOf = (i: Item) => WIDGETS[i.type].title;

  async function keyOnGrip(e: KeyboardEvent, item: Item) {
    const dir = e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : 0;
    if (!dir) return;
    e.preventDefault();
    if (e.shiftKey) {
      const horizontal = e.key === "ArrowLeft" || e.key === "ArrowRight";
      commit(resize(draft, item.key, item.w + (horizontal ? dir : 0), item.h + (horizontal ? 0 : dir)));
      const now = draft.items.find((i) => i.key === item.key);
      announce = `${titleOf(item)} is now ${now?.w} columns by ${now?.h} rows.`;
    } else {
      commit(moveBy(draft, item.key, dir, available));
      const at = shown(draft, available).findIndex((i) => i.key === item.key);
      announce = `${titleOf(item)} moved to position ${at + 1} of ${shown(draft, available).length}.`;
    }
    await refocus(item.key);
  }

  // -- dragging by the grip -------------------------------------------------------------------------------------------------

  let dragKey = $state<string | null>(null);
  let overKey = $state<string | null>(null);

  function dragStart(e: DragEvent, item: Item) {
    dragKey = item.key;
    e.dataTransfer?.setData("text/plain", item.key);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    const cell = (e.currentTarget as HTMLElement).closest("[data-cell]");
    if (cell && e.dataTransfer) e.dataTransfer.setDragImage(cell, 12, 12);
  }
  function dragOver(e: DragEvent, item: Item) {
    if (dragKey && dragKey !== item.key) {
      e.preventDefault();
      overKey = item.key;
    }
  }
  function drop(e: DragEvent, item: Item) {
    e.preventDefault();
    if (dragKey && dragKey !== item.key) {
      const to = visible.findIndex((i) => i.key === item.key);
      commit(moveTo(draft, dragKey, to, available));
      announce = `${WIDGETS[draft.items.find((i) => i.key === dragKey)?.type ?? "cpu"].title} moved.`;
    }
    dragKey = overKey = null;
  }

  // -- resizing by the corner -------------------------------------------------------------------------------------------------

  let gridEl = $state<HTMLDivElement>();
  let rem = $state(16);
  let preview = $state<{ key: string; w: number; h: number } | null>(null);
  let rz: { key: string; x: number; y: number; w: number; h: number } | null = null;
  const ROW = 4.25;
  const GAP = 0.75;

  onMount(() => {
    rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  });

  const px = (rows: number) => rem * (ROW * rows + GAP * (rows - 1));
  const sizeOf = (i: Item) => (preview?.key === i.key ? { w: preview.w, h: preview.h } : { w: i.w, h: i.h });

  function rzDown(e: PointerEvent, item: Item) {
    e.preventDefault();
    rz = { key: item.key, x: e.clientX, y: e.clientY, w: item.w, h: item.h };
    preview = { key: item.key, w: item.w, h: item.h };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function rzMove(e: PointerEvent) {
    if (!rz || !gridEl) return;
    const colStep = (gridEl.clientWidth + GAP * rem) / COLS;
    const rowStep = (ROW + GAP) * rem;
    const w = rz.w + Math.round((e.clientX - rz.x) / colStep);
    const h = rz.h + Math.round((e.clientY - rz.y) / rowStep);
    const item = draft.items.find((i) => i.key === rz!.key);
    if (item) {
      const next = resize({ items: [item] }, item.key, w, h).items[0];
      preview = { key: item.key, w: next.w, h: next.h };
    }
  }
  function rzUp() {
    if (rz && preview) {
      commit(resize(draft, rz.key, preview.w, preview.h));
      announce = `Resized to ${preview.w} columns by ${preview.h} rows.`;
    }
    rz = null;
    preview = null;
  }

  // -- the add menu and templates ---------------------------------------------------------------------------------------------------

  function add(t: WidgetType) {
    const next = addWidget(draft, t);
    commit(next);
    menu = null;
    announce = `${WIDGETS[t].title} added.`;
  }

  async function saveAsTemplate() {
    menu = null;
    const name = await askText("Name this layout so it can be applied to other hosts.", "", { title: "Save as template", confirm: "Save", placeholder: "Web servers" });
    if (name === null) return;
    const problem = nameProblem(name, dashboards.saved);
    if (problem) {
      ui.notify("error", problem);
      return;
    }
    dashboards.saveTemplate(name, draft);
    ui.notify("info", `Template "${name.trim()}" saved.`);
  }

  async function renameTemplate(id: string, current: string) {
    const name = await askText("New name for the template.", current, { title: "Rename template", confirm: "Rename" });
    if (name === null) return;
    const problem = nameProblem(name, dashboards.saved, id);
    if (problem) ui.notify("error", problem);
    else dashboards.rename(id, name);
  }

  async function deleteTemplate(id: string, name: string) {
    if (await ask(`Delete the template "${name}"?\n\nHosts that already use its layout keep their own copy.`, { title: "Delete template", confirm: "Delete", danger: true })) dashboards.remove(id);
  }

  function useHere(id: string, name: string) {
    const t = dashboards.saved.templates[id];
    if (!t) return;
    commit(structuredClone($state.snapshot(t.layout)));
    menu = null;
    announce = `Template "${name}" applied to this layout. Save to keep it.`;
  }

  function openApply(id: string, name: string) {
    menu = null;
    picked = {};
    applying = { id, name };
  }

  const allHosts = $derived(vaultStore.hosts.filter((h) => h.data).sort((a, b) => (a.data!.label ?? "").localeCompare(b.data!.label ?? "")));
  const pickedIds = $derived(Object.keys(picked).filter((k) => picked[k]));

  function applyTemplate() {
    if (!applying || !pickedIds.length) return;
    dashboards.apply(applying.id, pickedIds);
    ui.notify("info", `Layout "${applying.name}" applied to ${pickedIds.length} host${pickedIds.length === 1 ? "" : "s"}. Only the dashboard layout changed: sampling, permissions and alert settings were not touched.`);
    // This host's own draft is untouched; if it was among them, it shows the new layout the next time it is opened.
    applying = null;
  }

  const cellStyle = (i: Item) => {
    const z = sizeOf(i);
    return `--w:${z.w};--h:${z.h}`;
  };
</script>

<div class="space-y-3" data-testid="dashboard">
  <div class="sticky top-0 z-30 -mx-1 flex flex-wrap items-center gap-2 bg-panel/95 px-1 py-1 backdrop-blur">
    {#if editing}
      <p class="text-xs text-fg-muted">Editing the layout of <strong class="text-fg">{label}</strong>. Only this host changes.</p>
      <div class="relative ml-auto flex flex-wrap items-center gap-1">
        <button class="btn-secondary py-1 text-xs" aria-expanded={menu === "add"} onclick={() => (menu = menu === "add" ? null : "add")}><Plus size={13} /> Widgets</button>
        <button class="btn-secondary py-1 text-xs" aria-expanded={menu === "templates"} onclick={() => (menu = menu === "templates" ? null : "templates")}><LayoutTemplate size={13} /> Templates</button>
        <button class="icon-btn h-7 w-7" title="Undo" aria-label="Undo" disabled={!can.undo} onclick={undo}><Undo2 size={14} /></button>
        <button class="icon-btn h-7 w-7" title="Redo" aria-label="Redo" disabled={!can.redo} onclick={redo}><Redo2 size={14} /></button>
        <button class="btn-secondary py-1 text-xs" title="Put back the default layout (you can still undo or cancel)" onclick={() => commit(defaultLayout())}><RotateCcw size={13} /> Reset</button>
        <button class="btn-secondary py-1 text-xs" onclick={cancel}>Cancel</button>
        <button class="btn-primary py-1 text-xs" onclick={save}><Check size={13} /> Save layout</button>

        {#if menu === "add"}
          <div class="absolute right-0 top-9 z-40 max-h-[60vh] w-72 overflow-auto rounded-lg border border-line bg-panel p-2 shadow-xl" role="menu" aria-label="Widgets">
            {#if hiddenItems.length}
              <p class="px-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Hidden</p>
              {#each hiddenItems as h (h.key)}
                <div class="flex items-center gap-1 rounded px-2 py-1 text-xs hover:bg-panel-hover" role="none">
                  <button class="flex flex-1 items-center gap-2 text-left" role="menuitem" onclick={() => { commit(setHidden(draft, h.key, false)); menu = null; announce = `${titleOf(h)} shown.`; }}>
                    <Eye size={13} /> {titleOf(h)} <span class="text-fg-muted">· {WIDGETS[h.type].views.find((v) => v.id === h.view)?.label}</span>
                    {#if !available(h.type)}<span class="ml-auto text-[10px] text-fg-muted">not on this host</span>{/if}
                  </button>
                  <button class="icon-btn h-5 w-5" title="Remove it for good" aria-label="Remove {titleOf(h)}" onclick={() => commit(removeItem(draft, h.key))}><X size={11} /></button>
                </div>
              {/each}
            {/if}
            <p class="px-2 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Add another</p>
            {#each WIDGET_TYPES as t (t)}
              <button class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-panel-hover" role="menuitem" onclick={() => add(t)}>
                <Plus size={13} /> {WIDGETS[t].title}
                {#if WIDGETS[t].reads}<span class="ml-auto text-[10px] text-fg-muted">reads the host while shown</span>{:else if !available(t)}<span class="ml-auto text-[10px] text-fg-muted">not on this host</span>{/if}
              </button>
            {/each}
          </div>
        {/if}

        {#if menu === "templates"}
          <div class="absolute right-0 top-9 z-40 max-h-[60vh] w-80 overflow-auto rounded-lg border border-line bg-panel p-2 shadow-xl" role="menu" aria-label="Templates">
            <button class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-panel-hover" role="menuitem" onclick={saveAsTemplate}><Plus size={13} /> Save this layout as a template…</button>
            <p class="px-2 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Templates</p>
            {#each dashboards.templates as t (t.id)}
              <div class="rounded px-2 py-1 text-xs hover:bg-panel-hover" role="none">
                <div class="flex items-center gap-1"><span class="flex-1 truncate font-medium" title={t.name}>{t.name}</span>
                  <button class="icon-btn h-5 w-5" title="Rename" aria-label="Rename {t.name}" onclick={() => renameTemplate(t.id, t.name)}><Pencil size={11} /></button>
                  <button class="icon-btn h-5 w-5" title="Delete" aria-label="Delete {t.name}" onclick={() => deleteTemplate(t.id, t.name)}><Trash2 size={11} /></button>
                </div>
                <div class="mt-0.5 flex gap-1">
                  <button class="btn-ghost py-0 text-[11px]" role="menuitem" onclick={() => useHere(t.id, t.name)}>Use here</button>
                  <button class="btn-ghost py-0 text-[11px]" role="menuitem" onclick={() => openApply(t.id, t.name)}>Apply to hosts…</button>
                </div>
              </div>
            {:else}
              <p class="px-2 py-3 text-xs text-fg-muted">No templates yet.</p>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <p class="text-xs text-fg-muted">{dashboards.hasOwn(hostId) ? "Your layout for this host." : "The default layout."}</p>
      <button class="btn-secondary ml-auto py-1 text-xs" onclick={start} data-testid="customise"><SlidersHorizontal size={13} /> Customise</button>
    {/if}
  </div>

  <p class="sr-only" role="status" aria-live="polite">{announce}</p>

  <div bind:this={gridEl} class="grid-host" data-testid="dashboard-grid">
    <div class="dash">
      {#each visible as item (item.key)}
        {@const z = sizeOf(item)}
        <div
          class="cell relative {editing ? 'rounded-lg ring-2 ring-accent/40' : ''} {overKey === item.key ? 'ring-accent' : ''} {dragKey === item.key ? 'opacity-50' : ''}"
          style={cellStyle(item)}
          data-cell
          data-key={item.key}
          role="presentation"
          ondragover={(e) => dragOver(e, item)}
          ondragleave={() => overKey === item.key && (overKey = null)}
          ondrop={(e) => drop(e, item)}
        >
          <div class="h-full" inert={editing}>
            <WidgetBody {item} {s} {frames} {now} {windowMs} {rateUnit} {hostId} {goto} height={px(z.h)} />
          </div>
          {#if editing}
            <div class="absolute inset-x-1 top-1 z-20 flex items-center gap-1 rounded-md bg-base/90 px-1 py-0.5 text-[11px] shadow">
              <button
                id="grip-{item.key}"
                class="icon-btn h-5 w-5 cursor-grab"
                draggable="true"
                aria-label="Move {titleOf(item)}. Arrow keys move it, Shift with the arrows resizes it."
                title="Drag to move · arrow keys move · Shift+arrows resize"
                ondragstart={(e) => dragStart(e, item)}
                ondragend={() => (dragKey = overKey = null)}
                onkeydown={(e) => keyOnGrip(e, item)}
              ><GripVertical size={13} /></button>
              <span class="min-w-0 flex-1 truncate font-medium">{titleOf(item)} <span class="font-normal text-fg-muted">{z.w}×{z.h}</span></span>
              {#if WIDGETS[item.type].views.length > 1}
                <select class="input h-5 w-[4.75rem] py-0 pl-1 pr-0 text-[11px]" aria-label="How {titleOf(item)} is shown" value={item.view} onchange={(e) => commit(setView(draft, item.key, e.currentTarget.value))}>
                  {#each WIDGETS[item.type].views as v (v.id)}<option value={v.id}>{v.label}</option>{/each}
                </select>
              {/if}
              <button class="icon-btn h-5 w-5" title="Hide" aria-label="Hide {titleOf(item)}" onclick={() => { commit(setHidden(draft, item.key, true)); announce = `${titleOf(item)} hidden.`; }}><EyeOff size={12} /></button>
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="absolute bottom-0 right-0 z-20 h-4 w-4 cursor-nwse-resize rounded-tl bg-accent/50 hover:bg-accent"
              title="Drag to resize"
              data-testid="resize-{item.key}"
              onpointerdown={(e) => rzDown(e, item)}
              onpointermove={rzMove}
              onpointerup={rzUp}
              onpointercancel={rzUp}
            ></div>
          {/if}
        </div>
      {/each}
    </div>
    {#if visible.length === 0}
      <p class="rounded-lg border border-dashed border-line py-12 text-center text-sm text-fg-muted">Every widget is hidden. {editing ? "Add one from Widgets." : "Customise the layout to bring some back."}</p>
    {/if}
  </div>
</div>

{#if applying}
  <Modal title="Apply “{applying.name}”" onclose={() => (applying = null)} width="max-w-md">
    <div class="space-y-3 text-sm">
      <p class="text-xs text-fg-muted">Each host chosen gets its own copy of this layout, replacing the one it has. Only the dashboard layout changes: sampling intervals, permissions and alert settings are not touched.</p>
      <div class="flex items-center justify-between text-xs">
        <span>{pickedIds.length} of {allHosts.length} selected</span>
        <span class="flex gap-2"><button class="btn-ghost py-0 text-xs" onclick={() => (picked = Object.fromEntries(allHosts.map((h) => [h.id, true])))}>Select all</button><button class="btn-ghost py-0 text-xs" onclick={() => (picked = {})}>None</button></span>
      </div>
      <ul class="max-h-64 space-y-1 overflow-auto rounded-md border border-line p-2" aria-label="Hosts">
        {#each allHosts as h (h.id)}
          <li><label class="flex items-center gap-2 text-xs"><input type="checkbox" class="accent-input" bind:checked={picked[h.id]} /> {h.data?.label}{#if dashboards.hasOwn(h.id)}<span class="text-fg-muted">· has its own layout</span>{/if}</label></li>
        {/each}
      </ul>
      <div class="flex justify-end gap-2"><button class="btn-secondary" onclick={() => (applying = null)}>Cancel</button><button class="btn-primary" disabled={pickedIds.length === 0} onclick={applyTemplate}>Apply to {pickedIds.length || ""} host{pickedIds.length === 1 ? "" : "s"}</button></div>
    </div>
  </Modal>
{/if}

<style>
  .grid-host {
    container-type: inline-size;
  }
  .dash {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    grid-auto-rows: 4.25rem;
    grid-auto-flow: row dense;
    gap: 0.75rem;
  }
  .cell {
    grid-column: span var(--w);
    grid-row: span var(--h);
    min-width: 0;
    min-height: 0;
  }
  /* Twelve columns are for a wide window. In a narrower one each widget takes proportionally more, so a compact card is
     never squeezed into a sliver. */
  @container (max-width: 62rem) {
    .cell {
      grid-column: span min(12, calc(var(--w) * 2));
    }
  }
  @container (max-width: 40rem) {
    .cell {
      grid-column: span min(12, calc(var(--w) * 3));
    }
  }
</style>
