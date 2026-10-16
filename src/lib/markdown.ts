// A deliberately small Markdown subset: paragraphs, headings, bullet and numbered
// lists, block quotes, rules, tables, **bold**, *italic*, ~~strike~~, `code`,
// fenced code blocks and links. Everything is HTML-escaped first, so text
// from a note or a remote file can never inject markup. Images are not
// fetched: they show as their link text.

function esc(s: string) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function inline(s: string) {
  return esc(s)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/~~([^~]+)~~/g, "<del>$1</del>")
    .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>")
    .replace(/!?\[([^\]]*)\]\((https?:\/\/[^)\s]+)\)/g, (m, text, url) => (m.startsWith("!") ? text || url : `<a href="${url}" target="_blank" rel="noreferrer">${text}</a>`))
    .replace(/(^|\s)(https?:\/\/[^\s<]+)/g, '$1<a href="$2" target="_blank" rel="noreferrer">$2</a>');
}

const BULLET = /^\s*[-*+]\s+/;
const NUMBERED = /^\s*\d{1,9}[.)]\s+/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s{0,3}>/;
const TABLE_SEPARATOR = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

function cells(line: string): string[] {
  const t = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return t.split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));
}

const isTableStart = (lines: string[], i: number) => lines[i].includes("|") && i + 1 < lines.length && lines[i + 1].includes("-") && TABLE_SEPARATOR.test(lines[i + 1]) && cells(lines[i]).length === cells(lines[i + 1]).length;

/** A line that ends a paragraph because another block starts there. */
const startsBlock = (lines: string[], i: number) => /^(#{1,6}\s|```)/.test(lines[i]) || BULLET.test(lines[i]) || NUMBERED.test(lines[i]) || QUOTE.test(lines[i]) || RULE.test(lines[i]) || isTableStart(lines, i);

export interface MarkdownOptions {
  /** `compact` (the default) shows `#` as a small heading, to sit inside a note; `full` keeps the levels. */
  headings?: "compact" | "full";
}

export function renderMarkdown(src: string, opts: MarkdownOptions = {}): string {
  const shift = opts.headings === "full" ? 0 : 2;
  const out: string[] = [];
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      out.push(`<pre><code>${esc(buf.join("\n"))}</code></pre>`);
      continue;
    }
    const h = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (h) {
      const level = Math.min(6, h[1].length + shift);
      out.push(`<h${level}>${inline(h[2])}</h${level}>`);
      i++;
      continue;
    }
    if (RULE.test(line)) {
      out.push("<hr>");
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) buf.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
      out.push(`<blockquote>${renderMarkdown(buf.join("\n"), opts)}</blockquote>`);
      continue;
    }
    if (isTableStart(lines, i)) {
      const head = cells(lines[i]);
      const align = cells(lines[i + 1]).map((c) => (c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : ""));
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim() && lines[i].includes("|")) rows.push(cells(lines[i++]));
      const cell = (tag: string, c: string, n: number) => `<${tag}${align[n] ? ` style="text-align:${align[n]}"` : ""}>${inline(c)}</${tag}>`;
      const body = rows.map((r) => `<tr>${head.map((_, n) => cell("td", r[n] ?? "", n)).join("")}</tr>`).join("");
      out.push(`<table><thead><tr>${head.map((c, n) => cell("th", c, n)).join("")}</tr></thead><tbody>${body}</tbody></table>`);
      continue;
    }
    if (BULLET.test(line) || NUMBERED.test(line)) {
      const ordered = NUMBERED.test(line);
      const kind = ordered ? NUMBERED : BULLET;
      const items: string[] = [];
      while (i < lines.length && kind.test(lines[i])) items.push(lines[i++].replace(kind, ""));
      const tag = ordered ? "ol" : "ul";
      out.push(`<${tag}>${items.map((t) => `<li>${inline(t)}</li>`).join("")}</${tag}>`);
      continue;
    }
    if (!line.trim()) {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && (para.length === 0 || !startsBlock(lines, i))) para.push(lines[i++]);
    out.push(`<p>${inline(para.join(" "))}</p>`);
  }
  return out.join("");
}

/** Whether a file name is Markdown, so a preview can offer to render it. */
export const isMarkdownName = (name: string) => /\.(md|markdown|mdown|mkd)$/i.test(name);
