// Terminal colour schemes. Each maps directly onto xterm.js ITheme.
import type { ITheme } from "@xterm/xterm";

export interface TerminalTheme {
  id: string;
  name: string;
  theme: ITheme;
}

export const themes: TerminalTheme[] = [
  {
    id: "sshvault",
    name: "SSHVault",
    theme: {
      background: "#18191e", foreground: "#e6e6ea", cursor: "#7b61ff", cursorAccent: "#18191e",
      selectionBackground: "rgba(123, 97, 255, 0.35)",
      black: "#20222b", red: "#ff5c5c", green: "#3ddc84", yellow: "#ffb020",
      blue: "#5b8def", magenta: "#b678ff", cyan: "#38bdf8", white: "#e6e6ea",
      brightBlack: "#8b8d98", brightRed: "#ff7b7b", brightGreen: "#5ff29c", brightYellow: "#ffc855",
      brightBlue: "#7fa8ff", brightMagenta: "#cf9dff", brightCyan: "#6ad4ff", brightWhite: "#ffffff",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    theme: {
      background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", cursorAccent: "#282a36",
      selectionBackground: "rgba(68, 71, 90, 0.8)",
      black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c",
      blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
      brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94", brightYellow: "#ffffa5",
      brightBlue: "#d6acff", brightMagenta: "#ff92df", brightCyan: "#a4ffff", brightWhite: "#ffffff",
    },
  },
  {
    id: "nord",
    name: "Nord",
    theme: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", cursorAccent: "#2e3440",
      selectionBackground: "rgba(67, 76, 94, 0.8)",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
      blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c", brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1", brightMagenta: "#b48ead", brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    theme: {
      background: "#002b36", foreground: "#839496", cursor: "#93a1a1", cursorAccent: "#002b36",
      selectionBackground: "rgba(7, 54, 66, 0.9)",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
      blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#586e75", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83",
      brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
    },
  },
  {
    id: "gruvbox-dark",
    name: "Gruvbox Dark",
    theme: {
      background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", cursorAccent: "#282828",
      selectionBackground: "rgba(80, 73, 69, 0.8)",
      black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921",
      blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984",
      brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26", brightYellow: "#fabd2f",
      brightBlue: "#83a598", brightMagenta: "#d3869b", brightCyan: "#8ec07c", brightWhite: "#ebdbb2",
    },
  },
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    theme: {
      background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", cursorAccent: "#1a1b26",
      selectionBackground: "rgba(51, 70, 124, 0.7)",
      black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68",
      blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6",
      brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a", brightYellow: "#e0af68",
      brightBlue: "#7aa2f7", brightMagenta: "#bb9af7", brightCyan: "#7dcfff", brightWhite: "#c0caf5",
    },
  },
  {
    id: "one-dark",
    name: "One Dark",
    theme: {
      background: "#282c34", foreground: "#abb2bf", cursor: "#528bff", cursorAccent: "#282c34",
      selectionBackground: "rgba(62, 68, 81, 0.9)",
      black: "#282c34", red: "#e06c75", green: "#98c379", yellow: "#e5c07b",
      blue: "#61afef", magenta: "#c678dd", cyan: "#56b6c2", white: "#abb2bf",
      brightBlack: "#5c6370", brightRed: "#e06c75", brightGreen: "#98c379", brightYellow: "#e5c07b",
      brightBlue: "#61afef", brightMagenta: "#c678dd", brightCyan: "#56b6c2", brightWhite: "#ffffff",
    },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    theme: {
      background: "#fdf6e3", foreground: "#586e75", cursor: "#586e75", cursorAccent: "#fdf6e3",
      selectionBackground: "rgba(147, 161, 161, 0.35)",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
      blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83",
      brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
    },
  },
  {
    id: "github-light",
    name: "GitHub Light",
    theme: {
      background: "#ffffff", foreground: "#1f2328", cursor: "#0969da", cursorAccent: "#ffffff",
      selectionBackground: "rgba(9, 105, 218, 0.2)",
      black: "#24292f", red: "#cf222e", green: "#116329", yellow: "#4d2d00",
      blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
      brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#1a7f37", brightYellow: "#633c01",
      brightBlue: "#218bff", brightMagenta: "#a475f9", brightCyan: "#3192aa", brightWhite: "#8c959f",
    },
  },
];

export function themeById(id: string): TerminalTheme {
  return themes.find((t) => t.id === id) ?? themes[0];
}
