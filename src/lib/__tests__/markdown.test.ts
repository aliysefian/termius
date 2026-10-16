import { describe, expect, it } from "vitest";
import { isMarkdownName, renderMarkdown } from "../markdown";

describe("renderMarkdown", () => {
  it("renders the subset and escapes html", () => {
    const html = renderMarkdown("# Runbook\n\nRestart with `systemctl restart app` **carefully**.\n\n- step one\n- see https://wiki.local/x\n\n```\nrm -rf <danger>\n```\n<script>alert(1)</script>");
    expect(html).toContain("<h3>Runbook</h3>");
    expect(html).toContain("<code>systemctl restart app</code> <strong>carefully</strong>");
    expect(html).toContain("<ul><li>step one</li><li>see <a href=\"https://wiki.local/x\"");
    expect(html).toContain("<pre><code>rm -rf &lt;danger&gt;</code></pre>");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });

  it("keeps notes headings small by default and the real levels on request", () => {
    expect(renderMarkdown("# A")).toBe("<h3>A</h3>");
    expect(renderMarkdown("###### F")).toBe("<h6>F</h6>");
    expect(renderMarkdown("# A\n\n## B\n\n#### D", { headings: "full" })).toBe("<h1>A</h1><h2>B</h2><h4>D</h4>");
  });

  it("renders numbered lists, quotes, rules and strikethrough", () => {
    const html = renderMarkdown("1. one\n2. two\n10) ten\n\n> quoted **bold**\n> more\n\n---\n\n~~gone~~ and * * *", { headings: "full" });
    expect(html).toContain("<ol><li>one</li><li>two</li><li>ten</li></ol>");
    expect(html).toContain("<blockquote><p>quoted <strong>bold</strong> more</p></blockquote>");
    expect(html).toContain("<hr>");
    expect(html).toContain("<del>gone</del>");
    expect(renderMarkdown("* * *")).toBe("<hr>");
  });

  it("renders tables with alignment, and escapes what is in them", () => {
    const html = renderMarkdown("| Name | Size |\n|:--|--:|\n| <b>a</b> | 1 |\n| b |", { headings: "full" });
    expect(html).toContain('<th>Name</th><th style="text-align:right">Size</th>');
    expect(html).toContain("<td>&lt;b&gt;a&lt;/b&gt;</td><td style=\"text-align:right\">1</td>");
    expect(html).toContain("<td>b</td><td style=\"text-align:right\"></td>");
    // A pipe in prose is not a table.
    expect(renderMarkdown("a | b\nc")).toBe("<p>a | b c</p>");
  });

  it("never makes an image or a script link", () => {
    const html = renderMarkdown("![logo](https://x.test/a.png) [bad](javascript:alert(1)) [ok](https://x.test)");
    expect(html).not.toContain("<img");
    expect(html).not.toContain('href="javascript');
    expect(html).toContain('<a href="https://x.test" target="_blank" rel="noreferrer">ok</a>');
  });

  it("does not hang on odd input", () => {
    expect(() => renderMarkdown("|\n|\n| a\n-\n> \n>\n1.\n#\n```")).not.toThrow();
  });

  it("knows Markdown file names", () => {
    for (const n of ["README.md", "notes.MARKDOWN", "a.mdown"]) expect(isMarkdownName(n)).toBe(true);
    for (const n of ["md", "a.mdx.txt", "readme"]) expect(isMarkdownName(n)).toBe(false);
  });
});
