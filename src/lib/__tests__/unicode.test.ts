import { Terminal } from "@xterm/headless";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { describe, expect, it } from "vitest";

const write = (t: Terminal, s: string) => new Promise<void>((r) => t.write(s, r));
const fresh = (unicode11: boolean) => {
  const t = new Terminal({ cols: 40, rows: 5, allowProposedApi: true });
  if (unicode11) {
    t.loadAddon(new Unicode11Addon());
    t.unicode.activeVersion = "11";
  }
  return t;
};

describe("wide characters", () => {
  it("emoji take two cells with Unicode 11 widths, and one without (which misaligns the line after them)", async () => {
    const old = fresh(false);
    await write(old, "a😀b");
    const modern = fresh(true);
    await write(modern, "a😀b");
    expect(old.buffer.active.cursorX).toBe(3);
    expect(modern.buffer.active.cursorX).toBe(4);
  });

  it("CJK stays two cells either way, and combining marks none", async () => {
    for (const on of [false, true]) {
      const t = fresh(on);
      await write(t, "日本語");
      expect(t.buffer.active.cursorX, `unicode11=${on}`).toBe(6);
      await write(t, "\ré");
      expect(t.buffer.active.cursorX, `combining, unicode11=${on}`).toBe(1);
    }
  });

  it("newer emoji and flags are two cells too", async () => {
    const t = fresh(true);
    await write(t, "🤖🚀");
    expect(t.buffer.active.cursorX).toBe(4);
  });
});
