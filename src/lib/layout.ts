// Split-pane layout for a terminal tab: a binary tree of splits whose leaves
// are pane ids. The tree only computes rectangles. Panes themselves are
// rendered as one flat, keyed list positioned by those rectangles, so
// splitting or closing never moves a terminal to a new DOM parent (which
// would unmount it and drop its SSH session).

export type Dir = "row" | "column";

export type LayoutNode =
  | { type: "pane"; paneId: string }
  | { type: "split"; id: string; dir: Dir; ratio: number; a: LayoutNode; b: LayoutNode };

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Divider {
  splitId: string;
  dir: Dir;
  /** The split's own area, to convert pointer positions into a ratio. */
  area: Rect;
  /** Where the boundary line sits. */
  at: Rect;
}

export const MAX_PANES = 6;
export const MIN_RATIO = 0.15;

let seq = 0;
const splitId = () => `split-${++seq}-${Date.now().toString(36)}`;

export function leaf(paneId: string): LayoutNode {
  return { type: "pane", paneId };
}

export function paneIds(node: LayoutNode): string[] {
  return node.type === "pane" ? [node.paneId] : [...paneIds(node.a), ...paneIds(node.b)];
}

/** Replace `target`'s leaf with a split holding it and `newPaneId`. */
export function split(node: LayoutNode, target: string, newPaneId: string, dir: Dir): LayoutNode {
  if (node.type === "pane") {
    return node.paneId === target
      ? { type: "split", id: splitId(), dir, ratio: 0.5, a: node, b: leaf(newPaneId) }
      : node;
  }
  return { ...node, a: split(node.a, target, newPaneId, dir), b: split(node.b, target, newPaneId, dir) };
}

/** Remove a pane; its sibling takes the parent split's place. */
export function remove(node: LayoutNode, target: string): LayoutNode | null {
  if (node.type === "pane") return node.paneId === target ? null : node;
  const a = remove(node.a, target);
  const b = remove(node.b, target);
  if (!a) return b;
  if (!b) return a;
  return { ...node, a, b };
}

export function setRatio(node: LayoutNode, id: string, ratio: number): LayoutNode {
  if (node.type === "pane") return node;
  if (node.id === id) return { ...node, ratio: Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, ratio)) };
  return { ...node, a: setRatio(node.a, id, ratio), b: setRatio(node.b, id, ratio) };
}

/** Percent rectangles (0..100) for every pane and divider. */
export function layoutRects(
  node: LayoutNode,
  area: Rect = { x: 0, y: 0, w: 100, h: 100 },
  out: { panes: Map<string, Rect>; dividers: Divider[] } = { panes: new Map(), dividers: [] },
) {
  if (node.type === "pane") {
    out.panes.set(node.paneId, area);
    return out;
  }
  if (node.dir === "row") {
    const wa = area.w * node.ratio;
    layoutRects(node.a, { ...area, w: wa }, out);
    layoutRects(node.b, { ...area, x: area.x + wa, w: area.w - wa }, out);
    out.dividers.push({ splitId: node.id, dir: "row", area, at: { x: area.x + wa, y: area.y, w: 0, h: area.h } });
  } else {
    const ha = area.h * node.ratio;
    layoutRects(node.a, { ...area, h: ha }, out);
    layoutRects(node.b, { ...area, y: area.y + ha, h: area.h - ha }, out);
    out.dividers.push({ splitId: node.id, dir: "column", area, at: { x: area.x, y: area.y + ha, w: area.w, h: 0 } });
  }
  return out;
}
