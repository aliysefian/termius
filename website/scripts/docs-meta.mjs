// Reads src/content/docs/*.mdx once, for the build (virtual module, sitemap, per-route HTML, search index).
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import GithubSlugger from "github-slugger";

const here = dirname(fileURLToPath(import.meta.url));
export const DOCS_DIR = join(here, "..", "src", "content", "docs");
export const SECTION_ORDER = ["Start", "Connect", "Work", "Secure", "Help"];

function parseFrontmatter(src) {
  const m = /^---\n([\s\S]*?)\n---\n?/.exec(src);
  const meta = {};
  if (!m) return { meta, body: src };
  for (const line of m[1].split("\n")) {
    const kv = /^(\w+):\s*(.*?)\s*(?:#.*)?$/.exec(line);
    if (!kv) continue;
    let v = kv[2];
    if (/^".*"$/.test(v)) v = v.slice(1, -1);
    meta[kv[1]] = /^\d+$/.test(v) ? Number(v) : v;
  }
  return { meta, body: src.slice(m[0].length) };
}

/** Plain text of an MDX body: enough for search and the prerendered HTML, not a renderer. */
function toText(body) {
  return body
    .replace(/```[\s\S]*?```/g, (b) => b.replace(/```\w*/g, " "))
    .replace(/<\/?Callout[^>]*>/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_`>|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function loadDocsMeta() {
  const docs = readdirSync(DOCS_DIR)
    .filter((f) => f.endsWith(".mdx"))
    .map((f) => {
      const src = readFileSync(join(DOCS_DIR, f), "utf8");
      const { meta, body } = parseFrontmatter(src);
      const slugger = new GithubSlugger();
      const headings = [];
      let inFence = false;
      for (const line of body.split("\n")) {
        if (line.startsWith("```")) inFence = !inFence;
        if (inFence) continue;
        const h = /^(#{2,3})\s+(.+?)\s*$/.exec(line);
        if (h) {
          const text = h[2].replace(/[`*_]/g, "");
          headings.push({ depth: h[1].length, text, id: slugger.slug(text) });
        }
      }
      return {
        slug: f.replace(/\.mdx$/, ""),
        title: String(meta.title ?? f),
        description: String(meta.description ?? ""),
        section: String(meta.section ?? "Start"),
        order: Number(meta.order ?? 99),
        headings,
        text: toText(body),
      };
    });
  docs.sort((a, b) => SECTION_ORDER.indexOf(a.section) - SECTION_ORDER.indexOf(b.section) || a.order - b.order);
  return docs;
}
