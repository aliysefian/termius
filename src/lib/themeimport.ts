// Import terminal colour schemes from other tools. Each parser returns null
// when the text isn't that format, so callers can try them in turn.
import type { ITheme } from "@xterm/xterm";
import type { TerminalTheme } from "./themes";

const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;

function slug(name: string) {
  return `custom-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "theme"}`;
}

/** VS Code colour theme JSON (`colors["terminal.ansiRed"]`…). */
export function fromVsCode(text: string): TerminalTheme | null {
  let j: { name?: string; colors?: Record<string, string> };
  try {
    j = JSON.parse(text);
  } catch {
    return null;
  }
  const c = j?.colors;
  if (!c || typeof c !== "object" || !c["terminal.ansiBlack"]) return null;
  const cap = (s: string) => s[0].toUpperCase() + s.slice(1);
  const theme: ITheme = {
    background: c["terminal.background"] ?? c["editor.background"],
    foreground: c["terminal.foreground"] ?? c["editor.foreground"],
    cursor: c["terminalCursor.foreground"] ?? c["editorCursor.foreground"],
    selectionBackground: c["terminal.selectionBackground"] ?? c["editor.selectionBackground"],
  };
  for (const n of ANSI) {
    (theme as Record<string, string>)[n] = c[`terminal.ansi${cap(n)}`];
    (theme as Record<string, string>)[`bright${cap(n)}`] = c[`terminal.ansiBright${cap(n)}`];
  }
  const name = j.name ?? "VS Code theme";
  return { id: slug(name), name, theme };
}

/** Windows Terminal scheme (`{"name": …, "black": …, "brightBlack": …}`). */
export function fromWindowsTerminal(text: string): TerminalTheme | null {
  let j: Record<string, unknown>;
  try {
    j = JSON.parse(text);
  } catch {
    return null;
  }
  // Accept a whole settings.json too: take its first scheme.
  if (Array.isArray(j?.schemes) && j.schemes.length) j = j.schemes[0] as Record<string, unknown>;
  if (typeof j?.black !== "string" || typeof j?.brightBlack !== "string") return null;
  const s = j as Record<string, string>;
  const theme: ITheme = {
    background: s.background,
    foreground: s.foreground,
    cursor: s.cursorColor,
    selectionBackground: s.selectionBackground,
    black: s.black, red: s.red, green: s.green, yellow: s.yellow,
    blue: s.blue, magenta: s.purple ?? s.magenta, cyan: s.cyan, white: s.white,
    brightBlack: s.brightBlack, brightRed: s.brightRed, brightGreen: s.brightGreen, brightYellow: s.brightYellow,
    brightBlue: s.brightBlue, brightMagenta: s.brightPurple ?? s.brightMagenta, brightCyan: s.brightCyan, brightWhite: s.brightWhite,
  };
  const name = s.name ?? "Windows Terminal scheme";
  return { id: slug(name), name, theme };
}

/** iTerm2 `.itermcolors` (a plist with "Ansi N Color" dictionaries). */
export function fromIterm(text: string, fileName = "iTerm2 theme"): TerminalTheme | null {
  if (!text.includes("<plist") || !text.includes("Ansi 0 Color")) return null;
  const colors: Record<string, string> = {};
  const re = /<key>([^<]+)<\/key>\s*<dict>([\s\S]*?)<\/dict>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const comp = (name: string) => {
      const c = new RegExp(`<key>${name} Component</key>\\s*<real>([\\d.eE+-]+)</real>`).exec(m![2]);
      return c ? Math.round(Math.min(1, Math.max(0, Number(c[1]))) * 255) : 0;
    };
    const hex = (n: number) => n.toString(16).padStart(2, "0");
    colors[m[1]] = `#${hex(comp("Red"))}${hex(comp("Green"))}${hex(comp("Blue"))}`;
  }
  const ansi = (i: number) => colors[`Ansi ${i} Color`];
  const theme: ITheme = {
    background: colors["Background Color"],
    foreground: colors["Foreground Color"],
    cursor: colors["Cursor Color"],
    selectionBackground: colors["Selection Color"],
  };
  ANSI.forEach((n, i) => {
    (theme as Record<string, string>)[n] = ansi(i);
    (theme as Record<string, string>)[`bright${n[0].toUpperCase()}${n.slice(1)}`] = ansi(i + 8);
  });
  const name = fileName.replace(/\.itermcolors$/i, "");
  return { id: slug(name), name, theme };
}

export function importTheme(text: string, fileName?: string): TerminalTheme | null {
  return fromVsCode(text) ?? fromWindowsTerminal(text) ?? fromIterm(text, fileName);
}

/** Mix `hex` toward `toward` by `amount` (0..1). Used for the production tint. */
export function mix(hex: string, toward: string, amount: number): string {
  const p = (h: string) => {
    const s = h.replace("#", "");
    const v = s.length === 3 ? s.split("").map((c) => c + c).join("") : s;
    return [0, 2, 4].map((i) => parseInt(v.slice(i, i + 2), 16));
  };
  if (!/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) return hex;
  const a = p(hex);
  const b = p(toward);
  return `#${a.map((x, i) => Math.round(x + (b[i] - x) * amount).toString(16).padStart(2, "0")).join("")}`;
}
