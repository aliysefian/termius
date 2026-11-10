// The Tmux Monitor's data model and the pure logic around it: what the backend sends (src-tauri/src/tmux), the tree the
// explorer draws from it, the search over its metadata, and the rules for accepting and presenting a reading.
//
// Nothing here touches Tauri, the DOM or a store, so all of it is tested directly. Remote tmux state (sessions, windows,
// panes) is kept apart from SSHVault's own terminal state: a terminal tab is never part of a snapshot.
import type { Uuid } from "$lib/types";

// -- what the backend sends ------------------------------------------------------------------------------------------

export type Socket = { kind: "default" } | { kind: "name"; value: string } | { kind: "path"; value: string };

export const DEFAULT_SOCKET: Socket = { kind: "default" };

export interface Version {
  major: number;
  minor: number;
  dev: boolean;
  raw: string;
}

export interface Pane {
  id: string;
  index: number;
  active: boolean;
  title: string;
  command: string;
  path: string;
  pid: number | null;
  width: number;
  height: number;
  dead: boolean;
  in_mode: boolean;
}

export interface Window {
  id: string;
  index: number;
  name: string;
  active: boolean;
  layout: string;
  zoomed: boolean;
  activity: boolean;
  bell: boolean;
  panes: Pane[];
}

export interface Session {
  id: string;
  name: string;
  attached: number;
  created: number;
  activity: number;
  group: string | null;
  /** The session's mouse option; null when this tmux does not say. */
  mouse: boolean | null;
  windows: Window[];
}

export interface Server {
  pid: number;
  started: number;
  socket_path: string;
}

export type ServerState =
  | { kind: "not_installed"; os: string }
  | { kind: "no_server" }
  | { kind: "empty" }
  | { kind: "unsupported"; version: string }
  | { kind: "permission_denied"; message: string }
  | { kind: "failed"; message: string }
  | { kind: "ready" };

export interface Stale {
  since_ms: number;
  message: string;
}

export interface HostSnapshot {
  host_id: Uuid;
  socket: Socket;
  generation: number;
  taken_at_ms: number;
  version: Version | null;
  state: ServerState;
  server: Server | null;
  sessions: Session[];
  other_sockets: string[];
  stale: Stale | null;
}

export interface PaneInfo {
  id: string;
  width: number;
  height: number;
  command: string;
  dead: boolean;
  alternate: boolean;
  cursor_x: number;
  cursor_y: number;
  in_mode: boolean;
  history: number;
  pid: number | null;
  title: string;
}

export interface PaneCapture {
  info: PaneInfo;
  text: string;
  taken_at_ms: number;
}

export interface Hit {
  line: number;
  text: string;
  spans: [number, number][];
  before: string[];
  after: string[];
}

export interface PaneHits {
  session_id: string;
  window_id: string;
  pane_id: string;
  hits: Hit[];
}

export interface SearchReport {
  results: PaneHits[];
  failed: [string, string][];
  searched: number;
  limited: boolean;
  cancelled: boolean;
}

export interface Usage {
  processes: number;
  rss: number;
  cpu_pct: number | null;
  top: [string, number][];
}

export interface SessionUsage {
  session_id: string;
  usage: Usage | null;
  panes_seen: number;
  panes_total: number;
}

export interface Resources {
  host_id: Uuid;
  taken_at_ms: number;
  sessions: SessionUsage[];
  panes: Record<string, Usage>;
  cpu_note: string | null;
  estimate: boolean;
}

export type SplitDir = "right" | "down";
export type ResizeDir = "up" | "down" | "left" | "right";

export type Op =
  | { op: "new_session"; name: string | null; cwd: string | null; command: string | null }
  | { op: "rename_session"; session: string; name: string }
  | { op: "kill_session"; session: string }
  | { op: "new_window"; session: string; name: string | null; cwd: string | null; command: string | null }
  | { op: "rename_window"; window: string; name: string }
  | { op: "select_window"; window: string }
  | { op: "kill_window"; window: string }
  | { op: "split_pane"; pane: string; dir: SplitDir; cwd: string | null; command: string | null }
  | { op: "select_pane"; pane: string }
  | { op: "set_pane_title"; pane: string; title: string }
  | { op: "resize_pane"; pane: string; dir: ResizeDir; amount: number }
  | { op: "zoom_pane"; pane: string }
  | { op: "kill_pane"; pane: string }
  | { op: "set_mouse"; session: string; on: boolean };

