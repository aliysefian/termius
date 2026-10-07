// The guided tour: the steps, how each finds the thing it points at, and where the card goes. Pure, so it can be
// tested; the overlay that draws it is components/Tour.svelte.
import type { Key } from "./i18n/en";

export interface Step {
  id: string;
  title: Key;
  body: Key;
  /** Where to point, tried in order; the first that is on screen wins. None found: the card sits in the middle. */
  targets: string[];
  /** Pages to be on first, so what the step points at exists. */
  view?: "hosts";
}

export const STEPS: Step[] = [
  { id: "rail", title: "tour.rail.title", body: "tour.rail.body", targets: ['nav[aria-label]'] },
  { id: "list", title: "tour.list.title", body: "tour.list.body", targets: ['section[aria-label="List panel"]'], view: "hosts" },
  { id: "manage", title: "tour.manage.title", body: "tour.manage.body", targets: ['nav button[aria-haspopup="menu"]'] },
  { id: "palette", title: "tour.palette.title", body: "tour.palette.body", targets: ['button[title^="Command palette"]', "main button.btn-secondary"] },
  { id: "quick", title: "tour.quick.title", body: "tour.quick.body", targets: ["main button.btn-primary", 'button[title^="New connection"]'] },
  { id: "end", title: "tour.end.title", body: "tour.end.body", targets: [] },
];

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Where to put the card (a `card.width` by `card.height` box) for a spotlight on `target`, inside a window of
 * `view`: to the right of it if there is room, else below, else above, else on the left; always inside the
 * window, and centred when there is nothing to point at.
 */
export function placeCard(target: Box | null, card: { width: number; height: number }, view: { width: number; height: number }, gap = 16, margin = 12): { left: number; top: number; side: "right" | "below" | "above" | "left" | "center" } {
  const clampX = (x: number) => Math.min(Math.max(x, margin), Math.max(margin, view.width - card.width - margin));
  const clampY = (y: number) => Math.min(Math.max(y, margin), Math.max(margin, view.height - card.height - margin));
  if (!target) return { left: clampX((view.width - card.width) / 2), top: clampY((view.height - card.height) / 2), side: "center" };
  const right = target.left + target.width + gap;
  if (right + card.width + margin <= view.width) return { left: right, top: clampY(target.top), side: "right" };
  const below = target.top + target.height + gap;
  if (below + card.height + margin <= view.height) return { left: clampX(target.left), top: below, side: "below" };
  const above = target.top - gap - card.height;
  if (above >= margin) return { left: clampX(target.left), top: above, side: "above" };
  const left = target.left - gap - card.width;
  if (left >= margin) return { left, top: clampY(target.top), side: "left" };
  return { left: clampX((view.width - card.width) / 2), top: clampY((view.height - card.height) / 2), side: "center" };
}

/** The spotlight's box: the target, a little larger. */
export const around = (b: Box, pad = 6): Box => ({ left: b.left - pad, top: b.top - pad, width: b.width + pad * 2, height: b.height + pad * 2 });
