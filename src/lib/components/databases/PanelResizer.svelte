<script lang="ts">
  // The draggable edge between a side panel and the editor. `side` is the side of the panel the edge sits on:
  // dragging the right edge of the left panel to the right makes it wider; the left edge of the right panel, to the left.
  let { side, value, min, max, label, onchange, onreset }: { side: "left" | "right"; value: number; min: number; max: number; label: string; onchange: (w: number) => void; onreset: () => void } = $props();

  let dragging = $state(false);
  let startX = 0;
  let startW = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startW = value;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    const dx = e.clientX - startX;
    onchange(Math.round(Math.min(max, Math.max(min, side === "left" ? startW + dx : startW - dx))));
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
      const grow = (e.key === "ArrowRight") === (side === "left");
      onchange(Math.min(max, Math.max(min, value + (grow ? step : -step))));
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      onreset();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="relative z-10 w-1.5 shrink-0 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-line after:transition-colors {dragging ? 'after:bg-accent' : 'hover:after:bg-accent/60 focus-visible:after:bg-accent'}"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  title="Drag to resize · double-click to reset"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={stop}
  onpointercancel={stop}
  onlostpointercapture={() => dragging && stop()}
  ondblclick={onreset}
  onkeydown={key}
></div>