export interface OpResult {
  created: string | null;
  snapshot: HostSnapshot;
}

// -- identity --------------------------------------------------------------------------------------------------------

export const socketKey = (s: Socket): string => (s.kind === "default" ? "default" : s.kind === "name" ? `L:${s.value}` : `S:${s.value}`);

export const socketLabel = (s: Socket): string => (s.kind === "default" ? "default socket" : s.kind === "name" ? `socket ${s.value}` : s.value);

/** One tmux server on one host. Ids are only meaningful together with this. */
export const serverKey = (hostId: Uuid, s: Socket): string => `${hostId}|${socketKey(s)}`;

export const snapshotKey = (s: HostSnapshot): string => serverKey(s.host_id, s.socket);

/** Keys for tree nodes. Built from tmux ids and the server, never from names, so a rename keeps a node's identity. */
export const nodeKey = {
  host: (server: string) => `h:${server}`,
  session: (server: string, id: string) => `s:${server}:${id}`,
  window: (server: string, id: string) => `w:${server}:${id}`,
  pane: (server: string, id: string) => `p:${server}:${id}`,
};

// -- accepting readings ----------------------------------------------------------------------------------------------

/**
 * Keep the newer of two snapshots of the same server. A slow answer that arrives after a faster, newer one is dropped, and an
 * equal generation is the same reading arriving twice (the event and the reply to a request).
 */
export function accept(current: HostSnapshot | undefined, incoming: HostSnapshot): HostSnapshot {
  return current && incoming.generation <= current.generation ? current : incoming;
}

/** The same content, ignoring when it was read: used to skip redraws. */
export function sameContent(a: HostSnapshot, b: HostSnapshot): boolean {
  return JSON.stringify([a.state, a.sessions, a.server, a.stale, a.version, a.other_sockets]) === JSON.stringify([b.state, b.sessions, b.server, b.stale, b.version, b.other_sockets]);
}

// -- what a snapshot says to the person ------------------------------------------------------------------------------

export interface Notice {
  tone: "info" | "warn" | "bad";
  title: string;
  detail: string;
  /** What to do about it, when there is something. */
  hint?: string;
  /** Offer "Create Session" (the server is simply empty). */
  canCreate: boolean;
}

/** How to get tmux on a host, by what the probe found of its system. The app never installs anything itself. */
export function installHint(os: string): string {
  const [kernel, distro = ""] = os.split("|");
  const [id = "", like = ""] = distro.split("|");
  const family = `${id} ${like}`.toLowerCase();
  if (/debian|ubuntu|mint|raspbian/.test(family)) return "sudo apt install tmux";
  if (/fedora|rhel|centos|rocky|alma|amzn|ol\b/.test(family)) return "sudo dnf install tmux (or: sudo yum install tmux)";
  if (/arch|manjaro|endeavour/.test(family)) return "sudo pacman -S tmux";
  if (/alpine/.test(family)) return "sudo apk add tmux";
  if (/suse|sles/.test(family)) return "sudo zypper install tmux";
  if (/gentoo/.test(family)) return "sudo emerge app-misc/tmux";
  if (kernel === "Darwin") return "brew install tmux";
  if (/freebsd/i.test(kernel)) return "sudo pkg install tmux";
  if (/openbsd/i.test(kernel)) return "doas pkg_add tmux";
  return "Install tmux with this system's package manager.";
}

