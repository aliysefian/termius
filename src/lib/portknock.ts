// Port-knock sequences: the same spellings and limits the backend accepts (src-tauri/src/portknock.rs), so the host
// form can say what is wrong before anything is saved or sent.

export type KnockProtocol = "tcp" | "udp";
export interface KnockStep {
  port: number;
  protocol: KnockProtocol;
}
export interface PortKnocking {
  enabled: boolean;
  sequence: KnockStep[];
  /** Wait between one knock and the next (10 to 5000 ms). */
  inter_knock_delay_ms: number;
  /** Wait after the last knock before connecting (0 to 10000 ms). */
  post_knock_delay_ms: number;
}

export const INTER_KNOCK_DEFAULT = 150;
export const INTER_KNOCK_MIN = 10;
export const INTER_KNOCK_MAX = 5000;
export const POST_KNOCK_DEFAULT = 500;
export const POST_KNOCK_MAX = 10_000;
export const MAX_STEPS = 16;

export const emptyKnocking = (): PortKnocking => ({ enabled: false, sequence: [], inter_knock_delay_ms: INTER_KNOCK_DEFAULT, post_knock_delay_ms: POST_KNOCK_DEFAULT });

export type Parsed = { ok: true; steps: KnockStep[] } | { ok: false; error: string };

function parseStep(token: string): { step: KnockStep } | { error: string } {
  const at = token.search(/[:/]/);
  const portText = at < 0 ? token : token.slice(0, at);
  const protoText = at < 0 ? null : token.slice(at + 1).trim().toLowerCase();
  if (!/^\d+$/.test(portText.trim())) return { error: `"${token}" isn't a port number` };
  const port = Number(portText.trim());
  if (port < 1 || port > 65535) return { error: `port ${port} is out of range (1 to 65535)` };
  if (protoText !== null && protoText !== "tcp" && protoText !== "udp") return { error: `"${protoText}" isn't a protocol: use tcp or udp` };
  return { step: { port, protocol: protoText === "udp" ? "udp" : "tcp" } };
}

/** `"7000, 8000:udp, 9000"` as steps. Ports default to TCP; `:` or `/` may separate the protocol. */
export function parseSequence(text: string): Parsed {
  const tokens = text.split(/[,;\s]+/).filter(Boolean);
  if (tokens.length === 0) return { ok: false, error: "Enter at least one port, like 7000, 8000:udp, 9000" };
  if (tokens.length > MAX_STEPS) return { ok: false, error: `A knock sequence has at most ${MAX_STEPS} ports` };
  const steps: KnockStep[] = [];
  for (const t of tokens) {
    const r = parseStep(t);
    if ("error" in r) return { ok: false, error: r.error[0].toUpperCase() + r.error.slice(1) };
    steps.push(r.step);
  }
  return { ok: true, steps };
}

/** The steps as `"7000, 8000:udp"`: TCP is the default, so it has no suffix. */
export function formatSequence(steps: KnockStep[]): string {
  return steps.map((s) => (s.protocol === "udp" ? `${s.port}:udp` : String(s.port))).join(", ");
}

/** Why a delay can't be used, or null. */
export function delayProblem(value: unknown, min: number, max: number, what: string): string | null {
  const n = typeof value === "number" ? value : Number(value);
  if (value === "" || value === null || value === undefined || !Number.isInteger(n)) return `${what} must be a whole number of milliseconds`;
  if (n < min || n > max) return `${what} must be ${min} to ${max} ms`;
  return null;
}

/** Everything wrong with a configuration, in the order the form shows it; empty when it can be saved. */
export function knockProblems(sequenceText: string, inter: unknown, post: unknown, enabled: boolean): string[] {
  if (!enabled) return [];
  const problems: string[] = [];
  const seq = parseSequence(sequenceText);
  if (!seq.ok) problems.push(seq.error);
  const a = delayProblem(inter, INTER_KNOCK_MIN, INTER_KNOCK_MAX, "The delay between knocks");
  if (a) problems.push(a);
  const b = delayProblem(post, 0, POST_KNOCK_MAX, "The wait before connecting");
  if (b) problems.push(b);
  return problems;
}
