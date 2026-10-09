// Per-computer preferences. Stored in localStorage because they are
// deliberately *not* synced: font size or auto-lock suit one machine, not all.
import type { Alert, Severity } from "$lib/ops/alerts";
import type { HighlightRule } from "$lib/highlight";
import type { Schedule } from "$lib/schedule";
import { completionHistory, rejectReason } from "$lib/completion/history";

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
  appTheme: "dark" | "light" | "system" | "contrast";
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
  /** Learn from the commands already in the host's own shell history files (asked over the lookup channel). */
  acSeedHistory: boolean;
  /** Tell me when a monitored host stops answering or runs hot. Off until switched on. */
  alerts: boolean;
  alertDown: boolean;
  /** Percent limits; 0 turns one off. */
  alertCpu: number;
  alertMem: number;
  alertDisk: number;
  /** Tell me when a systemd service on a monitored host fails. */
  alertServices: boolean;
  /** Readings in a row over a limit before it counts. */
  alertSamples: number;
  alertQuiet: boolean;
  alertQuietFrom: string;
  alertQuietTo: string;
  /** Hosts that never alert. */
  alertMuted: string[];
  /** Announce (toast, notification) only alerts at least this serious; all of them still reach the list. */
  alertNotifyFrom: Severity;
  /** Keep the alert list on this computer between runs. Off: it lasts until the app closes or the vault locks. */
  alertKeepHistory: boolean;
  /** The kept alerts, when `alertKeepHistory` is on (host names and messages, never secrets). */
  alertHistory: Alert[];
  /** Keep the monitoring charts for longer than 15 minutes, on this computer. */
  metricsKeep: "off" | "day" | "week";
  /** The names under the sidebar icons. */
  railLabels: boolean;
  /** Show pictures a program draws in the terminal (Sixel, iTerm2). Takes effect in terminals opened afterwards. */
  terminalImages: boolean;
  /** Words coloured in terminal output, and which of them raise a notice. */
  highlightRules: HighlightRule[];
  /** A coloured bar in the margin for each command the shell reports (needs shell integration). */
  commandBlocks: boolean;
  /** Hosts whose connect command was approved: host id → fingerprint of the exact command. */
  approvedCommands: Record<string, string>;
  /** Scheduled runbooks, per computer; they run only while the app is open. */
  schedules: Schedule[];
  /** The language of the sidebar, menus and headings: a code like "de", or "auto" to follow the system. */
  language: string;
  /** Sidebar entries put away into the Manage menu (page names, e.g. "databases"). */
  railHidden: string[];
  /** Order of the sidebar entries within their groups (page names; what is not listed keeps its place). */
  railOrder: string[];
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
  cwd?: string;
  prev?: string;
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
  smartCompletion: false,
  acInline: true,
  acMenu: true,
  acSnippets: true,
  acOptions: true,
  acRemotePaths: true,
  acSeedHistory: false,
  alerts: false,
  alertDown: true,
  alertCpu: 90,
  alertMem: 90,
  alertDisk: 90,
  alertServices: false,
  alertSamples: 3,
  alertQuiet: false,
  alertQuietFrom: "22:00",
  alertQuietTo: "07:00",
  alertMuted: [],
  alertNotifyFrom: "info",
  alertKeepHistory: false,
  alertHistory: [],
  metricsKeep: "off",
  railLabels: true,
  terminalImages: true,
  highlightRules: [],
  commandBlocks: true,
  approvedCommands: {},
  schedules: [],
  language: "auto",
  railHidden: [],
  railOrder: [],
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

  recordCommand(hostId: string, command: string, exit: number | null, extra: { cwd?: string; prev?: string; leadingSpace?: boolean } = {}) {
    if (!this.prefs.rememberCommands || !command.trim()) return;
    // Secret-looking and leading-space commands are never written to storage.
    if (rejectReason({ command, exit, leadingSpace: extra.leadingSpace })) return;
    const prev = (this.history[hostId] ?? []).filter((h) => h.command !== command);
    const entry: HistoryEntry = { command, at: Date.now(), exit };
    if (extra.cwd) entry.cwd = extra.cwd;
    if (extra.prev) entry.prev = extra.prev;
    this.history[hostId] = [entry, ...prev].slice(0, HISTORY_PER_HOST);
  }

  clearHistory(hostId?: string) {
    completionHistory.clear(hostId);
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
