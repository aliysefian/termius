import type { Patch, TutorialStep } from "@/engine/types";

type Extra = { chapter?: string; note?: string };

/**
 * Step builders so a tutorial reads as a script. `makeHelpers(pace)` multiplies every duration, so one number
 * retimes a whole tutorial: a human pace (1.5) for the long walkthroughs, a brisk one (1) for the home page loop.
 */
export function makeHelpers(pace: number) {
  const ms = (n: number) => Math.round(n * pace);
  const type = (target: string, text: string, extra: Extra & { cps?: number; submit?: { terminal: string; prompt: string } } = {}): TutorialStep => {
    const { cps = 14, ...rest } = extra;
    return { type: "type", target, text, duration: ms(Math.max(350, Math.round((text.length / cps) * 1000))), ...rest };
  };
  return {
    type,
    move: (target: string, d = 850, extra: Extra = {}): TutorialStep => ({ type: "moveCursor", target, duration: ms(d), ...extra }),
    click: (target: string, effect?: Patch, extra: Extra & { duration?: number } = {}): TutorialStep => {
      const { duration, ...rest } = extra;
      return { type: "click", target, effect, duration: ms(duration ?? 380), ...rest };
    },
    dblclick: (target: string, effect?: Patch, extra: Extra = {}): TutorialStep => ({ type: "doubleClick", target, effect, duration: ms(620), ...extra }),
    wait: (d: number, extra: Extra = {}): TutorialStep => ({ type: "wait", duration: ms(d), ...extra }),
    set: (patch: Patch, extra: Extra = {}): TutorialStep => ({ type: "set", patch, ...extra }),
    toast: (text: string, d = 1800, kind: "success" | "info" | "error" = "success"): TutorialStep => ({ type: "toast", text, kind, duration: ms(d) }),
    highlight: (target: string | null, d = 900): TutorialStep => ({ type: "highlight", target, duration: ms(d) }),
    progress: (key: string, from: number, to: number, d: number): TutorialStep => ({ type: "progress", key, from, to, duration: ms(d) }),
    out: (terminal: string, lines: string[], d = 500): TutorialStep => ({ type: "terminalOutput", terminal, lines, duration: ms(d) }),
    /** `type` is ~70 ms a character at 1x with extra pauses on spaces and punctuation; `run` types a command and presses Enter. */
    run: (tabId: string, prompt: string, command: string, extra: Extra = {}, inputId = `term-input-${tabId}`, termId = tabId): TutorialStep =>
      type(inputId, command, { submit: { terminal: termId, prompt }, ...extra }),
  };
}

const h = makeHelpers(1.5);
export const { move, click, dblclick, wait, set, toast, highlight, progress, out, type, run } = h;
