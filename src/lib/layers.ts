// Escape handling for stacked overlays (modals, prompts, the palette).
//
// Each open overlay pushes a handler; Escape reaches only the topmost one.
// Before this, every dialog listened on the window by itself, so Escape in
// the master-password prompt also closed the host form under it.
//
// The listener runs in the bubble phase so controls inside a dialog (a
// Combobox list, for example) can take Escape first with stopPropagation.
type Handler = () => void;

const stack: Handler[] = [];
let bound = false;

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape" || stack.length === 0) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  stack[stack.length - 1]();
}

/** Register the overlay on top; call the returned function when it closes. */
export function pushLayer(onEscape: Handler): () => void {
  if (!bound && typeof window !== "undefined") {
    window.addEventListener("keydown", onKey);
    bound = true;
  }
  stack.push(onEscape);
  return () => {
    const i = stack.lastIndexOf(onEscape);
    if (i >= 0) stack.splice(i, 1);
  };
}

/** How many overlays are open (for tests and for "is a dialog open" checks). */
export function layerCount(): number {
  return stack.length;
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null || el === document.activeElement);
}

/** Keep Tab and Shift+Tab inside `root`. Call from the root's keydown. */
export function trapTab(e: KeyboardEvent, root: HTMLElement) {
  if (e.key !== "Tab") return;
  const items = focusables(root);
  if (items.length === 0) {
    e.preventDefault();
    return;
  }
  const first = items[0];
  const last = items[items.length - 1];
  const current = document.activeElement as HTMLElement | null;
  if (e.shiftKey && (current === first || !root.contains(current))) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && (current === last || !root.contains(current))) {
    e.preventDefault();
    first.focus();
  }
}
