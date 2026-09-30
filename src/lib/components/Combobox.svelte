<script lang="ts" module>
  export interface ComboOption {
    value: string;
    label: string;
    /** Second line / right-hand hint, also searched. */
    detail?: string;
    /** Extra searchable text that isn't shown. */
    keywords?: string;
    /** Small colored dot before the label. */
    color?: string;
  }
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { Check, ChevronDown, Plus, X } from "lucide-svelte";
  import { matchesQuery, placeDropdown } from "$lib/popover";

  let {
    options,
    value = $bindable(""),
    id,
    placeholder = "",
    ariaLabel,
    creatable = false,
    createLabel = (q: string) => `Create “${q}”`,
    clearable = false,
    emptyText = "No matches",
    class: cls = "",
    inputClass = "",
    onchange,
  }: {
    options: ComboOption[];
    value?: string;
    id?: string;
    placeholder?: string;
    ariaLabel?: string;
    /** Free text: whatever is typed becomes the value, options are suggestions. */
    creatable?: boolean;
    createLabel?: (q: string) => string;
    clearable?: boolean;
    emptyText?: string;
    class?: string;
    inputClass?: string;
    onchange?: (value: string) => void;
  } = $props();

  const LIMIT = 300;
  const listId = `cb-${Math.random().toString(36).slice(2, 9)}`;
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();
  let open = $state(false);
  let query = $state("");
  /** Until the user types, an opened list shows everything, not just matches for the current value. */
  let typed = $state(false);
  let active = $state(0);
  let pos = $state<ReturnType<typeof placeDropdown> | null>(null);

  const selected = $derived(options.find((o) => o.value === value));
  const text = $derived(open ? query : creatable ? value : (selected?.label ?? ""));

  type Item = { kind: "option"; option: ComboOption } | { kind: "create"; value: string };
  const items = $derived.by((): Item[] => {
    const q = typed ? query.trim() : "";
    const hits = options.filter((o) => matchesQuery(q, o.label, o.detail, o.keywords, o.value)).slice(0, LIMIT);
    const out: Item[] = hits.map((option) => ({ kind: "option", option }));
    // Existing matches first, so Enter picks "Production" rather than creating "prod".
    if (creatable && q && !options.some((o) => o.value.toLowerCase() === q.toLowerCase())) out.push({ kind: "create", value: q });
    return out;
  });

  function place() {
    if (!open || !input) return;
    const r = input.getBoundingClientRect();
    const natural = Math.min(280, Math.max(36, (list?.scrollHeight ?? 0) || items.length * 34 + 8));
    pos = placeDropdown(r, natural, { width: window.innerWidth, height: window.innerHeight });
  }

  async function show() {
    if (open) return;
    open = true;
    typed = false;
    query = creatable ? value : (selected?.label ?? "");
    const i = items.findIndex((it) => it.kind === "option" && it.option.value === value);
    active = Math.max(0, i);
    await tick();
    place();
    await tick();
    place();
    scrollActive();
  }

  function hide() {
    open = false;
    typed = false;
    pos = null;
  }

  function choose(item: Item | undefined) {
    if (!item) return;
    const v = item.kind === "option" ? item.option.value : item.value;
    value = v;
    onchange?.(v);
    hide();
  }

  function scrollActive() {
    list?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function move(delta: number) {
    if (!items.length) return;
    active = (active + delta + items.length) % items.length;
    void tick().then(scrollActive);
  }

  function keydown(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
        e.preventDefault();
        if (!open) void show();
        else move(e.key === "ArrowDown" ? 1 : -1);
        break;
      case "PageDown":
      case "PageUp":
        if (open) {
          e.preventDefault();
          move(e.key === "PageDown" ? 8 : -8);
        }
        break;
      case "Enter":
        if (open) {
          e.preventDefault();
          // Free text with nothing highlighted keeps what was typed.
          if (creatable && typed && !items.length) hide();
          else choose(items[active]);
        }
        break;
      case "Escape":
        if (open) {
          // Close the list, not the dialog around it.
          e.preventDefault();
          e.stopPropagation();
          hide();
        }
        break;
      case "Tab":
        if (open) hide();
        break;
    }
  }

  function oninput(e: Event) {
    query = (e.currentTarget as HTMLInputElement).value;
    typed = true;
    active = 0;
    if (!open) {
      open = true;
      void tick().then(place);
    }
    if (creatable) {
      value = query;
      onchange?.(query);
    }
    void tick().then(place);
  }

  function clear(e: MouseEvent) {
    e.preventDefault();
    value = "";
    onchange?.("");
    query = "";
    input?.focus();
  }

  $effect(() => {
    if (!open) return;
    const reflow = () => place();
    window.addEventListener("resize", reflow);
    // Capture so scrolling a modal body or panel also repositions the list.
    window.addEventListener("scroll", reflow, true);
    return () => {
      window.removeEventListener("resize", reflow);
      window.removeEventListener("scroll", reflow, true);
    };
  });
