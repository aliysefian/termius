// Per-computer preferences. Stored in localStorage because they are
// deliberately *not* synced: font size or auto-lock suit one machine, not all.
export interface Prefs {
  themeId: string;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  cursorStyle: "block" | "bar" | "underline";
  cursorBlink: boolean;
  scrollback: number;
  copyOnSelect: boolean;
  /** 0 disables auto-lock. */
  autoLockMinutes: number;
  showHiddenFiles: boolean;
  /** Session logs keep colours and control codes instead of plain text. */
  logRaw: boolean;
  appTheme: "dark" | "light" | "system";
  /** The list panel next to the activity bar is hidden (Ctrl+Shift+H). */
  sidebarHidden: boolean;
}

export const DEFAULT_PREFS: Prefs = {
  themeId: "sshvault",
  fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", ui-monospace, Menlo, Consolas, monospace',
  fontSize: 13,
  lineHeight: 1.2,
  cursorStyle: "block",
  cursorBlink: true,
  scrollback: 5000,
  copyOnSelect: false,
  autoLockMinutes: 0,
  showHiddenFiles: false,
  logRaw: false,
  appTheme: "dark",
  sidebarHidden: false,
};

const KEY = "sshvault.prefs.v1";
const RECENT_KEY = "sshvault.recent.v1";
const COLLAPSED_KEY = "sshvault.collapsed.v1";

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw);
    return Array.isArray(fallback) ? (parsed as T) : { ...fallback, ...parsed };
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage unavailable (private mode, quota): prefs just won't persist.
  }
}

class SettingsStore {
  prefs = $state<Prefs>(load(KEY, DEFAULT_PREFS));
  /** Host ids, most recent first. */
  recent = $state<string[]>(load<string[]>(RECENT_KEY, []));
  /** Collapsed host-group paths, remembered per computer. */
  collapsedGroups = $state<string[]>(load<string[]>(COLLAPSED_KEY, []));

  constructor() {
    $effect.root(() => {
      $effect(() => save(KEY, $state.snapshot(this.prefs)));
      $effect(() => save(RECENT_KEY, $state.snapshot(this.recent)));
      $effect(() => save(COLLAPSED_KEY, $state.snapshot(this.collapsedGroups)));
    });
  }

  markRecent(hostId: string) {
    this.recent = [hostId, ...this.recent.filter((id) => id !== hostId)].slice(0, 20);
  }

  zoom(delta: number) {
    this.prefs.fontSize = delta === 0 ? DEFAULT_PREFS.fontSize : Math.min(32, Math.max(8, this.prefs.fontSize + delta));
  }

  reset() {
    this.prefs = { ...DEFAULT_PREFS };
  }
}

export const settings = new SettingsStore();
