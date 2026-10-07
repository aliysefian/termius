<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { STEPS, around, placeCard, type Box } from "$lib/tour";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let step = $state(0);
  let spot = $state<Box | null>(null);
  let cardEl = $state<HTMLDivElement>();
  let pos = $state({ left: 0, top: 0, side: "center" as string });
  const current = $derived(STEPS[step]);
  const last = $derived(step === STEPS.length - 1);

  function find(selectors: string[]): HTMLElement | null {
    for (const sel of selectors) {
      for (const el of document.querySelectorAll<HTMLElement>(sel)) {
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) return el;
      }
    }
    return null;
  }

  async function show() {
    const s = current;
    // The steps point at the real sidebar and list, so be where they are.
    if (s.view === "hosts") {
      ui.view = "hosts";
      settings.prefs.sidebarHidden = false;
    }
    await tick();
    const el = find(s.targets);
    el?.scrollIntoView({ block: "nearest" });
    const r = el?.getBoundingClientRect();
    spot = r ? around({ left: r.left, top: r.top, width: r.width, height: r.height }) : null;
    await tick();
    const c = cardEl?.getBoundingClientRect();
    pos = placeCard(spot, { width: c?.width ?? 340, height: c?.height ?? 200 }, { width: window.innerWidth, height: window.innerHeight });
    cardEl?.focus();
  }

  $effect(() => {
    void step;
    void show();
  });

  onMount(() => {
    const again = () => void show();
    window.addEventListener("resize", again);
    return () => window.removeEventListener("resize", again);
  });

  function next() {
    if (last) end();
    else step++;
  }
  function back() {
    if (step > 0) step--;
  }
  function end() {
    ui.tour = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      end();
    } else if (e.key === "ArrowRight" || e.key === "Enter") {
      // A focused button's own Enter is its click; leave it.
      if (e.key === "Enter" && (e.target as HTMLElement).tagName === "BUTTON") return;
      e.preventDefault();
      next();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      back();
    } else if (e.key === "Tab") {
      // Keep the focus on the card's three buttons.
      const items = [...(cardEl?.querySelectorAll<HTMLElement>("button") ?? [])];
      if (!items.length) return;
      const at = items.indexOf(document.activeElement as HTMLElement);
      const to = e.shiftKey ? (at <= 0 ? items.length - 1 : at - 1) : at === items.length - 1 ? 0 : at + 1;
      e.preventDefault();
      items[to].focus();
    }
  }
</script>

<!-- A dim layer with a hole where the step points; clicks outside the card do nothing, so the tour can't be tripped over. -->
<div class="fixed inset-0 z-[90]" role="presentation" data-testid="tour">
  {#if spot}
    <div
      class="pointer-events-none absolute rounded-xl ring-2 ring-accent transition-all duration-200"
      style:left="{spot.left}px"
      style:top="{spot.top}px"
      style:width="{spot.width}px"
      style:height="{spot.height}px"
      style:box-shadow="0 0 0 9999px rgba(0,0,0,0.62)"
      data-testid="tour-spot"
    ></div>
  {:else}
    <div class="absolute inset-0 bg-black/60"></div>
  {/if}

  <div
    bind:this={cardEl}
    class="anim-pop absolute w-[340px] max-w-[calc(100vw-24px)] rounded-xl border border-line bg-panel p-4 shadow-2xl outline-none"
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    role="dialog"
    aria-modal="true"
    aria-labelledby="tour-title"
    aria-describedby="tour-body"
    tabindex="-1"
    {onkeydown}
  >
    <div class="mb-1 flex items-center justify-between text-[11px] text-fg-muted">
      <span>{t("tour.step", { n: step + 1, total: STEPS.length })}</span>
      <span class="flex gap-1" aria-hidden="true">
        {#each STEPS as s, i (s.id)}<span class="h-1.5 w-1.5 rounded-full {i === step ? 'bg-accent' : 'bg-line'}"></span>{/each}
      </span>
    </div>
    <h2 id="tour-title" class="text-[16px] leading-6 font-semibold">{t(current.title)}</h2>
    <p id="tour-body" class="mt-1 text-sm text-fg-muted">{t(current.body)}</p>
    <div class="mt-4 flex items-center justify-between gap-2">
      <button class="btn-ghost py-1 text-xs" onclick={end}>{t("common.close")}</button>
      <span class="flex gap-2">
        <button class="btn-secondary py-1 text-xs" disabled={step === 0} onclick={back}>{t("common.back")}</button>
        <button class="btn-primary py-1 text-xs" onclick={next}>{last ? t("common.done") : t("common.next")}</button>
      </span>
    </div>
  </div>
</div>
