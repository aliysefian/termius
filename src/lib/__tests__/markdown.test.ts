import { describe, expect, it } from "vitest";
import { renderMarkdown } from "../markdown";

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
});
