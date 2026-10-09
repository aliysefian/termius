// What closing the window and the tray's Exit do. Kept free of Svelte and Tauri so the rules are tested on their own.

export interface CloseContext {
  /** Settings → Connections: "Keep running in the tray when the window is closed". */
  closeToTray: boolean;
  /** The system has a tray icon to bring the window back (not every Linux desktop does). */
  trayAvailable: boolean;
  /** Connected sessions in the open tabs. */
  connected: number;
  /** Settings: ask before closing live sessions ("Don't ask again" turns it off). */
  confirmSessions: boolean;
}

/**
 * The window's close button. "hide": into the tray, everything keeps running. "confirm": ask, live sessions would
 * end. "close": quit straight away. Without a tray icon the window is never hidden, so it can't get lost.
 */
export function closeAction(c: CloseContext): "hide" | "confirm" | "close" {
  if (c.closeToTray && c.trayAvailable) return "hide";
  return c.connected > 0 && c.confirmSessions ? "confirm" : "close";
}

/** The tray's Exit always quits; it only asks first when live sessions would end and the person wants to be asked. */
export function exitAction(c: Pick<CloseContext, "connected" | "confirmSessions">): "confirm" | "exit" {
  return c.connected > 0 && c.confirmSessions ? "confirm" : "exit";
}