/** The message for a host that has no sessions to list, or null when it has some. */
export function noticeFor(s: HostSnapshot | undefined): Notice | null {
  if (!s) return { tone: "info", title: "Not read yet", detail: "This host's tmux has not been read yet.", canCreate: false };
  const st = s.state;
  switch (st.kind) {
    case "ready":
      return null;
    case "not_installed":
      return { tone: "warn", title: "Tmux is not installed on this host.", detail: "Nothing was changed on the host.", hint: installHint(st.os), canCreate: false };
    case "no_server":
      return { tone: "info", title: "No tmux sessions found.", detail: "No tmux server is running for this socket.", canCreate: true };
    case "empty":
      return { tone: "info", title: "No tmux sessions found.", detail: "The tmux server is running without sessions.", canCreate: true };
    case "unsupported":
      return { tone: "warn", title: "This tmux is too old to be managed here.", detail: `Found ${st.version}. Version 2.0 or newer is needed.`, canCreate: false };
    case "permission_denied":
      return { tone: "bad", title: "The current SSH user cannot access this tmux server.", detail: st.message, canCreate: false };
    case "failed":
      return { tone: "bad", title: "Tmux gave an answer that could not be used.", detail: st.message, canCreate: false };
  }
}

/** What to say about a failed request, given the code of the error the command returned. */
export function errorNotice(code: string | undefined, message: string): Notice {
  switch (code) {
    case "tmux_exec":
      return { tone: "bad", title: "Unable to refresh sessions because the SSH connection is unavailable.", detail: message, canCreate: false };
    case "tmux_timeout":
      return { tone: "bad", title: "The host did not answer in time.", detail: message, canCreate: false };
    case "tmux_server_changed":
      return { tone: "warn", title: "The tmux server on this host has restarted.", detail: "The list was refreshed; choose again.", canCreate: false };
    default:
      return { tone: "bad", title: "Tmux could not be read.", detail: message, canCreate: false };
  }
}

// -- the tree --------------------------------------------------------------------------------------------------------

export interface HostEntry {
  hostId: Uuid;
  name: string;
  socket: Socket;
}

export type RowKind = "host" | "session" | "window" | "pane" | "notice";

export interface Row {
  key: string;
  kind: RowKind;
  depth: number;
  label: string;
  /** Short facts shown after the label. */
  meta: string;
  hostId: Uuid;
  server: string;
  socket: Socket;
  sessionId?: string;
  windowId?: string;
  paneId?: string;
  expandable: boolean;
  expanded: boolean;
  /** Active tmux session/window/pane, or an attached session. */
  active: boolean;
  attached?: boolean;
  tags?: string[];
  /** For notice rows: the message. */
  notice?: Notice;
  stale?: boolean;
}

/** The marker that a host row has been collapsed. */
export const closedKey = (hostKey: string): string => `closed:${hostKey}`;

export type TagsFor = (server: string, sessionName: string) => string[];

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Case-insensitive, every word of the query must be somewhere in the haystack. */
export function matches(query: string, ...fields: (string | undefined)[]): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = fields.filter(Boolean).join("\n").toLowerCase();
  return words.every((w) => hay.includes(w));
}

/**
 * The rows the explorer shows. A search over metadata (host, session, window, pane title, command, tags) keeps a node when it,
 * or anything below it, matches, and opens the path to every match. It does not look at pane output: see the search panel.
 */
