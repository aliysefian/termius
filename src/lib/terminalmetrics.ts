// What the terminal's metrics toolbar and side panel remember, and when they take readings. Presentation and sampling
// choices only, kept on this computer (never in the vault, never synced). Turning them on is the person's choice per
// host: it makes the app read that host over a second connection, so nothing is on until asked for.
import type { Pane } from "./stores/ui.svelte";
import type { VaultRecord } from "./types";
import type { Host } from "./types";

export const KEY = "sshvault.hostmetrics.terminal.v1";

export const PANEL_MIN = 220;
export const PANEL_MAX = 520;
export const PANEL_DEFAULT = 288;
/** The panel never takes more than this share of the window, so the terminal beside it stays usable. */
export const PANEL_MAX_SHARE = 0.4;

/** How often the toolbar's figures are refreshed while the panel is not showing. */
export const TOOLBAR_EVERY_MS = 5000;
export const LIVE_CHOICES = [0, 1000, 2000, 5000] as const;

export interface HostChoice {
  /** The toolbar is on for this host (and so readings are taken while a terminal of it is in front). */
  enabled: boolean;
  /** The side panel is open for this host. Opening it turns the toolbar on too. */
  panel: boolean;
}

export interface TerminalPrefs {
  side: "left" | "right";
  width: number;
  /** The panel's live rate: 0 is off (the toolbar's own rate still applies). */
  live: (typeof LIVE_CHOICES)[number];
  hosts: Record<string, HostChoice>;
}

export const DEFAULTS: TerminalPrefs = { side: "right", width: PANEL_DEFAULT, live: 2000, hosts: {} };

export const clampWidth = (w: number, windowWidth = 1600): number => {
  const max = Math.max(PANEL_MIN, Math.min(PANEL_MAX, Math.floor(windowWidth * PANEL_MAX_SHARE)));
  return Math.round(Math.max(PANEL_MIN, Math.min(max, Number.isFinite(w) ? w : PANEL_DEFAULT)));
};

/** Whatever was stored, repaired field by field: a bad or old value becomes the default, never an error. */
export function readPrefs(raw: string | null | undefined): TerminalPrefs {
  let o: Record<string, unknown> = {};
  try {
    const v: unknown = raw ? JSON.parse(raw) : {};
    if (v && typeof v === "object" && !Array.isArray(v)) o = v as Record<string, unknown>;
  } catch {
    // Unreadable: start again from the defaults.
  }
  const hosts: Record<string, HostChoice> = {};
  if (o.hosts && typeof o.hosts === "object" && !Array.isArray(o.hosts)) {
    for (const [id, c] of Object.entries(o.hosts as Record<string, unknown>)) {
      if (Object.keys(hosts).length >= 1000 || id.length > 64 || !c || typeof c !== "object") continue;
      const { enabled, panel } = c as Record<string, unknown>;
      // An open panel is always a host that is on.
      if (enabled === true || panel === true) hosts[id] = { enabled: true, panel: panel === true };
    }
  }
  return {
    side: o.side === "left" ? "left" : "right",
    width: typeof o.width === "number" ? clampWidth(o.width) : PANEL_DEFAULT,
    live: (LIVE_CHOICES as readonly unknown[]).includes(o.live) ? (o.live as TerminalPrefs["live"]) : DEFAULTS.live,
    hosts,
  };
}

export function loadPrefs(): TerminalPrefs {
  try {
    return readPrefs(localStorage.getItem(KEY));
  } catch {
    return { ...DEFAULTS, hosts: {} };
  }
}

export function savePrefs(p: TerminalPrefs) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    // Not fatal: the choices just won't be there next time.
  }
}

/**
 * The host whose metrics a pane could show: a saved host reached over SSH. Telnet, remote desktops, local shells, serial
 * ports and quick connections have no saved credentials to read it with, and nothing here guesses.
 */
export function metricsHost(target: Pane["target"] | undefined, hostById: Map<string, VaultRecord<Host>>): string | null {
  if (!target || target.kind !== "host") return null;
  const protocol = hostById.get(target.hostId)?.data?.protocol ?? "";
  return protocol === "" || protocol === "ssh" ? target.hostId : null;
}

/**
 * How often to read, in milliseconds, or null for not at all. Off hosts are never read. The panel's live rate applies
 * only while the panel is on screen (not behind another page, not hidden); otherwise the toolbar's slow rate does.
 */
export function wantedInterval(choice: HostChoice | undefined, panelVisible: boolean, live: number): number | null {
  if (!choice?.enabled) return null;
  return choice.panel && panelVisible && live > 0 ? live : TOOLBAR_EVERY_MS;
}
