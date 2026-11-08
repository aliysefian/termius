// The shape every inspection script prints: sections introduced by a line `@@NAME`. The parsers take what is between.
export function sections(out: string): Map<string, string[]> {
  const map = new Map<string, string[]>();
  let cur: string[] | null = null;
  for (const raw of out.split("\n")) {
    const line = raw.replace(/\r$/, "");
    if (line.startsWith("@@")) {
      cur = [];
      map.set(line.slice(2).trim(), cur);
    } else if (cur && cur.length < 60_000) cur.push(line);
  }
  return map;
}

/** The output ended where the script ends: a run that was cut short has no `@@END`. */
export const complete = (s: Map<string, string[]>) => s.has("END");

export const nonEmpty = (lines: string[] | undefined) => (lines ?? []).filter((l) => l.trim() !== "");

/** `now` from the host's own clock (seconds since 1970), else this computer's. */
export function hostNow(s: Map<string, string[]>): number {
  const n = Number(nonEmpty(s.get("NOW"))[0]);
  return Number.isFinite(n) && n > 1_000_000_000 ? n : Math.floor(Date.now() / 1000);
}

/** A thing the view could not read, and why, in words a person can act on. */
export interface Limit {
  what: string;
  reason: string;
  /** More rights would show it: the view offers to read again with sudo. */
  needsRoot?: boolean;
}