export function buildRows(entries: HostEntry[], snapshots: ReadonlyMap<string, HostSnapshot>, expanded: ReadonlySet<string>, query: string, tagsFor: TagsFor): Row[] {
  const rows: Row[] = [];
  const q = query.trim();
  for (const e of entries) {
    const server = serverKey(e.hostId, e.socket);
    const snap = snapshots.get(server);
    const hostKey = nodeKey.host(server);
    const sessionRows: Row[] = [];
    for (const s of snap?.sessions ?? []) {
      const tags = tagsFor(server, s.name);
      const sKey = nodeKey.session(server, s.id);
      const windowRows: Row[] = [];
      // Whether the session itself (or its host) matches, apart from anything below it: a match deeper down must not make
      // its unrelated siblings match too.
      const sessionSelf = !q || matches(q, e.name, s.name, tags.join(" "));
      let anyBelow = false;
      for (const w of s.windows) {
        const wKey = nodeKey.window(server, w.id);
        const paneRows: Row[] = [];
        const windowSelf = !q || sessionSelf || matches(q, w.name);
        for (const p of w.panes) {
          if (q && !sessionSelf && !windowSelf && !matches(q, p.title, p.command, p.path)) continue;
          paneRows.push({
            key: nodeKey.pane(server, p.id), kind: "pane", depth: 3, label: `${p.id} ${p.command || p.title || "shell"}`, meta: p.dead ? "exited" : `${p.width}×${p.height}`,
            hostId: e.hostId, server, socket: e.socket, sessionId: s.id, windowId: w.id, paneId: p.id, expandable: false, expanded: false, active: p.active && w.active,
          });
        }
        if (q && !windowSelf && !paneRows.length) continue;
        anyBelow = true;
        const open = q ? true : expanded.has(wKey);
        windowRows.push({
          key: wKey, kind: "window", depth: 2, label: `${w.index}: ${w.name}`, meta: plural(w.panes.length, "pane") + (w.zoomed ? " · zoomed" : ""),
          hostId: e.hostId, server, socket: e.socket, sessionId: s.id, windowId: w.id, expandable: w.panes.length > 0, expanded: open, active: w.active,
        });
        if (open) windowRows.push(...paneRows);
      }
      if (q && !sessionSelf && !anyBelow) continue;
      const open = q ? true : expanded.has(sKey);
      sessionRows.push({
        key: sKey, kind: "session", depth: 1, label: s.name, meta: `${plural(s.windows.length, "window")} · ${s.attached > 0 ? `${s.attached} attached` : "detached"}`,
        hostId: e.hostId, server, socket: e.socket, sessionId: s.id, expandable: s.windows.length > 0, expanded: open, active: s.attached > 0, attached: s.attached > 0, tags,
      });
      if (open) sessionRows.push(...windowRows);
    }
    const notice = snap && snap.state.kind !== "ready" ? noticeFor(snap) : null;
    const hostMatches = !q || matches(q, e.name);
    if (q && !hostMatches && !sessionRows.length) continue;
    // Hosts start open; closing one is remembered as `closed:<key>` so the others are unaffected.
    const open = q ? true : !expanded.has(closedKey(hostKey));
    const count = snap?.sessions.length ?? 0;
    rows.push({
      key: hostKey, kind: "host", depth: 0, label: e.name,
      meta: !snap ? "reading…" : snap.state.kind === "ready" ? plural(count, "session") : (notice?.title.replace(/\.$/, "") ?? ""),
      hostId: e.hostId, server, socket: e.socket, expandable: true, expanded: open, active: false, stale: !!snap?.stale,
    });
    if (open) {
      if (notice && snap?.state.kind !== "ready") rows.push({ key: `${hostKey}:notice`, kind: "notice", depth: 1, label: notice.title, meta: "", hostId: e.hostId, server, socket: e.socket, expandable: false, expanded: false, active: false, notice });
      rows.push(...sessionRows);
    }
  }
  return rows;
}

/** Find a node's row by key. */
export const findRow = (rows: Row[], key: string): Row | undefined => rows.find((r) => r.key === key);

/** Move through the visible rows (arrow keys). */
export function step(rows: Row[], key: string | null, by: 1 | -1): string | null {
  const nav = rows.filter((r) => r.kind !== "notice");
  if (!nav.length) return null;
  const at = nav.findIndex((r) => r.key === key);
  if (at < 0) return nav[by === 1 ? 0 : nav.length - 1].key;
  return nav[Math.min(nav.length - 1, Math.max(0, at + by))].key;
}

/** The row that holds `row` (its parent), for the Left arrow. */
export function parentOf(rows: Row[], row: Row): Row | undefined {
  const at = rows.indexOf(row);
  for (let i = at - 1; i >= 0; i--) if (rows[i].depth === row.depth - 1) return rows[i];
  return undefined;
}

// -- looking things up -----------------------------------------------------------------------------------------------

export interface Located {
  session?: Session;
  window?: Window;
  pane?: Pane;
}

/** What a row points at in the snapshot it came from, or nothing when it has since gone ("this session no longer exists"). */
export function locate(snap: HostSnapshot | undefined, row: Pick<Row, "sessionId" | "windowId" | "paneId">): Located {
  const session = snap?.sessions.find((s) => s.id === row.sessionId);
  const window = row.windowId ? session?.windows.find((w) => w.id === row.windowId) : undefined;
  const pane = row.paneId ? window?.panes.find((p) => p.id === row.paneId) : undefined;
  return { session, window, pane };
}

