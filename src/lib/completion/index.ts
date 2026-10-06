// The entry point the terminal pane uses. Every question goes through a Policy
// first, so with the feature off none of the code behind it is reached.
import { readLine, type InputWatch, type ShellLine, type TermLike } from "./line";
import type { Policy } from "./policy";

export { InputWatch } from "./line";
export type { ShellLine, TermLike } from "./line";
export { OFF, policyFor, type Policy } from "./policy";

/** The typed line, or null when smart completion is off here or the line can't be known. */
export function completionLine(policy: Policy, term: TermLike, watch: InputWatch, typed: string | null): ShellLine | null {
  if (!policy.enabled) return null;
  return readLine(term, watch, typed);
}
