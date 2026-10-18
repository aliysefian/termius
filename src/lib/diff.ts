// A line diff for the overwrite dialog: what would change if the incoming file replaced the one that is there.
// Myers' algorithm on the lines between the common start and end; a pair of files that differ in more than MAX_D
// places is reported as "too different" instead of being diffed, so a huge or unrelated file can't freeze the window.

export type DiffKind = "same" | "add" | "del";

export interface DiffLine {
  kind: DiffKind;
  text: string;
  /** 1-based line numbers in the old and new text (absent on the side that doesn't have the line). */
  oldNo?: number;
  newNo?: number;
}

export interface DiffResult {
  lines: DiffLine[];
  added: number;
  removed: number;
  /** More differences than are worth diffing: `lines` is then the old text removed and the new text added. */
  tooDifferent: boolean;
}

/** Most edit-script length (additions plus removals) searched for. */
export const MAX_D = 2000;

const split = (s: string) => (s === "" ? [] : s.replace(/\r\n/g, "\n").split("\n"));

/** The edit script between `a` and `b`: for each step, "same", "del" (from a) or "add" (from b). */
function myers(a: string[], b: string[]): DiffKind[] | null {
  const n = a.length;
  const m = b.length;
  const max = Math.min(n + m, MAX_D);
  const size = 2 * max + 1;
  let v = new Int32Array(size);
  const trace: Int32Array[] = [];
  for (let d = 0; d <= max; d++) {
    trace.push(v.slice());
    const next = v.slice();
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      if (k === -d || (k !== d && v[max + k - 1] < v[max + k + 1])) x = v[max + k + 1];
      else x = v[max + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      next[max + k] = x;
      if (x >= n && y >= m) {
        trace.push(next);
        return backtrack(trace, a, b, max, d);
      }
    }
    v = next;
  }
  return null;
}

function backtrack(trace: Int32Array[], a: string[], b: string[], max: number, dEnd: number): DiffKind[] {
  const out: DiffKind[] = [];
  let x = a.length;
  let y = b.length;
  for (let d = dEnd; d > 0; d--) {
    const v = trace[d];
    const k = x - y;
    const prevK = k === -d || (k !== d && v[max + k - 1] < v[max + k + 1]) ? k + 1 : k - 1;
    const prevX = v[max + prevK];
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      out.push("same");
      x--;
      y--;
    }
    out.push(x === prevX ? "add" : "del");
    x = prevX;
    y = prevY;
  }
  while (x > 0 && y > 0) {
    out.push("same");
    x--;
    y--;
  }
  return out.reverse();
}

export function diffLines(oldText: string, newText: string): DiffResult {
  const a = split(oldText);
  const b = split(newText);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);
  const script = midA.length === 0 && midB.length === 0 ? [] : myers(midA, midB);
  const lines: DiffLine[] = [];
  let oldNo = 1;
  let newNo = 1;
  const same = (text: string) => lines.push({ kind: "same", text, oldNo: oldNo++, newNo: newNo++ });
  for (let i = 0; i < start; i++) same(a[i]);
  let tooDifferent = false;
  if (script === null) {
    tooDifferent = true;
    for (const t of midA) lines.push({ kind: "del", text: t, oldNo: oldNo++ });
    for (const t of midB) lines.push({ kind: "add", text: t, newNo: newNo++ });
  } else {
    let i = 0;
    let j = 0;
    for (const step of script) {
      if (step === "same") {
        same(midA[i]);
        i++;
        j++;
      } else if (step === "del") lines.push({ kind: "del", text: midA[i++], oldNo: oldNo++ });
      else lines.push({ kind: "add", text: midB[j++], newNo: newNo++ });
    }
  }
  for (let i = endA; i < a.length; i++) same(a[i]);
  const added = lines.filter((l) => l.kind === "add").length;
  const removed = lines.filter((l) => l.kind === "del").length;
  return { lines, added, removed, tooDifferent };
}

/** A stretch of the diff: changed lines with a little unchanged text around them, or a fold of the rest. */
export type Hunk = { type: "lines"; lines: DiffLine[] } | { type: "fold"; count: number };

/** Keep `context` unchanged lines around each change and fold the long unchanged runs between them. */
export function hunks(lines: DiffLine[], context = 3): Hunk[] {
  const keep = new Array<boolean>(lines.length).fill(false);
  lines.forEach((l, i) => {
    if (l.kind === "same") return;
    for (let k = Math.max(0, i - context); k <= Math.min(lines.length - 1, i + context); k++) keep[k] = true;
  });
  const out: Hunk[] = [];
  let i = 0;
  while (i < lines.length) {
    const start = i;
    if (keep[i]) {
      while (i < lines.length && keep[i]) i++;
      out.push({ type: "lines", lines: lines.slice(start, i) });
    } else {
      while (i < lines.length && !keep[i]) i++;
      out.push({ type: "fold", count: i - start });
    }
  }
  return out;
}

export interface SplitRow {
  left?: DiffLine;
  right?: DiffLine;
}

/** Side by side: unchanged lines on both sides, a run of removals beside the run of additions that replaced it. */
export function splitRows(lines: DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.kind === "same") {
      rows.push({ left: l, right: l });
      i++;
      continue;
    }
    const dels: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i].kind === "del") dels.push(lines[i++]);
    while (i < lines.length && lines[i].kind === "add") adds.push(lines[i++]);
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push({ left: dels[k], right: adds[k] });
  }
  return rows;
}
