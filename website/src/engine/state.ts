import type { DemoState, Patch, TutorialStep } from "./types";

export function emptyState(): DemoState {
  return {
    view: "hosts",
    panel: null,
    menu: null,
    fields: {},
    selected: {},
    terminals: {},
    progress: {},
    flags: {},
    toast: null,
    highlight: null,
    hover: null,
  };
}

/** Pure: returns a new state, sharing untouched branches. */
export function applyPatch(state: DemoState, patch: Patch): DemoState {
  const next: DemoState = { ...state };
  const { fields, selected, terminals, progress, flags, ...rest } = patch;
  Object.assign(next, rest);
  if (fields) next.fields = { ...state.fields, ...fields };
  if (selected) next.selected = { ...state.selected, ...selected };
  if (terminals) next.terminals = { ...state.terminals, ...terminals };
  if (progress) next.progress = { ...state.progress, ...progress };
  if (flags) next.flags = { ...state.flags, ...flags };
  return next;
}

export const clamp01 = (n: number) => (n < 0 ? 0 : n > 1 ? 1 : n);

/** The id before an optional `@fx,fy` anchor suffix. */
export function targetId(target: string): string {
  const at = target.indexOf("@");
  return at < 0 ? target : target.slice(0, at);
}

/** The fractional anchor inside the element (default: its centre). */
export function targetAnchor(target: string): { fx: number; fy: number } {
  const at = target.indexOf("@");
  if (at < 0) return { fx: 0.5, fy: 0.5 };
  const [a, b] = target.slice(at + 1).split(",");
  const fx = Number(a);
  const fy = Number(b);
  return { fx: Number.isFinite(fx) ? fx : 0.5, fy: Number.isFinite(fy) ? fy : 0.5 };
}

const cadenceCache = new Map<string, number[]>();

/** Cumulative typing time per character, 0..1. Spaces and punctuation pause a little; the jitter is a fixed function of the text. */
function cadence(text: string): number[] {
  const hit = cadenceCache.get(text);
  if (hit) return hit;
  let total = 0;
  const cum: number[] = [];
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!;
    const jitter = ((i * 7 + text.charCodeAt(i) * 13) % 5) * 0.12;
    total += 1 + jitter + (ch === " " ? 0.7 : ".-/@:_,".includes(ch) ? 0.45 : 0);
    cum.push(total);
  }
  const out = cum.map((v) => v / (total || 1));
  cadenceCache.set(text, out);
  return out;
}

/** How many characters of `text` are on screen at progress p (0..1). */
export function typedCount(text: string, p: number): number {
  if (p >= 1) return text.length;
  const cum = cadence(text);
  let n = 0;
  while (n < cum.length && cum[n]! <= p) n++;
  return n;
}

/** Number of "units" a step has revealed at progress p; the UI only re-renders when this changes. */
export function quantum(step: TutorialStep, p: number): number {
  switch (step.type) {
    case "type":
      return typedCount(step.text, p);
    case "terminalOutput":
      return Math.floor(step.lines.length * p);
    case "progress":
      return Math.round(step.from + (step.to - step.from) * p);
    case "moveCursor":
      return p >= 0.85 ? 1 : 0; // hover begins as the pointer arrives
    default:
      return 0;
  }
}

/** The state after a step has finished. */
export function finalizeStep(state: DemoState, step: TutorialStep): DemoState {
  switch (step.type) {
    case "moveCursor":
      return { ...state, hover: step.target };
    case "click":
    case "doubleClick":
      return applyPatch({ ...state, hover: step.target }, step.effect ?? {});
    case "type": {
      if (step.submit) {
        const prev = state.terminals[step.submit.terminal] ?? [];
        return applyPatch(state, {
          fields: { [step.target]: "" },
          terminals: { [step.submit.terminal]: [...prev, step.submit.prompt + step.text] },
        });
      }
      return applyPatch(state, { fields: { [step.target]: step.text } });
    }
    case "showPanel":
      return { ...state, panel: step.panel, menu: null };
    case "terminalOutput": {
      const prev = state.terminals[step.terminal] ?? [];
      return applyPatch(state, { terminals: { [step.terminal]: [...prev, ...step.lines] } });
    }
    case "highlight":
      return { ...state, highlight: step.target };
    case "set":
      return applyPatch(state, step.patch);
    case "toast":
      return { ...state, toast: null };
    case "progress":
      return applyPatch(state, { progress: { [step.key]: step.to } });
    case "wait":
      return state;
  }
}

/** The state part-way through a step (0 <= p < 1). Instant steps never reach here. */
export function partialStep(before: DemoState, step: TutorialStep, p: number): DemoState {
  switch (step.type) {
    case "type": {
      return applyPatch(before, { fields: { [step.target]: step.text.slice(0, typedCount(step.text, p)) } });
    }
    case "terminalOutput": {
      const n = Math.floor(step.lines.length * p);
      const prev = before.terminals[step.terminal] ?? [];
      return applyPatch(before, { terminals: { [step.terminal]: [...prev, ...step.lines.slice(0, n)] } });
    }
    case "progress": {
      const v = Math.round(step.from + (step.to - step.from) * p);
      return applyPatch(before, { progress: { [step.key]: v } });
    }
    case "toast":
      return { ...before, toast: { text: step.text, kind: step.kind ?? "success" } };
    case "highlight":
      return { ...before, highlight: step.target };
    case "moveCursor":
      return p >= 0.85 ? { ...before, hover: step.target } : before;
    case "click":
    case "doubleClick":
      return { ...before, hover: step.target };
    default:
      return before;
  }
}

/** Duration in ms a step occupies. */
export function stepDuration(step: TutorialStep): number {
  switch (step.type) {
    case "showPanel":
    case "set":
      return 0;
    case "click":
      return step.duration ?? 380;
    case "doubleClick":
      return step.duration ?? 620;
    case "highlight":
      return step.duration ?? 0;
    default:
      return step.duration;
  }
}
