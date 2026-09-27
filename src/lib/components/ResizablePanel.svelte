<script lang="ts">
  import type { Snippet } from "svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { SIDEBAR_DEFAULT, dragTo, effectiveWidth, maxWidth } from "$lib/sidebarsize";

  let { children }: { children: Snippet } = $props();

  let windowWidth = $state(typeof window === "undefined" ? 1600 : window.innerWidth);
  const width = $derived(effectiveWidth(settings.prefs.sidebarWidth, windowWidth));
  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;
  let frame = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
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
      const r = dragTo(raw, windowWidth);
      if (r.hide) {
        stop();
        settings.prefs.sidebarHidden = true;
      } else {
        settings.prefs.sidebarWidth = r.width;
      }
    });
  }

  function stop() {
    dragging = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
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

<svelte:window bind:innerWidth={windowWidth} />

<div class="relative flex shrink-0" style:width="{width}px">
  <div class="flex min-w-0 flex-1">
    {@render children()}
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="absolute -right-1 top-0 z-20 h-full w-2 cursor-col-resize outline-none
      after:absolute after:left-[3px] after:top-0 after:h-full after:w-0.5 after:transition-colors
      {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize the list panel"
    aria-valuenow={width}
    aria-valuemin={200}
    aria-valuemax={maxWidth(windowWidth)}
    tabindex="0"
    title="Drag to resize · double-click to reset · drag to the edge to hide"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={stop}
    onpointercancel={stop}
    ondblclick={() => (settings.prefs.sidebarWidth = SIDEBAR_DEFAULT)}
    onkeydown={key}
  ></div>
</div>
