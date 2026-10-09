// @ts-nocheck: reads the component sources with Node's fs, which the app's tsconfig has no types for.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// The theme names its page background colour `base` (--color-base), so in this app `text-base` is not Tailwind's
// 16px font size: it paints text in the background colour. The Runbooks page title was invisible because of it.
// Use `text-[1rem]` for the size.

const ROOT = join(__dirname, "..", "..");

function svelteFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "__tests__" ? [] : svelteFiles(p);
    return e.name.endsWith(".svelte") ? [p] : [];
  });
}

describe("class names", () => {
  it("never use text-base, which here is the background colour", () => {
    const hits: string[] = [];
    for (const file of svelteFiles(ROOT)) {
      readFileSync(file, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (/(^|[\s"'`{])text-base(?![\w-])/.test(line)) hits.push(`${file.slice(ROOT.length + 1)}:${i + 1}`);
        });
    }
    expect(hits).toEqual([]);
  });

  it("finds the components it checks", () => {
    expect(svelteFiles(ROOT).some((f) => f.endsWith("RunbooksView.svelte"))).toBe(true);
  });
});
