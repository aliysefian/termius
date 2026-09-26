// Copy of models::default_destructive() in the Rust backend.
export const DEFAULT_DESTRUCTIVE = [
  String.raw`\brm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)+/`,
  String.raw`\bmkfs(\.\w+)?\b`,
  String.raw`\bdd\b.*\bof=/dev/`,
  String.raw`(^|[;&|]|\bsudo)\s*(shutdown|reboot|poweroff|halt)(\s|$)`,
  String.raw`(?i)\bdrop\s+(database|table|schema)\b`,
  String.raw`(?i)\btruncate\s+table\b`,
  String.raw`\bkubectl\s+delete\b`,
  String.raw`\bterraform\s+destroy\b`,
  String.raw`\bgit\s+push\b.*(--force|\s-f\b)`,
  String.raw`\bsystemctl\s+(stop|disable|mask)\b`,
  String.raw`:\(\)\s*\{\s*:\|:&\s*\};:`,
];
