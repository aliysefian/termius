// App-wide keyboard shortcuts. Registered in the capture phase so they win
// over xterm.js, which otherwise swallows keys aimed at the terminal. Most
// default to Ctrl+Shift so plain Ctrl combos (Ctrl+W, Ctrl+T, ...) still reach
// the remote shell, where readline and editors rely on them.
//
// Every action has a default combo the user can change in Settings; the
// overrides live in `settings.prefs.keybindings`.
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";

export interface Action {
  id: string;
  label: string;
  /** e.g. "Ctrl+Shift+P". Empty = unbound. */
  combo: string;
  run: (e: KeyboardEvent) => boolean | void;
}

function inTextField(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  // xterm's hidden textarea counts as the terminal, not a text field.
  if (el.classList?.contains("xterm-helper-textarea")) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
}

const inTerminal = () => document.activeElement?.classList.contains("xterm-helper-textarea") ?? false;

export const ACTIONS: Action[] = [
  { id: "palette", label: "Command palette", combo: "Ctrl+Shift+P", run: () => void (ui.paletteOpen = !ui.paletteOpen) },
  { id: "quick-connect", label: "Quick connect", combo: "Ctrl+Shift+T", run: () => void (ui.modal = { kind: "quick-connect" }) },
  { id: "close-tab", label: "Close tab", combo: "Ctrl+Shift+W", run: () => void (ui.activeTabId && ui.closeTab(ui.activeTabId)) },
  { id: "split-right", label: "Split right", combo: "Ctrl+Shift+D", run: () => ui.splitActive("vertical") },
  { id: "split-down", label: "Split down", combo: "Ctrl+Shift+E", run: () => ui.splitActive("horizontal") },
  { id: "zoom-pane", label: "Maximize or restore the pane", combo: "Ctrl+Shift+Enter", run: () => ui.toggleZoomActive() },
  { id: "lock", label: "Lock the vault", combo: "Ctrl+Shift+L", run: () => void vaultStore.lock() },
  { id: "sync-input", label: "Type into all panes (toggle)", combo: "Ctrl+Shift+B", run: () => void ui.syncRequest++ },
  { id: "sidebar", label: "Hide or show the list panel", combo: "Ctrl+Shift+H", run: () => ui.toggleSidebar() },
  { id: "focus-mode", label: "Focus mode (toggle)", combo: "Ctrl+Shift+U", run: () => void (settings.prefs.focusMode = !settings.prefs.focusMode) },
  { id: "local-terminal", label: "New local terminal", combo: "Ctrl+Shift+`", run: () => ui.openLocal() },
  { id: "shortcuts", label: "Keyboard shortcuts", combo: "Ctrl+Shift+/", run: () => void (ui.modal = ui.modal?.kind === "shortcuts" ? null : { kind: "shortcuts" }) },
  { id: "prev-tab", label: "Previous tab", combo: "Ctrl+Shift+Tab", run: () => ui.cycleTab(-1) },
  { id: "next-tab", label: "Next tab", combo: "Ctrl+Tab", run: () => ui.cycleTab(1) },
  {
    id: "find",
    label: "Find in terminal",
    combo: "Ctrl+Shift+F",
    run: (e) => {
      // Terminals handle this themselves when focused; this covers the
      // case where focus is elsewhere in the window.
      if (ui.activeTab && !inTextField(e.target)) {
        ui.findRequest++;
        return true;
      }
      return false;
    },
  },
  {
    id: "palette-alt",
    label: "Command palette (outside a terminal)",
    combo: "Ctrl+K",
    run: () => {
      // Ctrl+K kills to end of line in shells, so only take it outside terminals.
      if (inTerminal()) return false;
      ui.paletteOpen = !ui.paletteOpen;
      return true;
    },
  },
];

const KEY_NAMES: Record<string, string> = {
  Backquote: "`", Slash: "/", Minus: "-", Equal: "=", Period: ".", Comma: ",", Semicolon: ";", Quote: "'",
  BracketLeft: "[", BracketRight: "]", Backslash: "\\", Space: "Space",
};

/** "Ctrl+Shift+P" for a key event, or null for a bare modifier. */
export function comboOf(e: KeyboardEvent): string | null {
  const code = e.code;
  if (!code || /^(Control|Shift|Alt|Meta)(Left|Right)$/.test(code)) return null;
  let key = KEY_NAMES[code];
  if (!key) {
    const m = /^(Key|Digit|Numpad)(.+)$/.exec(code);
    key = m ? m[2] : code;
  }
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

/** The combo in force for an action, after the user's overrides. */
export function comboFor(a: Action): string {
  const o = settings.prefs.keybindings[a.id];
  return o === undefined ? a.combo : o;
}

export function handleShortcut(e: KeyboardEvent): boolean {
  if (!vaultStore.unlocked) return false;
  const combo = comboOf(e);
  if (!combo) return false;
  const ctrl = e.ctrlKey || e.metaKey;

  for (const a of ACTIONS) {
    if (comboFor(a) === combo) {
      const r = a.run(e);
      return r !== false;
    }
  }

  // Fixed keys: tabs by number, zoom, PageUp/Down.
  if (ctrl && !e.altKey && !e.shiftKey) {
    switch (e.code) {
      case "PageDown":
        ui.cycleTab(1);
        return true;
      case "PageUp":
        ui.cycleTab(-1);
        return true;
      case "Equal":
      case "NumpadAdd":
        settings.zoom(1);
        return true;
      case "Minus":
      case "NumpadSubtract":
        settings.zoom(-1);
        return true;
      case "Digit0":
      case "Numpad0":
        settings.zoom(0);
        return true;
    }
    const digit = /^Digit([1-9])$/.exec(e.code);
    if (digit) {
      ui.selectTab(Number(digit[1]) - 1);
      return true;
    }
  }
  return false;
}
