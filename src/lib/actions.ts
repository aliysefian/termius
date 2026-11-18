import { placeMenu } from "./popover";

/**
 * Keeps a `position: fixed` context menu inside the window. Pass the pointer
 * position; the menu is measured after it renders and moved (flipped up or
 * left) so every item stays visible, scrolling only if the window is tiny.
 */
export function keepInView(node: HTMLElement, at: { x: number; y: number }) {
  function place(p: { x: number; y: number }) {
    node.style.maxHeight = "";
    node.style.overflowY = "";
    const r = node.getBoundingClientRect();
    const pos = placeMenu(p.x, p.y, r.width, r.height, { width: window.innerWidth, height: window.innerHeight });
    node.style.left = `${pos.left}px`;
    node.style.top = `${pos.top}px`;
    if (pos.maxHeight !== undefined) {
      node.style.maxHeight = `${pos.maxHeight}px`;
      node.style.overflowY = "auto";
    }
  }
  place(at);
  return { update: place };
}

/**
 * For an overlay that floats over a pane's corner (the terminal's action toolbar): publishes the room it
 * takes, measured from its left edge to the pane's right edge, as `--pane-toolbar-inset` on the pane (its
 * parent). Notices and the Find bar inside the pane stop short of that edge instead of being drawn
 * underneath it. Measured rather than hard-coded, since the toolbar can grow a group of buttons on its left.
 */
export function reservePaneInset(node: HTMLElement) {
  const pane = node.parentElement;
  if (!pane) return;
  const publish = () => {
    const inset = Math.max(0, pane.getBoundingClientRect().right - node.getBoundingClientRect().left);
    pane.style.setProperty("--pane-toolbar-inset", `${Math.ceil(inset)}px`);
  };
  publish();
  if (typeof ResizeObserver === "undefined") return { destroy: () => pane.style.removeProperty("--pane-toolbar-inset") };
  const ro = new ResizeObserver(publish);
  ro.observe(node);
  ro.observe(pane);
  return {
    destroy() {
      ro.disconnect();
      pane.style.removeProperty("--pane-toolbar-inset");
    },
  };
}
