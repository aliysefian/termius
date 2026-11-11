// What can be done to a node of the tmux tree, as data: the menu for a host, session, window or pane, with what is not
// available and why. The monitor, the explorer and the terminal's tmux panel all show these, so they agree.
import type { HostSnapshot, Row } from "./monitor";
import { locate } from "./monitor";

export type ActionId =
  | "refresh" | "new-session" | "add-socket" | "remove-host"
  | "attach-live" | "attach-tab" | "attach-right" | "attach-down" | "attach-replace" | "rename-session" | "new-window" | "tags" | "mouse-on" | "mouse-off" | "kill-session"
  | "select-window" | "rename-window" | "kill-window"
  | "select-pane" | "split-right" | "split-down" | "zoom-pane" | "title-pane" | "copy-pane" | "kill-pane";

export interface Action {
  id: ActionId;
  label: string;
  danger?: boolean;
  disabled?: boolean;
  why?: string;
  /** Actions with the same group are drawn together. */
  group: string;
}

export type Target = Pick<Row, "kind" | "sessionId" | "windowId" | "paneId">;

/**
 * The actions for a node. `canSplit` says whether a terminal tab has room to split; `canReplace` whether the active terminal is on
 * this host. `live` is given only where the live terminal can be shown (the monitor page, not the panel beside a terminal): it says
 * whether it can be opened for this host and, if not, why.
 */
export interface ActionContext { canSplit: boolean; canReplace: boolean; live?: { allowed: boolean; why?: string } }
export function actionsFor(row: Target, snap: HostSnapshot | undefined, ctx: ActionContext = { canSplit: true, canReplace: true }): Action[] {
  const live: Action[] = ctx.live ? [{ id: "attach-live", label: "Open live terminal here", group: "a", disabled: !ctx.live.allowed, why: ctx.live.allowed ? undefined : ctx.live.why }] : [];
  const here = locate(snap, row);
  const stale = !!snap?.stale;
  const out: Action[] = [];
  const gone = (what: string): Action[] => [{ id: "refresh", label: "Refresh", group: "a" }, { id: "attach-tab", label: `${what} no longer exists`, disabled: true, why: `This ${what.toLowerCase()} no longer exists on the remote host.`, group: "b" }];
  switch (row.kind) {
    case "host":
      out.push({ id: "new-session", label: "New session…", group: "a" }, { id: "refresh", label: "Refresh", group: "a" }, { id: "add-socket", label: "Add a tmux socket…", group: "b" }, { id: "remove-host", label: "Remove from the monitor", group: "c" });
      break;
    case "session": {
      if (!here.session) return gone("Session");
      out.push(
        ...live,
        { id: "attach-tab", label: "Attach in a new tab", group: "a" },
        { id: "attach-right", label: "Open in split right", group: "a", disabled: !ctx.canSplit, why: ctx.canSplit ? undefined : "There is no terminal tab with room to split." },
        { id: "attach-down", label: "Open in split down", group: "a", disabled: !ctx.canSplit, why: ctx.canSplit ? undefined : "There is no terminal tab with room to split." },
        { id: "attach-replace", label: "Replace the current terminal…", group: "a", disabled: !ctx.canReplace, why: ctx.canReplace ? undefined : "The active terminal is not on this host." },
        { id: "new-window", label: "New window…", group: "b" },
        { id: "rename-session", label: "Rename…", group: "b" },
        { id: "tags", label: "Edit tags…", group: "b" },
        here.session.mouse ? { id: "mouse-off", label: "Turn tmux mouse mode off", group: "b" } : { id: "mouse-on", label: "Turn tmux mouse mode on", group: "b" },
        { id: "kill-session", label: "Kill session…", danger: true, group: "c" },
      );
      break;
    }
    case "window": {
      if (!here.window) return gone("Window");
      out.push(
        ...live,
        { id: "select-window", label: "Make this the active window", group: "a", disabled: here.window.active, why: here.window.active ? "It is already the active window." : undefined },
        { id: "rename-window", label: "Rename…", group: "a" },
        { id: "new-window", label: "New window…", group: "a" },
        { id: "kill-window", label: "Kill window…", danger: true, group: "c" },
      );
      break;
    }
    case "pane": {
      if (!here.pane) return gone("Pane");
      out.push(
        { id: "select-pane", label: "Select pane", group: "a", disabled: here.pane.active && !!here.window?.active, why: "It is already the active pane." },
        { id: "split-right", label: "Split right", group: "a" },
        { id: "split-down", label: "Split down", group: "a" },
        { id: "zoom-pane", label: here.window?.zoomed ? "Unzoom" : "Zoom", group: "a", disabled: (here.window?.panes.length ?? 0) < 2, why: "A window with one pane has nothing to zoom." },
        ...live,
        { id: "title-pane", label: "Set title…", group: "b" },
        { id: "copy-pane", label: "Copy visible text", group: "b" },
        { id: "attach-tab", label: "Attach to its session in a new tab", group: "b" },
        { id: "kill-pane", label: "Kill pane…", danger: true, group: "c" },
      );
      break;
    }
    default:
      return [];
  }
  // A stale reading is what was last known, not what is true now; changes wait for a fresh one.
  return out.map((a) => (stale && !["refresh", "add-socket", "remove-host", "copy-pane"].includes(a.id) ? { ...a, disabled: true, why: "The host could not be read just now. Refresh first." } : a));
}
