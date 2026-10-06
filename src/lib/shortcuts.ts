// App-wide keyboard shortcuts. Registered in the capture phase so they win
// over xterm.js, which otherwise swallows keys aimed at the terminal. Most
// default to Ctrl+Shift so plain Ctrl combos (Ctrl+W, Ctrl+T, ...) still reach
// the remote shell, where readline and editors rely on them.
//
// Every action has a default combo the user can change in Settings; the
// overrides live in `settings.prefs.keybindings`.
import { completionBridge } from "$lib/completion/bridge";
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
  { id: "close-tab", label: "Close tab", combo: "Ctrl+Shift+W", run: () => void (ui.activeTabId && ui.requestCloseTab(ui.activeTabId)) },
  { id: "split-right", label: "Split right", combo: "Ctrl+Shift+D", run: () => ui.splitActive("vertical") },
  { id: "split-down", label: "Split down", combo: "Ctrl+Shift+E", run: () => ui.splitActive("horizontal") },
  { id: "zoom-pane", label: "Maximize or restore the pane", combo: "Ctrl+Shift+Enter", run: () => ui.toggleZoomActive() },
  { id: "lock", label: "Lock the vault", combo: "Ctrl+Shift+L", run: () => void vaultStore.lock() },
  { id: "sync-input", label: "Type into all panes (toggle)", combo: "Ctrl+Shift+B", run: () => void ui.syncRequest++ },
  { id: "sidebar", label: "Hide or show the list panel", combo: "Ctrl+Shift+H", run: () => ui.toggleSidebar() },
  { id: "focus-mode", label: "Focus mode (toggle)", combo: "Ctrl+Shift+U", run: () => void (settings.prefs.focusMode = !settings.prefs.focusMode) },
  { id: "local-terminal", label: "New local terminal", combo: "Ctrl+Shift+`", run: () => ui.openLocal() },
  // Alt+Shift, not plain Alt: bare Alt+Left/Right is readline's back/forward
  // word jump, used constantly in a shell, so that combo stays free.
  { id: "focus-pane-left", label: "Focus the pane to the left", combo: "Alt+Shift+ArrowLeft", run: () => ui.focusPane("left") },
  { id: "focus-pane-right", label: "Focus the pane to the right", combo: "Alt+Shift+ArrowRight", run: () => ui.focusPane("right") },
  { id: "focus-pane-up", label: "Focus the pane above", combo: "Alt+Shift+ArrowUp", run: () => ui.focusPane("up") },
  { id: "focus-pane-down", label: "Focus the pane below", combo: "Alt+Shift+ArrowDown", run: () => ui.focusPane("down") },
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
  // The suggestion after the cursor. Each returns false when there is none showing, so the key is
  // left alone and goes to the shell as usual (Right and End move the cursor, Esc reaches vim).
  { id: "completion-accept", label: "Accept the suggestion", combo: "ArrowRight", run: () => inTerminal() && completionBridge.accept() },
  { id: "completion-accept-end", label: "Accept the suggestion (End)", combo: "End", run: () => inTerminal() && completionBridge.accept() },
  { id: "completion-accept-word", label: "Accept one word of the suggestion", combo: "Ctrl+ArrowRight", run: () => inTerminal() && completionBridge.acceptWord() },
  // Ctrl+Space, not Tab: Tab is the shell's own completion. Bind "Open the suggestion list (Tab)" to
  // Tab to have it here instead; Tab then still reaches the shell whenever the list can't open.
  { id: "completion-menu", label: "Open the suggestion list", combo: "Ctrl+Space", run: () => inTerminal() && completionBridge.openMenu() },
  { id: "completion-menu-tab", label: "Open the suggestion list (Tab)", combo: "", run: () => inTerminal() && completionBridge.openMenu() },
  { id: "completion-dismiss", label: "Dismiss the suggestion", combo: "Escape", run: () => inTerminal() && completionBridge.dismiss() },
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

  // While the popup is open it owns these keys; with it closed they go on to the shell untouched.
  if (inTerminal() && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
    const key = combo === "ArrowUp" ? "up" : combo === "ArrowDown" ? "down" : combo === "Enter" || combo === "Tab" ? "choose" : null;
    if (key && completionBridge.menuKey(key)) return true;
  }

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
