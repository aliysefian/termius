// tmux window layout strings, as sent in `%layout-change` and printed by `list-windows -F '#{window_layout}'`.
//
//   bb62,159x48,0,0{79x48,0,0,79x48,80,0[79x24,80,0,2,79x23,80,25,3]}
//   ^csum ^cell     ^ `{…}` children side by side, `[…]` stacked; a leaf ends in its pane number.
//
// Every cell is `WxH,X,Y` in characters. Siblings are separated by one column or row (the divider tmux draws),
// which is why the numbers in a layout don't add up to the parent's size.

export type TmuxCell = { x: number; y: number; w: number; h: number };

export type TmuxLayoutNode =
  | (TmuxCell & { type: "pane"; /** The number after `%` in a pane id. */ paneId: number })
  | (TmuxCell & { type: "split"; /** `row`: children sit side by side (`{}`); `column`: stacked (`[]`). */ dir: "row" | "column"; children: TmuxLayoutNode[] });

export interface TmuxLayout {
  checksum: string;
  root: TmuxLayoutNode;
}

/** tmux's own 16-bit layout checksum (layout_checksum in layout-custom.c), as four hex digits. */
export function layoutChecksum(body: string): string {
  let csum = 0;
  for (let i = 0; i < body.length; i++) {
    csum = (csum >> 1) + ((csum & 1) << 15);
    csum = (csum + body.charCodeAt(i)) & 0xffff;
  }
  return csum.toString(16).padStart(4, "0");
}

class Reader {
  i = 0;
  constructor(readonly s: string) {}
  peek() {
    return this.s[this.i];
  }
  number(): number {
    const start = this.i;
    while (this.i < this.s.length && this.s.charCodeAt(this.i) >= 48 && this.s.charCodeAt(this.i) <= 57) this.i++;
    if (start === this.i) throw new Error(`expected a number at ${start}`);
    return Number(this.s.slice(start, this.i));
  }
  expect(c: string) {
    if (this.s[this.i] !== c) throw new Error(`expected "${c}" at ${this.i}`);
    this.i++;
  }
}

function node(r: Reader): TmuxLayoutNode {
  const w = r.number();
  r.expect("x");
  const h = r.number();
  r.expect(",");
  const x = r.number();
  r.expect(",");
  const y = r.number();
  const cell = { x, y, w, h };
  const open = r.peek();
  if (open === "{" || open === "[") {
    const close = open === "{" ? "}" : "]";
    r.i++;
    const children: TmuxLayoutNode[] = [node(r)];
    while (r.peek() === ",") {
      r.i++;
      children.push(node(r));
    }
    r.expect(close);
    return { type: "split", dir: open === "{" ? "row" : "column", ...cell, children };
  }
  r.expect(",");
  return { type: "pane", ...cell, paneId: r.number() };
}

/**
 * Parse a layout string. Returns `null` for anything malformed, or whose checksum is wrong, rather than
 * guessing: a half-read layout would resize panes to nonsense.
 */
export function parseLayout(layout: string): TmuxLayout | null {
  const m = layout.trim().match(/^([0-9a-f]{4}),(.+)$/);
  if (!m) return null;
  if (layoutChecksum(m[2]) !== m[1]) return null;
  try {
    const r = new Reader(m[2]);
    const root = node(r);
    return r.i === m[2].length ? { checksum: m[1], root } : null;
  } catch {
    return null;
  }
}

/** Every pane of a layout with its cell, in tmux's order (left to right, top to bottom). */
export function layoutPanes(root: TmuxLayoutNode): (TmuxCell & { paneId: number })[] {
  if (root.type === "pane") return [{ x: root.x, y: root.y, w: root.w, h: root.h, paneId: root.paneId }];
  return root.children.flatMap(layoutPanes);
}

/** The window's size in characters. */
export function layoutSize(layout: TmuxLayout): { cols: number; rows: number } {
  return { cols: layout.root.w, rows: layout.root.h };
}

/** A line between two siblings of a split, which a drag moves with `resize-pane`. */
export interface TmuxDivider {
  dir: "row" | "column";
  /** Its own cell: one character wide (row) or tall (column). */
  at: TmuxCell;
  /** The sibling before it, whose size the drag changes. */
  before: TmuxLayoutNode;
  /** A pane on the edge facing the divider, to aim `resize-pane` at. */
  edgePaneId: number;
}

function edgePane(n: TmuxLayoutNode, dir: "row" | "column"): number {
  if (n.type === "pane") return n.paneId;
  // The last child along the split's axis touches the divider that follows; across it, any child will do.
  return edgePane(n.dir === dir ? n.children[n.children.length - 1] : n.children[0], dir);
}

export function layoutDividers(root: TmuxLayoutNode): TmuxDivider[] {
  if (root.type === "pane") return [];
  const out: TmuxDivider[] = [];
  root.children.forEach((c, i) => {
    out.push(...layoutDividers(c));
    if (i === root.children.length - 1) return;
    out.push({
      dir: root.dir,
      at: root.dir === "row" ? { x: c.x + c.w, y: root.y, w: 1, h: root.h } : { x: root.x, y: c.y + c.h, w: root.w, h: 1 },
      before: c,
      edgePaneId: edgePane(c, root.dir),
    });
  });
  return out;
}
