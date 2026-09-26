import { describe, expect, it } from "vitest";
import { fromIterm, fromVsCode, fromWindowsTerminal, importTheme, mix } from "../themeimport";

describe("theme import", () => {
  it("reads VS Code themes", () => {
    const t = fromVsCode(JSON.stringify({ name: "My Dark", colors: { "terminal.background": "#000000", "terminal.ansiBlack": "#111111", "terminal.ansiBrightRed": "#ff0000" } }));
    expect(t?.id).toBe("custom-my-dark");
    expect(t?.theme.black).toBe("#111111");
    expect(t?.theme.brightRed).toBe("#ff0000");
    expect(fromVsCode("{}")).toBeNull();
  });

  it("reads Windows Terminal schemes, including purple", () => {
    const t = fromWindowsTerminal(JSON.stringify({ name: "Campbell", black: "#0C0C0C", purple: "#881798", brightBlack: "#767676", brightPurple: "#B4009E", background: "#0C0C0C" }));
    expect(t?.theme.magenta).toBe("#881798");
    expect(t?.theme.brightMagenta).toBe("#B4009E");
  });

  it("reads iTerm2 plists", () => {
    const plist = `<?xml version="1.0"?><plist version="1.0"><dict>
      <key>Ansi 0 Color</key><dict><key>Blue Component</key><real>0.0</real><key>Green Component</key><real>0.5</real><key>Red Component</key><real>1</real></dict>
      <key>Background Color</key><dict><key>Blue Component</key><real>0.1</real><key>Green Component</key><real>0.1</real><key>Red Component</key><real>0.1</real></dict>
    </dict></plist>`;
    const t = fromIterm(plist, "Solarized.itermcolors");
    expect(t?.name).toBe("Solarized");
    expect(t?.theme.black).toBe("#ff8000");
    expect(t?.theme.background).toBe("#1a1a1a");
    expect(importTheme("not a theme")).toBeNull();
  });

  it("mixes colours", () => {
    expect(mix("#000000", "#ff0000", 0.5)).toBe("#800000");
    expect(mix("#fff", "#000", 1)).toBe("#000000");
    expect(mix("rgba(1,2,3,0.5)", "#000", 0.5)).toBe("rgba(1,2,3,0.5)");
  });
});
