<script lang="ts">
  // A remote screen: a canvas that shows what a remote desktop protocol sends,
  // and turns keyboard, mouse and wheel into events for it. Shared by every
  // remote-desktop protocol; it knows nothing about RDP, only pictures and input.
  import { onDestroy } from "svelte";
  import { remotePoint, wheelUnits } from "$lib/rdp";
  import type { RdpInput } from "$lib/api";

  let {
    onInput,
    /** Show the picture at the size it is, scrolling, instead of shrinking it to the tab. */
    actualSize = false,
    disabled = false,
  }: {
    onInput: (input: RdpInput) => void;
    actualSize?: boolean;
    disabled?: boolean;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let wrapper = $state<HTMLDivElement>();
  let size = $state({ width: 0, height: 0 });
  let cursorCss = $state("default");
  let pendingMove: { x: number; y: number } | null = null;
  let raf = 0;

  /** Show a new picture size, blank. */
  export function resize(width: number, height: number) {
    size = { width, height };
    if (canvas) {
      canvas.width = width;
      canvas.height = height;
    }
  }

  /** Paint a changed rectangle. */
  export function draw(x: number, y: number, width: number, height: number, rgba: Uint8ClampedArray<ArrayBuffer>) {
    const ctx = canvas?.getContext("2d");
    if (!ctx) return;
    ctx.putImageData(new ImageData(rgba, width, height), x, y);
  }

  export function focus() {
    wrapper?.focus();
  }

  /** The pointer shape the remote wants: its own bitmap, the default, or none. */
  export function setCursor(shape: "default" | "hidden" | { hotX: number; hotY: number; width: number; height: number; rgba: Uint8ClampedArray<ArrayBuffer> }) {
    if (shape === "default") cursorCss = "default";
    else if (shape === "hidden") cursorCss = "none";
    else {
      const c = document.createElement("canvas");
      c.width = shape.width;
      c.height = shape.height;
      c.getContext("2d")?.putImageData(new ImageData(shape.rgba, shape.width, shape.height), 0, 0);
      // Browsers ignore cursors above 128 px.
      cursorCss = shape.width <= 128 && shape.height <= 128 ? `url(${c.toDataURL()}) ${shape.hotX} ${shape.hotY}, default` : "default";
    }
  }

  const flushMove = () => {
    raf = 0;
    if (pendingMove) {
      onInput({ type: "mouse_move", ...pendingMove });
      pendingMove = null;
    }
  };

  function point(e: MouseEvent) {
    if (!canvas || size.width === 0) return null;
    return remotePoint(e.clientX, e.clientY, canvas.getBoundingClientRect(), size);
  }

  function move(e: MouseEvent) {
    const p = point(e);
    if (!p || disabled) return;
    // At most one move per frame; a held-button drag still arrives in order.
    pendingMove = p;
    if (!raf) raf = requestAnimationFrame(flushMove);
  }

  function button(e: MouseEvent, down: boolean) {
    const p = point(e);
    if (!p || disabled) return;
    e.preventDefault();
    if (down) wrapper?.focus();
    pendingMove = null;
    onInput({ type: "mouse_move", ...p });
    onInput({ type: "button", button: e.button, down });
  }

  function wheel(e: WheelEvent) {
    if (disabled) return;
    e.preventDefault();
    const vertical = Math.abs(e.deltaY) >= Math.abs(e.deltaX);
    const units = wheelUnits(vertical ? e.deltaY : -e.deltaX, e.deltaMode);
    if (units !== 0) onInput({ type: "wheel", vertical, units });
  }

  function key(e: KeyboardEvent, down: boolean) {
    if (disabled) return;
    // The app's own shortcuts have already been handled (they run first and stop the event).
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat && !down) return;
    onInput({ type: "key", code: e.code, down });
  }

  const release = () => onInput({ type: "release_all" });

  onDestroy(() => {
    if (raf) cancelAnimationFrame(raf);
  });
</script>

<svelte:window onblur={release} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={wrapper}
  class="relative h-full w-full outline-none {actualSize ? 'overflow-auto' : 'flex items-center justify-center overflow-hidden'} bg-black"
  tabindex="0"
  role="application"
  aria-label="Remote desktop. Keys you type go to the remote computer."
  onkeydown={(e) => key(e, true)}
  onkeyup={(e) => key(e, false)}
  onblur={release}
>
  <canvas
    bind:this={canvas}
    class={actualSize ? "" : "max-h-full max-w-full object-contain"}
    style="cursor:{cursorCss}; image-rendering: auto;"
    onmousemove={move}
    onmousedown={(e) => button(e, true)}
    onmouseup={(e) => button(e, false)}
    onwheel={wheel}
    oncontextmenu={(e) => e.preventDefault()}
    aria-hidden="true"
  ></canvas>
</div>
