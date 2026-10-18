// What the overwrite dialog decides from: how an incoming file compares with the one already there, whether a
// text comparison is worth offering, what a backup is called, and how the person's answers become transfers.

import { fileKind } from "$lib/filekind";
import { languageFor } from "$lib/editorlang";

/** One side of a clash: the incoming file, or the one already in the destination folder. */
export interface SideInfo {
  name: string;
  path: string;
  size: number;
  /** Unix seconds. */
  modified: number | null;
  isDir: boolean;
  /** SHA-256, once computed. */
  sha256?: string | null;
}

export type Decision = "overwrite" | "backup" | "rename" | "skip";

/** The biggest file offered a side-by-side comparison. */
export const DIFF_MAX_BYTES = 5 * 1024 * 1024;

export interface Comparison {
  /** `true` when the files are the same, `false` when they differ, `null` when it can't be told yet. */
  identical: boolean | null;
  /** Which has the later modification time. */
  newer: "source" | "destination" | "same" | null;
  /** Source size minus destination size. */
  sizeDelta: number;
}

export function compareSides(src: SideInfo, dst: SideInfo): Comparison {
  let identical: boolean | null = null;
  if (src.sha256 && dst.sha256) identical = src.sha256 === dst.sha256;
  else if (src.size !== dst.size) identical = false;
  const newer = src.modified === null || dst.modified === null ? null : src.modified > dst.modified ? "source" : src.modified < dst.modified ? "destination" : "same";
  return { identical, newer, sizeDelta: src.size - dst.size };
}

/** Whether a name is one a person would open as text: code, settings, notes, or a plain name with no extension. */
export function isTextName(name: string): boolean {
  const kind = fileKind(name, false);
  if (kind === "code" || kind === "config" || kind === "text") return true;
  if (kind === "key" || kind === "image" || kind === "archive") return false;
  if (languageFor(name) !== "plain") return true;
  return !/\.[^./\\]+$/.test(name);
}

/** A comparison is offered for two files that are text-like and small enough to hold in memory. */
export function canDiff(src: SideInfo, dst: SideInfo): boolean {
  return !src.isDir && !dst.isDir && src.size <= DIFF_MAX_BYTES && dst.size <= DIFF_MAX_BYTES && isTextName(src.name);
}

/** `name.bak`, or `name.bak.1`, `name.bak.2`… when that is taken too. */
export function backupName(taken: Set<string>, name: string): string {
  if (!taken.has(`${name}.bak`)) return `${name}.bak`;
  for (let n = 1; n < 10_000; n++) if (!taken.has(`${name}.bak.${n}`)) return `${name}.bak.${n}`;
  return `${name}.bak.${Date.now()}`;
}

export interface Plan {
  /** Copied over what is there. */
  overwrite: string[];
  /** Renamed to a backup first, then copied. */
  backup: string[];
  /** Copied beside what is there under a new name. */
  rename: string[];
  skipped: string[];
}

/**
 * Turn the answers into groups of sources. A name with no clash is copied as it is. `base` says which name a source
 * path has.
 */
export function planFromDecisions(sources: string[], clashes: Set<string>, decisions: Map<string, Decision>, base: (p: string) => string): Plan {
  const plan: Plan = { overwrite: [], backup: [], rename: [], skipped: [] };
  for (const s of sources) {
    const name = base(s);
    if (!clashes.has(name)) {
      plan.overwrite.push(s);
      continue;
    }
    const d = decisions.get(name) ?? "skip";
    if (d === "overwrite") plan.overwrite.push(s);
    else if (d === "backup") plan.backup.push(s);
    else if (d === "rename") plan.rename.push(s);
    else plan.skipped.push(s);
  }
  return plan;
}

/** "1.2 KB larger", "340 B smaller", "same size". */
export function sizeDeltaText(delta: number, format: (n: number) => string): string {
  if (delta === 0) return "same size";
  return `${format(Math.abs(delta))} ${delta > 0 ? "larger" : "smaller"}`;
}
