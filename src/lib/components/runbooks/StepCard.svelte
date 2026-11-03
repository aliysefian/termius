<script lang="ts">
  import { ArrowDown, ArrowUp, EllipsisVertical, GripVertical, MinusCircle } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import { KIND_ICON } from "./kindicon";
  import { kindOf, type Obj } from "$lib/runbookdoc";
  import { KIND_BY, summary } from "$lib/runbookschema";

  let {
    step,
    index,
    total,
    rollback = false,
    selected,
    problems,
    onselect,
    onmove,
    onduplicate,
    onremove,
    ontoggle,
    ondrop,
  }: {
    step: Obj;
    index: number;
    total: number;
    rollback?: boolean;
    selected: boolean;
    problems: string[];
    onselect: () => void;
    onmove: (delta: number) => void;
    onduplicate: () => void;
    onremove: () => void;
    ontoggle: () => void;
    /** Another card was dropped here (its index). */
    ondrop: (from: number) => void;
  } = $props();

  const kind = $derived(kindOf(step));
  const Icon = $derived(kind ? KIND_ICON[kind] : MinusCircle);
  const disabled = $derived(step.disabled === true);
  const name = $derived(typeof step.name === "string" && step.name ? step.name : `Step ${index + 1}`);
  const when = $derived(step.when && typeof step.when === "object" ? (step.when as Obj) : null);
  const retries = $derived(typeof step.retries === "number" && step.retries > 0 ? step.retries : 0);
  const timeout = $derived(typeof step.timeout_secs === "number" ? step.timeout_secs : null);

  let menu = $state<{ x: number; y: number } | null>(null);
  let over = $state(false);

  function key(e: KeyboardEvent) {
    // Reordering from the keyboard: Alt with the arrow keys.
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      onmove(e.key === "ArrowUp" ? -1 : 1);
    } else if (e.key === "Enter" && e.target === e.currentTarget) {
      e.preventDefault();
      onselect();
    } else if (e.key === "Delete" && e.target === e.currentTarget) {
      e.preventDefault();
      onremove();
    }
  }

  function openMenu(e: MouseEvent | KeyboardEvent, el: HTMLElement) {
    e.preventDefault();
    const r = el.getBoundingClientRect();
    menu = e instanceof MouseEvent && e.clientX ? { x: e.clientX, y: e.clientY } : { x: r.right - 200, y: r.bottom };
  }

  const condition = $derived(
    when ? (typeof when.step === "string" ? `if ${when.step} ${when.exit !== undefined ? `exits ${when.exit}` : `doesn't exit ${when.exit_not}`}` : typeof when.param === "string" ? `if ${when.param} ${when.equals !== undefined ? "=" : "≠"} ${String(when.equals ?? when.not_equals)}` : "conditional") : null,
  );
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="group relative flex items-start gap-2 rounded-[10px] border bg-panel px-2.5 py-2 transition-colors {selected ? 'border-accent/60 bg-accent/5' : 'border-line hover:border-line/80 hover:bg-panel-hover'} {over ? 'ring-2 ring-accent/50' : ''} {disabled ? 'opacity-60' : ''}"
  role="group"
  aria-label="Step {index + 1}: {name}"
  tabindex="0"
  data-testid="step-card"
  draggable="true"
  ondragstart={(e) => e.dataTransfer?.setData("text/step", String(index))}
  ondragover={(e) => {
    if (e.dataTransfer?.types.includes("text/step")) {
      e.preventDefault();
      over = true;
    }
  }}
  ondragleave={() => (over = false)}
  ondrop={(e) => {
    over = false;
    const from = Number(e.dataTransfer?.getData("text/step"));
    if (Number.isInteger(from) && from !== index) {
      e.preventDefault();
      ondrop(from);
    }
  }}
  onkeydown={key}
  oncontextmenu={(e) => openMenu(e, e.currentTarget as HTMLElement)}
>
  <span class="mt-0.5 hidden cursor-grab text-fg-muted/50 group-hover:inline" aria-hidden="true"><GripVertical size={14} /></span>
  <span class="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-line bg-base text-[11px] font-semibold tabular-nums {problems.length ? 'border-danger text-danger' : 'text-fg-muted'}" aria-hidden="true">{index + 1}</span>
  <button class="min-w-0 flex-1 text-left focus-visible:outline-none" onclick={onselect} aria-expanded={selected} aria-label="Edit {name}">
    <span class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
      <Icon size={14} class="shrink-0 text-accent" aria-hidden="true" />
      <span class="truncate text-sm font-medium {disabled ? 'line-through' : ''}">{name}</span>
      <span class="rounded bg-fg-muted/15 px-1.5 text-[10px] uppercase tracking-wide text-fg-muted">{kind ? KIND_BY[kind].label : "no action"}</span>
      {#if disabled}<span class="rounded bg-fg-muted/15 px-1.5 text-[10px] text-fg-muted">off</span>{/if}
      {#if KIND_BY[kind ?? "run"].local && kind && kind !== "run"}<span class="text-[10px] text-fg-muted" title="Acts on this computer, not on the host">from this computer</span>{/if}
    </span>
    <span class="mt-0.5 block truncate font-mono text-xs text-fg-muted">{summary(step, kind)}</span>
    {#if condition || retries || timeout || step.on_error === "continue"}
      <span class="mt-1 flex flex-wrap gap-1.5 text-[10px] text-fg-muted">
        {#if condition}<span class="rounded border border-line px-1.5">{condition}</span>{/if}
        {#if step.on_error === "continue"}<span class="rounded border border-line px-1.5">carries on if it fails</span>{/if}
        {#if retries}<span class="rounded border border-line px-1.5">{retries} {retries === 1 ? "retry" : "retries"}</span>{/if}
        {#if timeout}<span class="rounded border border-line px-1.5">timeout {timeout} s</span>{/if}
      </span>
    {/if}
    {#each problems as p (p)}<span class="mt-1 block text-[11px] text-danger" role="alert">{p}</span>{/each}
  </button>
  <div class="flex shrink-0 items-center opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
    <button class="icon-btn h-6 w-6" title="Move up (Alt+↑)" aria-label="Move {name} up" disabled={index === 0} onclick={() => onmove(-1)}><ArrowUp size={13} /></button>
    <button class="icon-btn h-6 w-6" title="Move down (Alt+↓)" aria-label="Move {name} down" disabled={index === total - 1} onclick={() => onmove(1)}><ArrowDown size={13} /></button>
    <button class="icon-btn h-6 w-6" title="More" aria-label="More actions for {name}" aria-haspopup="menu" onclick={(e) => openMenu(e, e.currentTarget as HTMLElement)}><EllipsisVertical size={13} /></button>
  </div>
</div>

{#if menu}
  <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" tabindex="-1" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
  <div class="fixed z-50 w-52 rounded-xl border border-line bg-panel p-1 text-sm shadow-xl" use:keepInView={menu} role="menu" aria-label="Actions for {name}">
    {#snippet item(label: string, run: () => void, danger = false, off = false)}
      <button class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 {danger ? 'text-danger' : ''}" role="menuitem" disabled={off} onclick={() => { menu = null; run(); }}>{label}</button>
    {/snippet}
    {@render item("Edit", onselect)}
    {@render item("Duplicate", onduplicate)}
    {@render item(disabled ? "Turn on" : "Turn off", ontoggle)}
    {@render item("Move up", () => onmove(-1), false, index === 0)}
    {@render item("Move down", () => onmove(1), false, index === total - 1)}
    <div class="my-1 h-px bg-line" role="separator"></div>
    {@render item("Delete", onremove, true)}
  </div>
{/if}
