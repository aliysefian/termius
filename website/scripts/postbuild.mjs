// Runs after `vite build`. A single-page app on GitHub Pages needs three things the bundler does not make:
//   1. a real HTML file per route, so deep links and crawlers get 200 + the right title (and Pagefind has text to index),
//   2. a 404.html that boots the app for any other path,
//   3. sitemap.xml and robots.txt.
// Node 22.6+ strips TypeScript types, which lets this read the tutorial registry without a second copy of it.
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadDocsMeta } from "./docs-meta.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(root, "dist");
const SITE = (process.env.SITE_URL ?? "https://aliysefian.github.io/termius").replace(/\/$/, "");
const BASE = (process.env.VITE_BASE ?? "/termius/").replace(/\/?$/, "/");

const { tutorials } = await import("../src/tutorials/registry.ts");
const docs = loadDocsMeta();
const shell = readFileSync(join(dist, "index.html"), "utf8");

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const routes = [
  { path: "/", title: "SSHVault: an SSH client with an encrypted vault you sync yourself", description: "Open-source SSH client for Linux, Windows and macOS. Hosts, keys and credentials are encrypted on your computer and synced through a folder you already use.", text: "" },
  { path: "/features", title: "Features: SSHVault", description: "Everything SSHVault does today, each with a short animated demonstration, a documentation link and a tutorial.", text: "" },
  { path: "/tutorials", title: "Tutorials: SSHVault", description: "Animated, step-by-step SSHVault walkthroughs.", text: tutorials.map((t) => `${t.title}. ${t.description}`).join(" ") },
  { path: "/downloads", title: "Download SSHVault", description: "SSHVault installers for Windows, macOS and Linux (.deb, .rpm, AppImage), straight from GitHub Releases.", text: "" },
  { path: "/faq", title: "FAQ and troubleshooting: SSHVault", description: "Answers about SSHVault's vault, sync, installers, host keys, the SSH agent and common problems.", text: "" },
  { path: "/docs", title: "Documentation: SSHVault", description: "Guides for installing and using SSHVault.", text: docs.map((d) => `${d.title}. ${d.description}`).join(" ") },
  ...docs.map((d) => ({ path: `/docs/${d.slug}`, title: `${d.title}: SSHVault docs`, description: d.description, text: d.text, heading: d.title })),
  ...tutorials.map((t) => ({
    path: `/tutorials/${t.id}`,
    title: `${t.title}: SSHVault tutorial`,
    description: t.description,
    text: `${t.description} ${t.steps.map((s) => `${s.title}. ${s.body}`).join(" ")}`,
    heading: t.title,
  })),
];

function page(r) {
  let html = shell;
  html = html.replace(/<title>[\s\S]*?<\/title>/, `<title>${esc(r.title)}</title>`);
  html = html.replace(/<meta name="description"[^>]*>/, `<meta name="description" content="${esc(r.description)}" />`);
  html = html.replace(/<meta property="og:title"[^>]*>/, `<meta property="og:title" content="${esc(r.title)}" />`);
  html = html.replace(/<meta property="og:description"[^>]*>/, `<meta property="og:description" content="${esc(r.description)}" />`);
  const url = `${SITE}${r.path === "/" ? "/" : r.path}`;
  html = html.replace("</head>", `    <link rel="canonical" href="${url}" />\n    <meta property="og:url" content="${url}" />\n  </head>`);
  html = html.replace(/<meta property="og:image"[^>]*>/, `<meta property="og:image" content="${SITE}/og.png" />`);
  // Text for crawlers and for Pagefind. `hidden` keeps it out of sight; the app replaces nothing here.
  const body = `<div hidden data-static-seo data-pagefind-body>${r.heading ? `<h1 data-pagefind-meta="title">${esc(r.heading)}</h1>` : `<h1 data-pagefind-meta="title">${esc(r.title)}</h1>`}<p>${esc(r.description)}</p>${r.text ? `<p>${esc(r.text)}</p>` : ""}</div>`;
  html = html.replace("</body>", `    ${body}\n  </body>`);
  return html;
}

for (const r of routes) {
  const out = r.path === "/" ? join(dist, "index.html") : join(dist, r.path, "index.html");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, page(r));
}
// Any other path: the app boots and shows its own not-found page.
writeFileSync(join(dist, "404.html"), shell.replace(/<title>[\s\S]*?<\/title>/, "<title>Page not found: SSHVault</title>"));
writeFileSync(join(dist, ".nojekyll"), "");

const today = new Date().toISOString().slice(0, 10);
const sitemap = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${routes
  .map((r) => `  <url><loc>${SITE}${r.path === "/" ? "/" : r.path}</loc><lastmod>${today}</lastmod></url>`)
  .join("\n")}\n</urlset>\n`;
writeFileSync(join(dist, "sitemap.xml"), sitemap);
writeFileSync(join(dist, "robots.txt"), `User-agent: *\nAllow: /\n\nSitemap: ${SITE}/sitemap.xml\n`);

// A custom domain: put a CNAME file in website/public and Vite copies it; nothing else changes (build with VITE_BASE=/).
if (existsSync(join(root, "public", "CNAME"))) copyFileSync(join(root, "public", "CNAME"), join(dist, "CNAME"));
console.log(`postbuild: ${routes.length} routes, sitemap.xml, robots.txt, 404.html (base ${BASE}, site ${SITE})`);
