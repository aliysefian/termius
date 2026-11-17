import { applyPatch, clamp01, emptyState, finalizeStep, partialStep, quantum, stepDuration } from "./state";
import type { Chapter, CursorFrame, DemoState, TimelineItem, TutorialDefinition, TutorialStep } from "./types";

export const easeInOut = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

const CURSOR_STEPS = new Set<TutorialStep["type"]>(["moveCursor", "click", "doubleClick"]);

/** A compiled tutorial: absolute start/end for every step, and the state after each one. */
export class Timeline {
  readonly items: TimelineItem[] = [];
  readonly chapters: Chapter[] = [];
  readonly duration: number;
  readonly initialState: DemoState;
  /** checkpoints[i] is the state once item i has finished. */
  private readonly checkpoints: DemoState[] = [];
  private readonly clickItems: TimelineItem[] = [];
  private readonly captions: { start: number; text: string }[] = [];

  constructor(readonly def: TutorialDefinition) {
    this.initialState = applyPatch(emptyState(), def.initial);
    let t = 0;
    let cursor: string | null = null;
    let state = this.initialState;
    for (const [index, step] of def.steps.entries()) {
      const dur = Math.max(0, stepDuration(step));
      const item: TimelineItem = { index, step, start: t, end: t + dur, cursorFrom: cursor };
      this.items.push(item);
      if (CURSOR_STEPS.has(step.type)) cursor = (step as { target: string }).target;
      if (step.type === "click" || step.type === "doubleClick") this.clickItems.push(item);
      if (step.chapter) {
        const prev = this.chapters[this.chapters.length - 1];
        if (prev) prev.end = t;
        this.chapters.push({ id: `c${this.chapters.length + 1}`, title: step.chapter, start: t, end: t, firstStep: index });
      }
      if (step.note) this.captions.push({ start: t, text: step.note });
      state = finalizeStep(state, step);
      this.checkpoints.push(state);
      t += dur;
    }
    this.duration = t;
    const last = this.chapters[this.chapters.length - 1];
    if (last) last.end = t;
  }

  get posterTime(): number {
    return this.def.posterTime ?? Math.round(this.duration * 0.7);
  }

  /** Index of the last step that has started at time t, or -1. */
  indexAt(t: number): number {
    let lo = 0;
    let hi = this.items.length - 1;
    let ans = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (this.items[mid]!.start <= t) {
        ans = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return ans;
  }

  /** The complete UI state at time t. Pure and independent of how the playhead got here. */
  stateAt(t: number): DemoState {
    const i = this.indexAt(t);
    if (i < 0) return this.initialState;
    const item = this.items[i]!;
    if (t >= item.end) return this.checkpoints[i]!;
    const before = i > 0 ? this.checkpoints[i - 1]! : this.initialState;
    return partialStep(before, item.step, clamp01((t - item.start) / (item.end - item.start)));
  }

  /** A cheap key that changes exactly when the rendered state would. */
  keyAt(t: number): string {
    const i = this.indexAt(t);
    if (i < 0) return "-1";
    const item = this.items[i]!;
    if (t >= item.end) return `${i}:end`;
    return `${i}:${quantum(item.step, clamp01((t - item.start) / (item.end - item.start)))}`;
  }

  caption(t: number): string | null {
    let text: string | null = null;
    for (const c of this.captions) {
      if (c.start <= t) text = c.text;
      else break;
    }
    return text;
  }

  chapterAt(t: number): Chapter | null {
    let found: Chapter | null = null;
    for (const c of this.chapters) {
      if (c.start <= t) found = c;
      else break;
    }
    return found;
  }

  /**
   * Cursor pose at time t. `resolve` maps a target to a normalised position, or null while its element is
   * not on screen; unresolved ends fall back to `fallback` (the last position seen for that target).
   */
  cursorAt(t: number, resolve: (target: string) => { x: number; y: number } | null): CursorFrame {
    const start = this.def.cursorStart ?? { x: 0.9, y: 0.92 };
    const pos = (target: string | null) => (target ? resolve(target) : null) ?? (target === null ? start : null);
    const i = this.indexAt(t);
    let click = -1;
    let clickAge = Infinity;
    let clickDouble = false;
    for (let k = this.clickItems.length - 1; k >= 0; k--) {
      const c = this.clickItems[k]!;
      if (c.start <= t) {
        click = k;
        clickAge = t - c.start;
        clickDouble = c.step.type === "doubleClick";
        break;
      }
    }
    if (i < 0) return { ...start, pressed: false, visible: false, target: null, clickIndex: click, clickAge, clickDouble };
    const item = this.items[i]!;
    const p = item.end > item.start ? clamp01((t - item.start) / (item.end - item.start)) : 1;
    const done = t >= item.end;
    let target: string | null = item.cursorFrom;
    let from = pos(item.cursorFrom) ?? start;
    let to = from;
    let amount = 1;
    if (item.step.type === "moveCursor") {
      target = item.step.target;
      to = pos(target) ?? from;
      amount = done ? 1 : easeInOut(p);
    } else if (item.step.type === "click" || item.step.type === "doubleClick") {
      target = item.step.target;
      from = to = pos(target) ?? from;
    } else if (item.cursorFrom) {
      from = to = pos(item.cursorFrom) ?? start;
    }
    const x = from.x + (to.x - from.x) * amount;
    const y = from.y + (to.y - from.y) * amount;
    let pressed = false;
    if (item.step.type === "click" || item.step.type === "doubleClick") {
      const d = item.end - item.start;
      const local = t - item.start;
      pressed = item.step.type === "click" ? local < Math.min(160, d * 0.5) : local < 140 || (local > 220 && local < 360);
    }
    return { x, y, pressed, visible: true, target, clickIndex: click, clickAge, clickDouble };
  }

  /** Time at which the nth chapter starts. */
  chapterStart(n: number): number {
    return this.chapters[Math.max(0, Math.min(n, this.chapters.length - 1))]?.start ?? 0;
  }

  /** Start and end of the chapter with this id/title (for looping a slice of a tutorial). */
  range(chapter: string): [number, number] {
    const c = this.chapters.find((c) => c.id === chapter || c.title === chapter);
    return c ? [c.start, c.end] : [0, this.duration];
  }
}