/** Every pane of a session, with the window it is in. */
export const panesOf = (s: Session): { window: Window; pane: Pane }[] => s.windows.flatMap((w) => w.panes.map((pane) => ({ window: w, pane })));

export const paneCount = (s: Session): number => s.windows.reduce((n, w) => n + w.panes.length, 0);

/** The session's active pane: the active pane of its active window. */
export function activePane(s: Session): { window: Window; pane: Pane } | undefined {
  const w = s.windows.find((x) => x.active) ?? s.windows[0];
  const p = w?.panes.find((x) => x.active) ?? w?.panes[0];
  return w && p ? { window: w, pane: p } : undefined;
}

// -- formatting ------------------------------------------------------------------------------------------------------

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

/** `3 s ago`, `5 min ago`, from seconds. */
export function ago(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 5) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

export const when = (unixSeconds: number): string => (unixSeconds > 0 ? new Date(unixSeconds * 1000).toLocaleString() : "unknown");

// -- previews --------------------------------------------------------------------------------------------------------

/** How often each kind of capture may be repeated while the monitor is visible. All bounded (see the preferences). */
export interface PreviewPlan {
  /** Panes to capture now, selected one first. */
  due: string[];
}

export interface PreviewPrefs {
  /** The selected pane. */
  selectedMs: number;
  /** Other panes in the gallery. */
  thumbnailMs: number;
  /** At most this many thumbnails are kept fresh at once. */
  maxThumbnails: number;
}

export const PREVIEW_DEFAULTS: PreviewPrefs = { selectedMs: 3000, thumbnailMs: 8000, maxThumbnails: 12 };

export const PREVIEW_LIMITS = { selectedMs: [1000, 30000], thumbnailMs: [5000, 60000], maxThumbnails: [1, 24] } as const;

export function clampPrefs(p: Partial<PreviewPrefs>): PreviewPrefs {
  const c = (v: number | undefined, [lo, hi]: readonly [number, number], d: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : d);
  return {
    selectedMs: c(p.selectedMs, PREVIEW_LIMITS.selectedMs, PREVIEW_DEFAULTS.selectedMs),
    thumbnailMs: c(p.thumbnailMs, PREVIEW_LIMITS.thumbnailMs, PREVIEW_DEFAULTS.thumbnailMs),
    maxThumbnails: c(p.maxThumbnails, PREVIEW_LIMITS.maxThumbnails, PREVIEW_DEFAULTS.maxThumbnails),
  };
}

/**
 * Which panes to capture now. Only what is on screen is captured: the selected pane often, the visible thumbnails (up to a
 * cap) rarely, and nothing that is collapsed, scrolled out of view or on a hidden monitor. `lastAt` is when each pane was
 * last captured, by key.
 */
export function previewsDue(now: number, selected: string | null, visible: string[], lastAt: ReadonlyMap<string, number>, prefs: PreviewPrefs, monitorVisible: boolean): string[] {
  if (!monitorVisible) return [];
  const due: string[] = [];
  if (selected && now - (lastAt.get(selected) ?? 0) >= prefs.selectedMs) due.push(selected);
  let thumbs = 0;
  for (const k of visible) {
    if (k === selected) continue;
    if (thumbs >= prefs.maxThumbnails) break;
    thumbs++;
    if (now - (lastAt.get(k) ?? 0) >= prefs.thumbnailMs) due.push(k);
  }
  return due;
}

// -- resizing --------------------------------------------------------------------------------------------------------

/**
 * The width of a panel after its edge is dragged `delta` pixels to the right. A panel on the left of the divider grows with the
 * pointer; one on the right shrinks with it. Held between `min` and `max`.
 */
export function dragWidth(start: number, delta: number, side: "left" | "right", min: number, max: number): number {
  const w = start + (side === "left" ? delta : -delta);
  return Math.round(Math.min(max, Math.max(min, w)));
}

/** Which panels a window of this width has room for: the inspector goes first, then the explorer. */
export function layoutFor(width: number, prefs: { explorerOpen: boolean; inspectorOpen: boolean }): { explorer: boolean; inspector: boolean } {
  return { explorer: prefs.explorerOpen && width >= 560, inspector: prefs.inspectorOpen && width >= 900 };
}
