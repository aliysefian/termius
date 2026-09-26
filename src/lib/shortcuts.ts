// App-wide keyboard shortcuts. Registered in the capture phase so they win
// over xterm.js, which otherwise swallows keys aimed at the terminal. Most
// use Ctrl+Shift so plain Ctrl combos (Ctrl+W, Ctrl+T, ...) still reach the
// remote shell, where readline and editors rely on them.
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";

function inTextField(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  // xterm's hidden textarea counts as the terminal, not a text field.
  if (el.classList?.contains("xterm-helper-textarea")) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
}

export function handleShortcut(e: KeyboardEvent): boolean {
  if (!vaultStore.unlocked) return false;
  const ctrl = e.ctrlKey || e.metaKey;
  if (!ctrl || e.altKey) return false;

  if (e.shiftKey) {
    switch (e.code) {
      case "KeyP":
        ui.paletteOpen = !ui.paletteOpen;
        return true;
      case "KeyT":
        ui.modal = { kind: "quick-connect" };
        return true;
      case "KeyW":
        if (ui.activeTabId) ui.closeTab(ui.activeTabId);
        return true;
      case "KeyD":
        ui.splitActive("vertical");
        return true;
      case "KeyE":
        ui.splitActive("horizontal");
        return true;
      case "KeyL":
        void vaultStore.lock();
        return true;
      case "KeyB":
        ui.syncRequest++;
        return true;
      case "Backquote":
        ui.openLocal();
        return true;
      case "Tab":
        ui.cycleTab(-1);
        return true;
      case "KeyF":
        // Terminals handle this themselves when focused; this covers the
        // case where focus is elsewhere in the window.
        if (ui.activeTab && !inTextField(e.target)) {
          ui.findRequest++;
          return true;
        }
        return false;
    }
    return false;
  }

  switch (e.code) {
    case "Tab":
      ui.cycleTab(1);
      return true;
    case "PageDown":
      ui.cycleTab(1);
      return true;
    case "PageUp":
      ui.cycleTab(-1);
      return true;
    case "KeyK":
      // Ctrl+K kills to end of line in shells, so only take it outside terminals.
      if (document.activeElement?.classList.contains("xterm-helper-textarea")) return false;
      ui.paletteOpen = !ui.paletteOpen;
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
  return false;
}
