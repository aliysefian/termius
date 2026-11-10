<script lang="ts">
  // A thin draggable divider between two panels. It reports a new width for the panel on one side; the page keeps it.
  let { width, min = 180, max = 640, side = "left", onchange, label }: { width: number; min?: number; max?: number; side?: "left" | "right"; onchange: (w: number) => void; label: string } = $props();

  let dragging = $state(false);
  let startX = 0;
  let startW = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startW = width;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    document.body.style.userSelect = "none";
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    // The panel on the left grows with the pointer; the one on the right shrinks.
    const delta = side === "left" ? e.clientX - startX : startX - e.clientX;
    onchange(Math.round(Math.min(max, Math.max(min, startW + delta))));
  }
  function up() {
    dragging = false;
    document.body.style.userSelect = "";
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 10;
    const dir = (e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0) * (side === "left" ? 1 : -1);
    if (!dir) return;
    e.preventDefault();
    onchange(Math.min(max, Math.max(min, width + dir * step)));
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="group relative z-10 w-1 shrink-0 cursor-col-resize bg-line/40 transition-colors hover:bg-accent/60 focus-visible:bg-accent/60 focus-visible:outline-none {dragging ? 'bg-accent/70' : ''}"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onkeydown={key}
  ondblclick={() => onchange(Math.min(max, Math.max(min, side === "left" ? 300 : 300)))}
></div>
