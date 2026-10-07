// "After connecting, wait for this text and send that": for a banner, a menu, a jump box's prompt. Steps run in
// order, each gives up after its own time, and a step may not wait for (or send) a secret: what a host
// stores here is plain text in the vault, and typing a password for someone is exactly what must not be automated.
import { sensitiveReason } from "$lib/completion/history";
import type { ExpectStep } from "$lib/types";

export const MAX_STEPS = 10;
export const MAX_TEXT = 200;
export const DEFAULT_TIMEOUT = 30;
export const MAX_TIMEOUT = 300;
/** How much recent output is searched. */
const WINDOW = 4096;

/** Prompts that ask for something secret. A step waiting for one is refused. */
const SECRET_PROMPT = /pass(word|phrase|code|wd)|\bpin\b|token|secret|\botp\b|2fa|verification code|one[- ]time|credential/i;

/** What is wrong with a list of steps, or null. */
export function problem(steps: ExpectStep[]): string | null {
  if (steps.length > MAX_STEPS) return `At most ${MAX_STEPS} steps.`;
  for (const [i, s] of steps.entries()) {
    const n = i + 1;
    if (!s.wait.trim()) return `Step ${n}: say what to wait for.`;
    if (s.wait.length > MAX_TEXT || s.send.length > MAX_TEXT) return `Step ${n}: keep each text under ${MAX_TEXT} characters.`;
    if (/[\r\n\0]/.test(s.wait) || /[\r\n\0]/.test(s.send)) return `Step ${n}: one line of text, without line breaks (Enter is sent for you).`;
    if (SECRET_PROMPT.test(s.wait)) return `Step ${n}: this waits for a password, code or similar. Nothing here types secrets for you; type it yourself, or use a key.`;
    if (sensitiveReason(s.send)) return `Step ${n}: what is sent looks like a secret. It would be stored as plain text in the vault, so it isn't allowed.`;
    if (s.timeout_secs !== undefined && (!Number.isInteger(s.timeout_secs) || s.timeout_secs < 1 || s.timeout_secs > MAX_TIMEOUT)) return `Step ${n}: wait between 1 and ${MAX_TIMEOUT} seconds.`;
  }
  return null;
}

/** Output without the escape sequences a terminal would act on, so a prompt is found as it is read. */
export function plain(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g, "").replace(/\r/g, "");
}

/** Walks the steps as output arrives. */
export class Expect {
  #i = 0;
  #seen = "";
  #since: number;

  constructor(
    readonly steps: ExpectStep[],
    now = Date.now(),
  ) {
    this.#since = now;
  }

  get done(): boolean {
    return this.#i >= this.steps.length;
  }

  /** The step being waited for. */
  get waiting(): ExpectStep | null {
    return this.steps[this.#i] ?? null;
  }

  /** New output; returns what to type (each already ends with Enter). */
  feed(output: string, now = Date.now()): string[] {
    const sent: string[] = [];
    this.#seen = (this.#seen + plain(output)).slice(-WINDOW);
    while (!this.done && this.#seen.includes(this.steps[this.#i].wait)) {
      sent.push(`${this.steps[this.#i].send}\r`);
      // What matched is used up, so the next step waits for something new.
      this.#seen = this.#seen.slice(this.#seen.indexOf(this.steps[this.#i].wait) + this.steps[this.#i].wait.length);
      this.#i++;
      this.#since = now;
    }
    return sent;
  }

  /** The step that has waited too long, if any; the rest are then dropped. */
  expired(now = Date.now()): ExpectStep | null {
    const s = this.waiting;
    if (!s || now - this.#since < (s.timeout_secs ?? DEFAULT_TIMEOUT) * 1000) return null;
    this.#i = this.steps.length;
    return s;
  }
}