</script>

<div class="relative {cls}">
  {#if selected?.color && !open}
    <span class="pointer-events-none absolute left-2.5 top-1/2 h-2 w-2 -translate-y-1/2 rounded-full" style:background={selected.color}></span>
  {/if}
  <input
    bind:this={input}
    {id}
    class="input pr-12 {selected?.color && !open ? 'pl-7' : ''} {inputClass}"
    role="combobox"
    aria-label={ariaLabel}
    aria-expanded={open}
    aria-controls={listId}
    aria-autocomplete="list"
    aria-activedescendant={open && items[active] ? `${listId}-${active}` : undefined}
    autocomplete="off"
    spellcheck="false"
    {placeholder}
    value={text}
    onfocus={() => void show()}
    onclick={() => void show()}
    onblur={hide}
    {oninput}
    onkeydown={keydown}
  />
  <div class="pointer-events-none absolute inset-y-0 right-1.5 flex items-center gap-0.5 text-fg-muted">
    {#if clearable && value}
      <button
        type="button"
        class="pointer-events-auto rounded p-0.5 hover:bg-panel-hover hover:text-fg"
        tabindex="-1"
        aria-label="Clear"
        onmousedown={clear}
      >
        <X size={13} />
      </button>
    {/if}
    <ChevronDown size={14} class="transition-transform {open ? 'rotate-180' : ''}" />
  </div>
</div>

{#if open}
  <ul
    bind:this={list}
    id={listId}
    role="listbox"
    class="fixed z-[70] overflow-y-auto rounded-md border border-line bg-panel py-1 text-sm shadow-2xl {pos ? '' : 'invisible'}"
    style:left="{pos?.left ?? 0}px"
    style:width="{pos?.width ?? 0}px"
    style:top={pos?.top !== undefined ? `${pos.top}px` : undefined}
    style:bottom={pos?.bottom !== undefined ? `${pos.bottom}px` : undefined}
    style:max-height="{pos?.maxHeight ?? 280}px"
    onmousedown={(e) => e.preventDefault()}
  >
    {#each items as item, i (item.kind === "create" ? "\u0000create" : item.option.value)}
      <!-- Keyboard focus stays in the input (ARIA combobox pattern), which handles the keys. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id="{listId}-{i}"
        data-i={i}
        role="option"
        aria-selected={i === active}
        class="flex cursor-pointer items-center gap-2 px-2.5 py-1.5 {i === active ? 'bg-accent/15 text-fg' : 'text-fg'}"
        onmousemove={() => (active = i)}
        onclick={() => choose(item)}
      >
        {#if item.kind === "create"}
          <Plus size={13} class="shrink-0 text-accent" />
          <span class="truncate">{createLabel(item.value)}</span>
        {:else}
          {#if item.option.color}
            <span class="h-2 w-2 shrink-0 rounded-full" style:background={item.option.color}></span>
          {/if}
          <span class="min-w-0 flex-1 truncate">{item.option.label}</span>
          {#if item.option.detail}
            <span class="max-w-[45%] shrink-0 truncate text-xs text-fg-muted">{item.option.detail}</span>
          {/if}
          <Check size={13} class="shrink-0 text-accent {item.option.value === value ? '' : 'invisible'}" />
        {/if}
      </li>
    {:else}
      <li class="px-2.5 py-2 text-xs text-fg-muted">{emptyText}</li>
    {/each}
  </ul>
{/if}
