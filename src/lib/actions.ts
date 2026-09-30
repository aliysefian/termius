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
