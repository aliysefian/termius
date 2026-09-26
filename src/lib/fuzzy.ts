// Small fuzzy matcher for the command palette: every query character must
// appear in order; contiguous runs and word starts score higher.
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().trim();
  if (!q) return 0;
  const t = text.toLowerCase();
  const direct = t.indexOf(q);
  if (direct >= 0) return 1000 - direct - (t.length - q.length) * 0.1;
  let score = 0;
  let ti = 0;
  let run = 0;
  for (const ch of q) {
    if (ch === " ") continue;
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    run = found === ti ? run + 1 : 0;
    const wordStart = found === 0 || /[\s\-_./@:]/.test(t[found - 1]);
    score += 10 + run * 5 + (wordStart ? 8 : 0) - (found - ti) * 0.5;
    ti = found + 1;
  }
  return score;
}
