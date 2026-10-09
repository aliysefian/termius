import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import { redrawPreview } from "../terminalpreview";

function screen(term: Terminal): string[] {
  const b = term.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < b.length; i++) out.push(b.getLine(i)?.translateToString(true) ?? "");
  while (out.length && out[out.length - 1] === "") out.pop();
  return out;
}

describe("Settings terminal preview", () => {
  it("shows the sample once, however often it is redrawn in a row", async () => {
    const term = new Terminal({ cols: 80, rows: 12, allowProposedApi: true });
    // Mount and the first settings effect both draw before the terminal has parsed anything.
    redrawPreview(term);
    redrawPreview(term);
    await new Promise<void>((resolve) => redrawPreview(term, resolve));
    const lines = screen(term);
    expect(lines.filter((l) => l.endsWith("$ ls"))).toHaveLength(1);
    expect(lines[0]).toBe("user@host:~/sshvault$ ls");
    expect(lines.at(-1)?.trimEnd()).toBe("user@host:~/sshvault$");
    expect(lines.some((l) => l.includes("$ user@host"))).toBe(false);
    term.dispose();
  });
});
