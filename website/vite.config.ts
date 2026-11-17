import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import mdx from "@mdx-js/rollup";
import remarkGfm from "remark-gfm";
import remarkFrontmatter from "remark-frontmatter";
import remarkMdxFrontmatter from "remark-mdx-frontmatter";
import rehypeSlug from "rehype-slug";
import rehypeAutolinkHeadings from "rehype-autolink-headings";
import rehypeHighlight from "rehype-highlight";
import { fileURLToPath } from "node:url";
// @ts-expect-error plain ESM script shared with the postbuild step
import { loadDocsMeta } from "./scripts/docs-meta.mjs";

/** `virtual:docs-index`: titles, sections and headings of every doc, without shipping their text. */
function docsIndex(): Plugin {
  const id = "virtual:docs-index";
  return {
    name: "docs-index",
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(this: { addWatchFile(f: string): void }, source) {
      if (source !== `\0${id}`) return null;
      const docs = (loadDocsMeta() as { text: string }[]).map(({ text: _text, ...rest }) => rest);
      return `export default ${JSON.stringify(docs)};`;
    },
    handleHotUpdate({ file, server }) {
      if (file.endsWith(".mdx")) {
        const mod = server.moduleGraph.getModuleById(`\0${id}`);
        if (mod) server.moduleGraph.invalidateModule(mod);
      }
    },
  };
}

// GitHub Pages serves the project site under /termius/. A custom domain builds with VITE_BASE=/.
const base = process.env.VITE_BASE ?? "/termius/";

export default defineConfig({
  base,
  plugins: [
    {
      enforce: "pre",
      ...mdx({
        remarkPlugins: [remarkFrontmatter, [remarkMdxFrontmatter, { name: "meta" }], remarkGfm],
        rehypePlugins: [
          rehypeSlug,
          [rehypeAutolinkHeadings, { behavior: "append", properties: { className: ["anchor"], ariaLabel: "Link to this section" }, content: { type: "text", value: "#" } }],
          [rehypeHighlight, { detect: false, ignoreMissing: true }],
        ],
        providerImportSource: "@mdx-js/react",
      }),
    },
    docsIndex(),
    react({ include: /\.(mdx|tsx|ts)$/ }),
    tailwindcss(),
  ],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: { target: "es2022", sourcemap: false, chunkSizeWarningLimit: 600 },
});
