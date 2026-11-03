// Matching the columns of a CSV file to the columns of a table: by name first (ignoring case, spaces, dashes and
// underscores), and each table column at most once. A column with no match is left out, never guessed.
const norm = (s: string) => s.toLowerCase().replace(/[\s_\-.]+/g, "");

export function autoMapping(fileColumns: string[], tableColumns: string[]): (string | null)[] {
  const used = new Set<string>();
  return fileColumns.map((h) => {
    const n = norm(h);
    if (!n) return null;
    const hit = tableColumns.find((t) => !used.has(t) && norm(t) === n);
    if (hit) used.add(hit);
    return hit ?? null;
  });
}
