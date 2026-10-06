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
  /** Extra space between characters, in CSS pixels. */
  letterSpacing: number;
  /** Space around the terminal's text, in CSS pixels. */
  terminalPadding: number;
  /** Raises dim text's contrast against the background (1 = off, xterm's own scale up to 21). */
  minimumContrastRatio: number;
  /** Empty string keeps the active theme's own cursor colour. */
  cursorColor: string;
  /** Bold text uses the bright version of its colour (xterm's default). */
  boldAsBright: boolean;
  /** Extra characters xterm treats as word boundaries for double-click selection; empty keeps xterm's own default. */
  wordSeparator: string;
  /** Announces terminal output to assistive tech; on by default in most screen readers' own detection, but can be forced here. */
  screenReaderMode: boolean;
  /** Drops one trailing newline from a paste, so it lands on the prompt instead of also submitting a blank line. */
  trimPasteNewline: boolean;
  copyOnSelect: boolean;
  /** Programs may put text on the clipboard with OSC 52 (tmux, Claude Code, Neovim). */
  remoteClipboard: boolean;
  /** 0 disables auto-lock. */
  autoLockMinutes: number;
  showHiddenFiles: boolean;
  /** The SFTP view's local/remote split, 0.15-0.85. */
  sftpSplitRatio: number;
  /** Session logs keep colours and control codes instead of plain text. */
  logRaw: boolean;
  appTheme: "dark" | "light" | "system";
  /** The list panel next to the activity bar is hidden (Ctrl+Shift+H). */
  sidebarHidden: boolean;
  /** Its width in pixels (drag its edge; double-click resets). */
  sidebarWidth: number;
  /** Reconnect by itself when a connection drops (not when you exit). */
  autoReconnect: boolean;
  /** Reopen the tabs that were open when the vault was last locked or the app quit. */
  restoreLastSession: boolean;
  /** When restoring, leave production hosts closed; reconnect them by hand. */
  restoreSkipProduction: boolean;
  /** Ask before closing a tab, pane or the app while sessions are connected. */
  confirmCloseSessions: boolean;
  /**
   * Keep a per-computer history of commands run on each host. Off by default:
   * commands can carry passwords and tokens, and the history is plain text.
   * (Renamed from `commandHistory`, which defaulted on, so old installs start off.)
   */
  rememberCommands: boolean;
  /**
   * Smart completion: suggestions while typing at a shell prompt. The master switch turns every
   * part off; the others choose which parts run. Suggestions from history are kept only for the
   * session unless `rememberCommands` is on. See src/lib/completion/.
   */
  smartCompletion: boolean;
  /** A faint suggestion after the cursor, from history. */
  acInline: boolean;
  /** A popup of matches (Tab or Ctrl+Space). */
  acMenu: boolean;
  /** Include snippets in the popup. */
  acSnippets: boolean;
  /** Include command options and subcommands in the popup. */
  acOptions: boolean;
  /** Look up remote file names over an extra SSH channel. */
  acRemotePaths: boolean;
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
  /** A detected shell (see the local shells store) chosen as the default; empty = use `localShell`. */
  localShellId: string;
  /** Local terminal start folder; empty = home. */
  localCwd: string;
  /** Look for a new version at start-up (at most twice a day). */
  autoUpdateCheck: boolean;
  /** The version last shown as "what's new"; empty before this existed. */
  lastSeenVersion: string;
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
  letterSpacing: 0,
  terminalPadding: 4,
  minimumContrastRatio: 1,
  cursorColor: "",
  boldAsBright: true,
  wordSeparator: "",
  screenReaderMode: false,
  trimPasteNewline: false,
  scrollback: 5000,
  copyOnSelect: false,
  remoteClipboard: true,
  autoLockMinutes: 0,
  showHiddenFiles: false,
  sftpSplitRatio: 0.5,
  logRaw: false,
  appTheme: "dark",
  sidebarHidden: false,
  sidebarWidth: 288,
  autoReconnect: true,
  restoreLastSession: true,
  restoreSkipProduction: true,
  confirmCloseSessions: true,
  rememberCommands: false,
  smartCompletion: true,
  acInline: true,
  acMenu: true,
  acSnippets: true,
  acOptions: true,
  acRemotePaths: true,
  notifyBackground: true,
  focusMode: false,
  density: "comfortable",
  keybindings: {},
  customThemes: [],
  prodTint: true,
  localShell: "",
  localShellId: "",
  localCwd: "",
  autoUpdateCheck: true,
  lastSeenVersion: "",
};

/** What "Reset" in Terminal appearance touches. */
export const APPEARANCE_PREFS = [
  "themeId", "fontFamily", "fontSize", "lineHeight", "cursorStyle", "cursorBlink", "scrollback", "appTheme", "prodTint",
  "density", "letterSpacing", "terminalPadding", "minimumContrastRatio", "cursorColor", "boldAsBright", "wordSeparator", "screenReaderMode",
] as const satisfies readonly (keyof Prefs)[];

const KEY = "sshvault.prefs.v1";
const RECENT_KEY = "sshvault.recent.v1";
const RECENT_VAULTS_KEY = "sshvault.recentvaults.v1";
const LAST_SESSION_KEY = "sshvault.lastsession.v1";
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
  /** Vault folder paths opened on this computer, most recent first. Never synced. */
  recentVaults = $state<string[]>(load<string[]>(RECENT_VAULTS_KEY, []));
  /** The tabs open when the vault was last locked or the app quit. Never synced; never holds secrets. */
  lastSession = $state<import("./ui.svelte").WorkspaceTab[]>(load(LAST_SESSION_KEY, []));
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
      $effect(() => save(RECENT_VAULTS_KEY, $state.snapshot(this.recentVaults)));
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

  markRecentVault(path: string) {
    this.recentVaults = [path, ...this.recentVaults.filter((p) => p !== path)].slice(0, 5);
  }

  saveLastSession(tabs: import("./ui.svelte").WorkspaceTab[]) {
    this.lastSession = tabs;
    save(LAST_SESSION_KEY, tabs);
  }

  forgetRecentVault(path: string) {
    this.recentVaults = this.recentVaults.filter((p) => p !== path);
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

  /** Back to the defaults for everything. */
  reset() {
    this.prefs = { ...DEFAULT_PREFS };
  }

  /** Back to the default look only; shortcuts, custom themes and the rest stay. */
  resetAppearance() {
    const prefs = this.prefs as unknown as Record<string, unknown>;
    for (const k of APPEARANCE_PREFS) prefs[k] = DEFAULT_PREFS[k];
  }
}

export const settings = new SettingsStore();
