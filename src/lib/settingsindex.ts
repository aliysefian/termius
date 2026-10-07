// What the command palette can find in Settings: a name for each setting or group of settings and the word
// the Settings page is searched with to show it. Every `query` must match a section of the page; a test
// reads SettingsPanel.svelte to make sure none of them points at nothing.
export interface SettingEntry {
  label: string;
  /** Typed into the Settings search box. */
  query: string;
  /** Other words that should find it in the palette. */
  words?: string;
}

export const SETTING_ENTRIES: SettingEntry[] = [
  { label: "Terminal theme and colours", query: "theme", words: "dark light colour color scheme" },
  { label: "Font and font size", query: "font", words: "typeface zoom size" },
  { label: "Cursor style and blink", query: "cursor" },
  { label: "Scrollback length", query: "scrollback", words: "history lines buffer" },
  { label: "Copy on select and clipboard", query: "clipboard", words: "copy paste osc 52" },
  { label: "Session logging", query: "session log", words: "record raw" },
  { label: "Screen reader mode", query: "screen reader", words: "accessibility" },
  { label: "Auto-lock after inactivity", query: "auto-lock", words: "timeout minutes security" },
  { label: "Updates and changelog", query: "update", words: "version release install" },
  { label: "Sidebar: names, hide, show and reorder", query: "sidebar", words: "rail icons labels manage pin" },
  { label: "Layout density and focus mode", query: "density", words: "compact comfortable focus mode" },
  { label: "Local terminal shell", query: "local terminal", words: "bash zsh fish powershell wsl" },
  { label: "Smart completion", query: "smart completion", words: "autocomplete suggestions ghost" },
  { label: "Alerts and monitoring history", query: "alerts", words: "notify cpu memory disk quiet hours charts" },
  { label: "Auto-reconnect", query: "auto-reconnect", words: "connection drop" },
  { label: "Remember commands and history", query: "remember", words: "command history" },
  { label: "Reopen tabs from last time", query: "restore session", words: "reopen tabs" },
  { label: "Paste protection", query: "paste", words: "newline confirm multi-line" },
  { label: "Command line (CLI)", query: "command line", words: "cli scripting sshvault" },
  { label: "Shell integration", query: "shell integration", words: "osc 133 prompt directory" },
  { label: "Safety: destructive command patterns", query: "destructive", words: "production confirm" },
  { label: "Backups and retention", query: "backup", words: "retention" },
  { label: "Export hosts as ssh config", query: "export ssh config", words: "openssh include ansible" },
  { label: "Reset settings to defaults", query: "reset", words: "default" },
];
