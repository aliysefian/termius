<script lang="ts">
  import type { Snippet } from "svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { SIDEBAR_DEFAULT, SIDEBAR_MIN, dragTo, effectiveWidth, maxWidth } from "$lib/sidebarsize";

  let { children }: { children: Snippet } = $props();

  let windowWidth = $state(typeof window === "undefined" ? 1600 : window.innerWidth);
  const width = $derived(effectiveWidth(settings.prefs.sidebarWidth, windowWidth));
  let dragging = $state(false);
  /** Dragged past the snap point: shown closed, but only committed on release. */
  let collapsing = $state(false);
  let startX = 0;
  let startWidth = 0;
  let frame = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    collapsing = false;
    startX = e.clientX;
    startWidth = width;
    // Keep receiving moves even over the terminal.
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    const raw = startWidth + (e.clientX - startX);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (!dragging) return;
      const r = dragTo(raw, windowWidth);
      collapsing = r.hide;
      if (!r.hide) settings.prefs.sidebarWidth = r.width;
    });
  }

  function stop() {
    cancelAnimationFrame(frame);
    if (dragging && collapsing) {
      // Reopening restores the width it had before this drag.
      settings.prefs.sidebarWidth = startWidth;
      settings.prefs.sidebarHidden = true;
    }
    dragging = false;
    collapsing = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }

  /** Escape mid-drag puts everything back. */
  function cancel(e: KeyboardEvent) {
    if (!dragging || e.key !== "Escape") return;
    e.preventDefault();
    settings.prefs.sidebarWidth = startWidth;
    collapsing = false;
    stop();
  }

  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const r = dragTo(width + (e.key === "ArrowRight" ? step : -step), windowWidth);
      if (!r.hide) settings.prefs.sidebarWidth = r.width;
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      settings.prefs.sidebarWidth = SIDEBAR_DEFAULT;
    }
  }
</script>

<svelte:window bind:innerWidth={windowWidth} onkeydown={cancel} />

<div
  class="relative flex shrink-0 {dragging && !collapsing ? '' : 'transition-[width] duration-150 ease-out'}"
  style:width="{collapsing ? 0 : width}px"
>
  <!-- Content keeps its minimum width and is clipped while collapsing, so it
       slides away instead of squashing into an unreadable column. -->
  <div class="flex min-w-0 flex-1 overflow-hidden">
    <div class="@container flex flex-1" style:min-width="{SIDEBAR_MIN}px">
      {@render children()}
    </div>
  </div>
  {#if collapsing}
    <div class="pointer-events-none fixed left-14 top-1/2 z-30 -translate-y-1/2 rounded-md border border-line bg-panel px-2 py-1 text-xs text-fg-muted shadow-lg">
      Release to hide · drag back to keep
    </div>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="absolute -right-1 top-0 z-20 h-full w-2 cursor-col-resize outline-none
      after:absolute after:left-[3px] after:top-0 after:h-full after:w-0.5 after:transition-colors
      {dragging ? (collapsing ? 'after:bg-danger/70' : 'after:bg-accent') : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize the list panel"
    aria-valuenow={width}
    aria-valuemin={SIDEBAR_MIN}
    aria-valuemax={maxWidth(windowWidth)}
    tabindex="0"
    title="Drag to resize · double-click to reset · drag to the edge to hide"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={stop}
    onpointercancel={stop}
    onlostpointercapture={() => dragging && stop()}
    ondblclick={() => (settings.prefs.sidebarWidth = SIDEBAR_DEFAULT)}
    onkeydown={key}
  ></div>
</div>
