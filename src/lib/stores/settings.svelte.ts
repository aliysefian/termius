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
  /** Programs may put text on the clipboard with OSC 52 (tmux, Claude Code, Neovim). */
  remoteClipboard: boolean;
  /** 0 disables auto-lock. */
  autoLockMinutes: number;
  showHiddenFiles: boolean;
  /** Session logs keep colours and control codes instead of plain text. */
  logRaw: boolean;
  appTheme: "dark" | "light" | "system";
  /** The list panel next to the activity bar is hidden (Ctrl+Shift+H). */
  sidebarHidden: boolean;
  /** Its width in pixels (drag its edge; double-click resets). */
  sidebarWidth: number;
  /** Reconnect by itself when a connection drops (not when you exit). */
  autoReconnect: boolean;
  /**
   * Keep a per-computer history of commands run on each host. Off by default:
   * commands can carry passwords and tokens, and the history is plain text.
   * (Renamed from `commandHistory`, which defaulted on, so old installs start off.)
   */
  rememberCommands: boolean;
  /** System notification when a long command finishes in a background tab. */
  notifyBackground: boolean;
  /** Hide the activity bar, list panel, tab strip and pane headers. */
  focusMode: boolean;
  /** Row spacing across lists and headers. */
  density: "comfortable" | "compact";
  /** Shortcut overrides by action id ("" = unbound). */
  keybindings: Record<string, string>;
  /** Terminal colour schemes imported by the user. */
  customThemes: import("$lib/themes").TerminalTheme[];
  /** Tint the terminal background of production hosts red. */
  prodTint: boolean;
  /** Local terminal program (e.g. "zsh", "pwsh -NoLogo", "wsl"); empty = system default. */
  localShell: string;
  /** Local terminal start folder; empty = home. */
  localCwd: string;
  /** Look for a new version at start-up (at most twice a day). */
  autoUpdateCheck: boolean;
}

export interface HistoryEntry {
  command: string;
  at: number;
  exit: number | null;
}

/** When and how often a host was connected to, per computer. */
export interface HostUsage {
  last: number;
  count: number;
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
  remoteClipboard: true,
  autoLockMinutes: 0,
  showHiddenFiles: false,
  logRaw: false,
  appTheme: "dark",
  sidebarHidden: false,
  sidebarWidth: 288,
  autoReconnect: true,
  rememberCommands: false,
  notifyBackground: true,
  focusMode: false,
  density: "comfortable",
  keybindings: {},
  customThemes: [],
  prodTint: true,
  localShell: "",
  localCwd: "",
  autoUpdateCheck: true,
};

const KEY = "sshvault.prefs.v1";
const RECENT_KEY = "sshvault.recent.v1";
const COLLAPSED_KEY = "sshvault.collapsed.v1";
const USAGE_KEY = "sshvault.usage.v1";
const HISTORY_KEY = "sshvault.history.v1";
const HISTORY_PER_HOST = 200;

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

function forget(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {
    // Storage unavailable: nothing was stored either.
  }
}

class SettingsStore {
  prefs = $state<Prefs>(load(KEY, DEFAULT_PREFS));
  /** Host ids, most recent first. */
  recent = $state<string[]>(load<string[]>(RECENT_KEY, []));
  /** Collapsed host-group paths, remembered per computer. */
  collapsedGroups = $state<string[]>(load<string[]>(COLLAPSED_KEY, []));
  usage = $state<Record<string, HostUsage>>(load<Record<string, HostUsage>>(USAGE_KEY, {}));
  /** Commands run per host, newest first. Never synced. */
  history = $state<Record<string, HistoryEntry[]>>({});

  constructor() {
    // Drop the old, default-on switch so it is never re-saved.
    delete (this.prefs as Partial<Prefs> & { commandHistory?: boolean }).commandHistory;
    if (this.prefs.rememberCommands) this.history = load<Record<string, HistoryEntry[]>>(HISTORY_KEY, {});
    $effect.root(() => {
      // While off, nothing is kept in memory or on disk.
      $effect(() => {
        if (!this.prefs.rememberCommands) this.history = {};
      });
      $effect(() => save(KEY, $state.snapshot(this.prefs)));
      $effect(() => save(RECENT_KEY, $state.snapshot(this.recent)));
      $effect(() => save(COLLAPSED_KEY, $state.snapshot(this.collapsedGroups)));
      $effect(() => save(USAGE_KEY, $state.snapshot(this.usage)));
      $effect(() => {
        const history = $state.snapshot(this.history);
        if (this.prefs.rememberCommands) save(HISTORY_KEY, history);
        else forget(HISTORY_KEY);
      });
    });
  }

  markRecent(hostId: string) {
    this.recent = [hostId, ...this.recent.filter((id) => id !== hostId)].slice(0, 20);
    const u = this.usage[hostId];
    this.usage[hostId] = { last: Date.now(), count: (u?.count ?? 0) + 1 };
  }

  recordCommand(hostId: string, command: string, exit: number | null) {
    if (!this.prefs.rememberCommands || !command.trim()) return;
    const prev = (this.history[hostId] ?? []).filter((h) => h.command !== command);
    this.history[hostId] = [{ command, at: Date.now(), exit }, ...prev].slice(0, HISTORY_PER_HOST);
  }

  clearHistory(hostId?: string) {
    if (hostId) delete this.history[hostId];
    else this.history = {};
  }

  zoom(delta: number) {
    this.prefs.fontSize = delta === 0 ? DEFAULT_PREFS.fontSize : Math.min(32, Math.max(8, this.prefs.fontSize + delta));
  }

  reset() {
    this.prefs = { ...DEFAULT_PREFS };
  }
}

export const settings = new SettingsStore();
