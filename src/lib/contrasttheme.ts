// The app's high-contrast theme: black and white with visible edges, for low vision and bright rooms.
// Text and status colours are 7:1 or better (WCAG AAA) on every surface. The accent is a bright blue and what
// sits on it is black, not white (a rule in app.css). Kept here, not in the stylesheet, so a test can read it.
export const CONTRAST_TOKENS: Record<string, string> = {
  base: "#000000",
  panel: "#0b0b0b",
  "panel-hover": "#1f1f1f",
  line: "#9a9a9a",
  accent: "#66b3ff",
  "accent-solid": "#66b3ff",
  "accent-hover": "#8cc6ff",
  fg: "#ffffff",
  "fg-muted": "#d0d0d0",
  danger: "#ff9494",
  "danger-solid": "#ff9494",
  success: "#5dff8f",
  warning: "#ffd966",
};
