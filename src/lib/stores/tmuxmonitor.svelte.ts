// The Tmux Monitor's state: which hosts are watched, what their tmux servers look like, what the person selected, previews of
// panes, searches and tags.
//
// Remote tmux state (`snapshots`) is kept apart from terminal state (the tabs in `ui`): a snapshot comes only from the host's
// tmux server, through the backend's shared engine, and a terminal tab is never part of it. The engine runs ONE poll loop per
// host and socket however many views watch it; this store only says who is watching (`watch`/`unwatch`) and listens for the
// changes it pushes. A hidden monitor watches nothing, and a locked vault forgets everything, pane text included.
//
// Pane text is held in memory only: never written to storage, never sent anywhere but shown.
import * as api from "$lib/api";
import { ask, askText } from "$lib/dialogs.svelte";
import { ssh } from "$lib/ssh";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type AutoTmux, type Uuid } from "$lib/types";
import { type ActionId } from "$lib/tmux/actions";
import {
  DEFAULT_SOCKET, PREVIEW_DEFAULTS, accept, buildRows, clampPrefs, closedKey, diffEvents, errorNotice, locate, nodeKey, panesOf, previewsDue, pushHistory, sameContent, serverKey, socketKey, summarize,
  type HostEntry, type Pane, type Summary, type TmuxEvent, type Window, type HostSnapshot, type Notice, type Op, type OpResult, type PaneCapture, type PaneHits, type PreviewPrefs, type Resources, type Row, type SearchReport, type Session, type Socket,
} from "$lib/tmux/monitor";

const KEY = "sshvault.tmuxmonitor.v1";

export type PreviewMode = "list" | "grid";
/** What the main area shows for the selection. */
export type WorkTab = "grid" | "list" | "windows" | "info" | "resources";
export type BottomTab = "output" | "search" | "events" | "activity";

export interface BottomPrefs {
  open: boolean;
  height: number;
  tab: BottomTab;
  /** Lines of the focused pane's output shown in the Output tab. */
  lines: number;
}

/** One thing the person did here and how it went. Kept in memory for this run only. */
export interface Activity {
  at: number;
  ok: boolean;
  text: string;
  /** The error, when it failed. */
  detail?: string;
}

export interface PanelPrefs {
  /** Show the tmux panel beside SSH terminals at all. */
  enabled: boolean;
  /** Open it by itself when a terminal opens on a host that has tmux sessions. */
  autoOpen: boolean;
  side: "left" | "right";
  width: number;
  /** Hosts the panel is turned off for. */
  offFor: Uuid[];
}

export interface Prefs {
  /** Hosts in the explorer, in order. */
  hostIds: Uuid[];
  /** Other sockets to read on a host, besides the default one. */
  sockets: Record<Uuid, Socket[]>;
  /** The explorer was filled once from the open terminals; after that it is the person's list. */
  seeded: boolean;
  mode: PreviewMode;
  ansi: boolean;
  /** Lines of history above the screen in the preview. */
  scrollback: number;
  discoverMs: number;
  preview: PreviewPrefs;
  /** Lines searched above each pane's screen. */
  searchScrollback: number;
  explorerWidth: number;
  inspectorWidth: number;
  explorerOpen: boolean;
  inspectorOpen: boolean;
  /** Session labels, by `server|session name`. */
  tags: Record<string, string[]>;
  panel: PanelPrefs;
  /** The area below the previews (Output, Search, Events, Activity). */
  bottom: BottomPrefs;
  /** Live refresh is paused when false; the Refresh button still reads on request. */
  live: boolean;
}

const DEFAULTS: Prefs = {
  hostIds: [],
  sockets: {},
  seeded: false,
  mode: "list",
  ansi: true,
  scrollback: 200,
  discoverMs: 4000,
  preview: PREVIEW_DEFAULTS,
  searchScrollback: 1000,
  explorerWidth: 300,
  inspectorWidth: 300,
  explorerOpen: true,
  inspectorOpen: true,
  tags: {},
  panel: { enabled: true, autoOpen: false, side: "right", width: 280, offFor: [] },
  bottom: { open: true, height: 220, tab: "output", lines: 100 },
  live: true,
};

function load(): Prefs {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Prefs> | null;
    if (!raw) return structuredClone(DEFAULTS);
    return {
      ...DEFAULTS,
      ...raw,
      preview: clampPrefs({ ...PREVIEW_DEFAULTS, ...(raw.preview ?? {}) }),
      panel: { ...DEFAULTS.panel, ...(raw.panel ?? {}) },
      bottom: (() => {
        const b = { ...DEFAULTS.bottom, ...(raw.bottom ?? {}) };
        return { ...b, height: Math.min(600, Math.max(120, Number(b.height) || 220)), lines: [50, 100, 200, 500].includes(Number(b.lines)) ? Number(b.lines) : 100 };
      })(),
      live: raw.live !== false,
      scrollback: Math.min(5000, Math.max(0, Number(raw.scrollback ?? DEFAULTS.scrollback))),
      searchScrollback: Math.min(5000, Math.max(0, Number(raw.searchScrollback ?? DEFAULTS.searchScrollback))),
      discoverMs: Math.min(60000, Math.max(3000, Number(raw.discoverMs ?? DEFAULTS.discoverMs))),
    };
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export interface Preview {
  capture?: PaneCapture;
  error?: string;
  /** When this window last asked (ms), whether or not it worked. */
  at: number;
  loading: boolean;
}

/** A search's answer across every server searched: each pane's hits say which server the pane is on (ids repeat between servers). */
export interface SearchSummary {
  results: (PaneHits & { server: string })[];
  failed: [string, string][];
  searched: number;
  limited: boolean;
  cancelled: boolean;
}

export const paneKey = (server: string, paneId: string): string => `${server}#${paneId}`;

/** A pane in the terminal that was started by the monitor carries its marker in the command it was given. */
export function markerOf(command: string | undefined): string | null {
  // The backend quotes the marker, and then quotes the whole command for `sh -c`, so a quote around it comes out as `'\''`.
  const m = command?.match(/SSHVAULT_TAB=(?:'\\''|')([A-Za-z0-9-]{1,64})(?:'\\''|')/);
  return m ? m[1] : null;
}

/** An operation in words, for the activity list when the caller gave none. */
function describeOp(op: Op): string {
  const t = op.op.replace(/_/g, " ");
  return t.charAt(0).toUpperCase() + t.slice(1);
}

const newMarker = (): string => (typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `m${Date.now()}${Math.floor(Math.random() * 1e9)}`);

class TmuxMonitor {
  prefs = $state<Prefs>(load());
  /** The newest reading of each tmux server, by `serverKey`. */
  snapshots = $state<Record<string, HostSnapshot>>({});
  /** The last request that failed outright (never read at all), by server. */
  failures = $state<Record<string, Notice>>({});
  loading = $state<Record<string, boolean>>({});
  /** Open nodes; a closed host is `closed:<host node>`. */
  expanded = $state<Record<string, true>>({});
  selected = $state<string | null>(null);
  query = $state("");
  previews = $state<Record<string, Preview>>({});
  resources = $state<Record<string, Resources>>({});
  resourceError = $state<Record<string, string>>({});
  /** Panes the monitor is showing now (selected, gallery), so only they are captured. */
  visiblePanes = $state<string[]>([]);
  /** The pane the preview is centred on (may be the active pane of a selected session), captured at the selected-pane rate. */
  focusKey = $state<string | null>(null);
  /** The tmux panel beside the active terminal is open. */
  panelOpen = $state(false);
  /** Whether the monitor page is on screen. A hidden monitor watches and captures nothing. */
  visible = $state(false);
  /** The session each terminal marker is attached to, as far as this app has put it there. */
  attached = $state<Record<string, { server: string; sessionId: string }>>({});
  /** The marker of the tmux client Auto tmux started in a terminal pane, by pane. */
  autoMarker = $state<Record<string, string>>({});

  /** What changed between readings, newest first. Kept in memory, this run only. */
  events = $state<TmuxEvent[]>([]);
  /** What was done from here and how it went, newest first. */
  activity = $state<Activity[]>([]);
  /** Which tab of the main area is on, per selection kind it makes sense for. */
  tab = $state<WorkTab>(this.prefs.mode);
  /** The pane the Output tab is reading (set by that tab while it is on screen), captured at the selected-pane rate. */
  outputKey = $state<string | null>(null);
  /** CPU % and resident bytes of each session over the last readings, by `server|session id`, for the small charts. */
  history = $state<Record<string, { cpu: number[]; mem: number[] }>>({});
  /** When any server was last read successfully (ms). */
  lastReadAt = $state(0);

  searchText = $state("");
  searchCase = $state(false);
  searchRegex = $state(false);
  searchScope = $state<"all" | "host" | "session" | "window" | "pane">("all");
  searching = $state(false);
  searchReport = $state<SearchSummary | null>(null);
  searchError = $state<string | null>(null);

  #unlisten: (() => void) | undefined;
  #previewTimer: ReturnType<typeof setInterval> | undefined;
  #resourceTimer: ReturnType<typeof setInterval> | undefined;
  #lastCapture = new Map<string, number>();
  #inflight = new Set<string>();
  #searchSeq = 0;
  #resourceTicks = 0;
  #started = false;

  constructor() {
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
      $effect(() => {
        const snapshot = $state.snapshot(this.prefs);
        try {
          localStorage.setItem(KEY, JSON.stringify(snapshot));
        } catch {
          // Not fatal: the choices just won't be remembered.
        }
      });
    });
  }

  // -- hosts -------------------------------------------------------------------------------------------------------

  /** The servers drawn in the explorer: each watched host with its default socket and any others added. */
  get entries(): HostEntry[] {
    const out: HostEntry[] = [];
    for (const id of this.prefs.hostIds) {
      const host = vaultStore.hostById.get(id)?.data;
      if (!host) continue;
      const name = host.label || host.hostname;
      for (const socket of [DEFAULT_SOCKET, ...(this.prefs.sockets[id] ?? [])]) out.push({ hostId: id, name: socket.kind === "default" ? name : `${name} · ${socketKey(socket)}`, socket });
    }
    return out;
  }

  /** Hosts that could be added: SSH hosts not in the explorer yet. */
  get addable(): { id: Uuid; label: string }[] {
    const have = new Set(this.prefs.hostIds);
    return vaultStore.hosts
      .filter((r) => r.data && !have.has(r.id) && (!r.data.protocol || r.data.protocol === "ssh"))
      .map((r) => ({ id: r.id, label: r.data!.label || r.data!.hostname }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }

  /** Fill an empty explorer from the hosts that have a terminal open, once. */
  seed() {
    if (this.prefs.seeded) return;
    const open = new Set<Uuid>();
    for (const t of ui.tabs) for (const p of t.panes) if (p.target.kind === "host") open.add(p.target.hostId);
    const ids = [...open].filter((id) => {
      const p = vaultStore.hostById.get(id)?.data?.protocol;
      return !p || p === "ssh";
    });
    if (ids.length) this.prefs.hostIds = ids;
    this.prefs.seeded = true;
  }

  addHost(id: Uuid) {
    if (this.prefs.hostIds.includes(id)) return;
    this.prefs.hostIds = [...this.prefs.hostIds, id];
    this.prefs.seeded = true;
    if (this.#started) this.#watchAll();
  }

  removeHost(id: Uuid) {
    this.prefs.hostIds = this.prefs.hostIds.filter((h) => h !== id);
    delete this.prefs.sockets[id];
    for (const k of Object.keys(this.snapshots)) if (k.startsWith(`${id}|`)) delete this.snapshots[k];
    void api.tmuxMonitor.forget(id).catch(() => {});
    if (this.#started) this.#watchAll();
  }

  addSocket(hostId: Uuid, socket: Socket) {
    const have = this.prefs.sockets[hostId] ?? [];
    if (socket.kind === "default" || have.some((s) => socketKey(s) === socketKey(socket))) return;
    this.prefs.sockets[hostId] = [...have, socket];
    if (this.#started) this.#watchAll();
  }

  removeSocket(hostId: Uuid, socket: Socket) {
    this.prefs.sockets[hostId] = (this.prefs.sockets[hostId] ?? []).filter((s) => socketKey(s) !== socketKey(socket));
    delete this.snapshots[serverKey(hostId, socket)];
    if (this.#started) this.#watchAll();
  }

  // -- lifecycle ---------------------------------------------------------------------------------------------------

  /** Listen for the changes the engine pushes (once, for every view). */
  async listen() {
    if (this.#unlisten) return;
    try {
      this.#unlisten = await api.tmuxMonitor.onSnapshot((s) => this.ingest(s));
    } catch {
      // Outside the app (tests, the browser preview) there is no event channel; requests still work.
    }
  }

  /**
   * Keep one host's list current for the panel beside a terminal. The engine runs a single loop per host however many views
   * watch it; the returned function ends this view's share of it.
   */
  watchPanel(paneId: string, hostId: Uuid, socket: Socket = DEFAULT_SOCKET): () => void {
    const subscriber = `panel:${paneId}`;
    void this.listen();
    void api.tmuxMonitor.watch(hostId, socket, subscriber, this.prefs.discoverMs).catch((e) => this.#fail(serverKey(hostId, socket), e));
    void this.refresh(hostId, socket, false);
    return () => void api.tmuxMonitor.unwatch(subscriber).catch(() => {});
  }

  /** The monitor page opened: listen for pushed changes and watch the hosts. */
  async start() {
    this.visible = true;
    this.seed();
    await this.listen();
    this.#started = true;
    if (this.prefs.live) this.#watchAll();
    for (const e of this.entries) void this.refresh(e.hostId, e.socket, false);
    this.#previewTimer ??= setInterval(() => void this.#tickPreviews(), 1000);
    this.#resourceTimer ??= setInterval(() => void this.#tickResources(), 5000);
  }

  /** The monitor page closed or was hidden: stop watching and capturing. Nothing is lost; the next `start` resumes. */
  stop() {
    this.visible = false;
    this.#started = false;
    void api.tmuxMonitor.unwatch("monitor").catch(() => {});
    clearInterval(this.#previewTimer);
    clearInterval(this.#resourceTimer);
    this.#previewTimer = undefined;
    this.#resourceTimer = undefined;
    this.cancelSearch();
  }

  /** The vault locked: forget everything read from hosts. */
  reset() {
    this.stop();
    this.#unlisten?.();
    this.#unlisten = undefined;
    this.snapshots = {};
    this.failures = {};
    this.loading = {};
    this.previews = {};
    this.resources = {};
    this.resourceError = {};
    this.searchReport = null;
    this.attached = {};
    this.autoMarker = {};
    this.events = [];
    this.activity = [];
    this.history = {};
    this.selected = null;
    this.#lastCapture.clear();
  }

  #watchAll() {
    if (!this.prefs.live) return;
    // One subscriber name for the whole page: dropping it stops every loop that nobody else watches.
    void api.tmuxMonitor.unwatch("monitor").catch(() => {});
    for (const e of this.entries) void api.tmuxMonitor.watch(e.hostId, e.socket, "monitor", this.prefs.discoverMs).catch((err) => this.#fail(serverKey(e.hostId, e.socket), err));
  }

  // -- readings ----------------------------------------------------------------------------------------------------

  /** Take a reading into the store if it is newer than what is there. */
  ingest(s: HostSnapshot) {
    const key = serverKey(s.host_id, s.socket);
    const current = this.snapshots[key];
    const next = accept(current, s);
    if (next === current) return;
    const changes = diffEvents(current, next, Date.now());
    if (changes.length) this.events = [...changes.reverse(), ...this.events].slice(0, 200);
    if (next.state.kind === "ready" && !next.stale) this.lastReadAt = next.taken_at_ms;
    // Same content, only newer: keep the old object so nothing redraws.
    this.snapshots[key] = current && sameContent(current, next) ? { ...current, generation: next.generation, taken_at_ms: next.taken_at_ms } : next;
    delete this.failures[key];
    this.#forgetGone(key);
  }

  #fail(server: string, e: unknown) {
    const code = isApiError(e) ? e.code : undefined;
    // With an earlier reading on screen, the engine already marked it stale; a notice replaces only an empty tree.
    if (!this.snapshots[server]) this.failures[server] = errorNotice(code, errorMessage(e));
  }

  async refresh(hostId: Uuid, socket: Socket, force = true) {
    const key = serverKey(hostId, socket);
    this.loading[key] = true;
    try {
      this.ingest(await (force ? api.tmuxMonitor.refresh(hostId, socket) : api.tmuxMonitor.snapshot(hostId, socket, 2000)));
    } catch (e) {
      this.#fail(key, e);
    } finally {
      this.loading[key] = false;
    }
  }

  refreshAll() {
    for (const e of this.entries) void this.refresh(e.hostId, e.socket);
  }

  /** Drop previews and selections of things that no longer exist on the server. */
  #forgetGone(server: string) {
    const snap = this.snapshots[server];
    const live = new Set<string>();
    for (const s of snap?.sessions ?? []) for (const { pane } of panesOf(s)) live.add(paneKey(server, pane.id));
    for (const k of Object.keys(this.previews)) if (k.startsWith(`${server}#`) && !live.has(k)) delete this.previews[k];
  }

  // -- the tree ----------------------------------------------------------------------------------------------------

  tagsFor = (server: string, name: string): string[] => this.prefs.tags[`${server}|${name}`] ?? [];

  get rows(): Row[] {
    const map = new Map(Object.entries(this.snapshots));
    return buildRows(this.entries, map, new Set(Object.keys(this.expanded)), this.query, this.tagsFor);
  }

  /**
   * What the main area is about, from the selection: a pane as selected, else the window selected (or the session's active one),
   * and the pane that is active in it. Null while nothing is selected or what was selected is gone.
   */
  get focusTarget(): { session: Session; window: Window; pane: Pane | undefined } | null {
    const sel = this.selectedRow;
    if (!sel || sel.kind === "host" || sel.kind === "notice") return null;
    const here = locate(this.snapshots[sel.server], sel);
    if (!here.session) return null;
    const window = here.window ?? here.session.windows.find((w) => w.active) ?? here.session.windows[0];
    if (!window) return null;
    const pane = here.pane ?? window.panes.find((p) => p.active) ?? window.panes[0];
    return { session: here.session, window, pane };
  }

  /** Select a window (opening its session) without picking one of its panes. */
  revealWindow(server: string, sessionId: string, windowId: string) {
    delete this.expanded[closedKey(nodeKey.host(server))];
    this.expanded[nodeKey.session(server, sessionId)] = true;
    this.selected = nodeKey.window(server, windowId);
  }

  /** The totals over the page: counts from the readings, CPU and memory only from what was measured. */
  get summary(): Summary {
    return summarize(this.entries, this.snapshots, this.resources);
  }

  /** Pause or resume the live refresh. Pausing stops the host loops and the preview timers; Refresh still reads on request. */
  setLive(on: boolean) {
    this.prefs.live = on;
    if (!this.#started) return;
    if (on) {
      this.#watchAll();
      this.refreshAll();
    } else void api.tmuxMonitor.unwatch("monitor").catch(() => {});
  }

  /** Change how often the session list is read, and restart the loops with it. */
  setDiscoverSeconds(sec: number) {
    this.prefs.discoverMs = Math.min(60, Math.max(3, Math.round(sec))) * 1000;
    if (this.#started && this.prefs.live) this.#watchAll();
  }

  get selectedRow(): Row | undefined {
    return this.selected ? this.rows.find((r) => r.key === this.selected) : undefined;
  }

  snapshotOf(server: string): HostSnapshot | undefined {
    return this.snapshots[server];
  }

  toggle(row: Row) {
    if (!row.expandable) return;
    const key = row.kind === "host" ? closedKey(row.key) : row.key;
    if (row.kind === "host") {
      if (this.expanded[key]) delete this.expanded[key];
      else this.expanded[key] = true;
    } else if (this.expanded[row.key]) delete this.expanded[row.key];
    else this.expanded[row.key] = true;
  }

  setOpen(row: Row, open: boolean) {
    if (!row.expandable || row.expanded === open) return;
    this.toggle(row);
  }

  select(key: string | null) {
    this.selected = key;
  }

  /** Select what a search hit points at and open the path to it. */
  reveal(server: string, sessionId: string, windowId: string, paneId: string) {
    delete this.expanded[closedKey(nodeKey.host(server))];
    this.expanded[nodeKey.session(server, sessionId)] = true;
    this.expanded[nodeKey.window(server, windowId)] = true;
    this.selected = nodeKey.pane(server, paneId);
  }

  // -- tags --------------------------------------------------------------------------------------------------------

  setTags(server: string, sessionName: string, tags: string[]) {
    const clean = [...new Set(tags.map((t) => t.trim()).filter(Boolean))].slice(0, 8);
    const key = `${server}|${sessionName}`;
    if (clean.length) this.prefs.tags[key] = clean;
    else delete this.prefs.tags[key];
  }

  // -- previews ----------------------------------------------------------------------------------------------------

  /** Panes drawn right now (selected pane, gallery tiles). Everything else is left alone. */
  setVisiblePanes(keys: string[]) {
    if (keys.length === this.visiblePanes.length && keys.every((k, i) => k === this.visiblePanes[i])) return;
    this.visiblePanes = keys;
  }

  #resolve(key: string): { server: string; hostId: Uuid; socket: Socket; paneId: string } | null {
    const at = key.lastIndexOf("#");
    const server = key.slice(0, at);
    const snap = this.snapshots[server];
    if (!snap) return null;
    return { server, hostId: snap.host_id, socket: snap.socket, paneId: key.slice(at + 1) };
  }

  async capture(key: string, opts: { scrollback?: number } = {}) {
    const r = this.#resolve(key);
    if (!r || this.#inflight.has(key)) return;
    this.#inflight.add(key);
    this.#lastCapture.set(key, Date.now());
    this.previews[key] = { ...(this.previews[key] ?? { at: 0 }), loading: true, at: Date.now() };
    try {
      const wanted = key === this.outputKey && this.prefs.bottom.open ? this.prefs.bottom.lines : 0;
      const capture = await api.tmuxMonitor.capture(r.hostId, r.socket, r.paneId, this.prefs.ansi, opts.scrollback ?? Math.max(this.prefs.scrollback, wanted));
      // The pane may have been selected away from while this ran; the text is still right for its own key.
      this.previews[key] = { capture, at: Date.now(), loading: false };
    } catch (e) {
      const code = isApiError(e) ? e.code : "";
      this.previews[key] = { ...(this.previews[key] ?? { at: 0 }), error: code === "tmux_server_changed" ? "The tmux server restarted. Refresh the list." : code === "tmux_rejected" ? "Pane output could not be captured." : errorMessage(e), at: Date.now(), loading: false };
    } finally {
      this.#inflight.delete(key);
    }
  }

  async #tickPreviews() {
    if (!this.visible || !this.prefs.live || (typeof document !== "undefined" && document.hidden)) return;
    const sel = this.selectedRow;
    const selected = this.focusKey ?? (sel?.paneId ? paneKey(sel.server, sel.paneId) : null);
    const shown = this.outputKey && !this.visiblePanes.includes(this.outputKey) ? [...this.visiblePanes, this.outputKey] : this.visiblePanes;
    const due = previewsDue(Date.now(), selected ?? this.outputKey, shown, this.#lastCapture, this.prefs.preview, this.visible);
    // A few at a time; the engine bounds what reaches the hosts.
    for (const key of due.slice(0, 3)) void this.capture(key);
  }

  // -- resources ---------------------------------------------------------------------------------------------------

  async loadResources(server: string) {
    const snap = this.snapshots[server];
    if (!snap || snap.state.kind !== "ready") return;
    try {
      const res = await api.tmuxMonitor.resources(snap.host_id, snap.socket);
      this.resources[server] = res;
      delete this.resourceError[server];
      // A reading per session for the small charts; a session with nothing measured adds nothing rather than a zero.
      for (const su of res.sessions) {
        if (!su.usage) continue;
        const k = `${server}|${su.session_id}`;
        const h = this.history[k] ?? { cpu: [], mem: [] };
        this.history[k] = { cpu: su.usage.cpu_pct === null ? h.cpu : pushHistory(h.cpu, su.usage.cpu_pct), mem: pushHistory(h.mem, su.usage.rss) };
      }
    } catch (e) {
      this.resourceError[server] = errorMessage(e);
    }
  }

  async #tickResources() {
    if (!this.visible || !this.prefs.live || (typeof document !== "undefined" && document.hidden)) return;
    this.#resourceTicks++;
    const selected = this.selectedRow?.server;
    if (selected) await this.loadResources(selected);
    // The totals over the page cover every server, so the others are read too, less often.
    if (this.#resourceTicks % 3 === 0) for (const e of this.entries) if (serverKey(e.hostId, e.socket) !== selected) void this.loadResources(serverKey(e.hostId, e.socket));
  }

  // -- changes -----------------------------------------------------------------------------------------------------

  /** Run an operation. The answer is a reading taken after it that shows the change; anything else is reported as a failure. */
  async run(hostId: Uuid, socket: Socket, op: Op, done?: string): Promise<OpResult | null> {
    try {
      const r = await api.tmuxMonitor.operate(hostId, socket, op);
      this.ingest(r.snapshot);
      if (done) ui.notify("info", done);
      this.record(true, `${this.#hostLabel(hostId)}: ${done ?? describeOp(op)}`);
      return r;
    } catch (e) {
      const code = isApiError(e) ? e.code : "";
      this.record(false, `${this.#hostLabel(hostId)}: ${describeOp(op)} failed`, errorMessage(e));
      ui.notify(code === "tmux_server_changed" ? "warning" : "error", errorMessage(e));
      // The engine refreshed after a refusal; mirror that for a connection that failed outright.
      if (code && code !== "validation") void this.refresh(hostId, socket);
      return null;
    }
  }

  /** Note something done from here, newest first, capped. */
  record(ok: boolean, text: string, detail?: string) {
    this.activity = [{ at: Date.now(), ok, text, detail }, ...this.activity].slice(0, 100);
  }

  #hostLabel(hostId: Uuid): string {
    const d = vaultStore.hostById.get(hostId)?.data;
    return d?.label || d?.hostname || "host";
  }

  async newSession(hostId: Uuid, socket: Socket, seedName = "") {
    const name = await askText("Name for the new tmux session", seedName, { title: "New session", confirm: "Create", placeholder: "backend" });
    if (name === null || !name.trim()) return null;
    const cwd = await askText("Starting folder (absolute path, leave empty for the home folder)", "", { title: "New session", confirm: "Create" });
    if (cwd === null) return null;
    const r = await this.run(hostId, socket, { op: "new_session", name: name.trim(), cwd: cwd.trim() || null, command: null }, `Created session ${name.trim()}.`);
    if (r?.created) {
      const server = serverKey(hostId, socket);
      this.expanded[nodeKey.session(server, r.created)] = true;
      this.selected = nodeKey.session(server, r.created);
    }
    return r;
  }

  async renameSession(server: string, session: Session) {
    const snap = this.snapshots[server];
    if (!snap) return;
    const name = await askText("New name for the session", session.name, { title: "Rename session", confirm: "Rename" });
    if (name === null || !name.trim() || name === session.name) return;
    const tags = this.tagsFor(server, session.name);
    const r = await this.run(snap.host_id, snap.socket, { op: "rename_session", session: session.id, name: name.trim() }, `Renamed to ${name.trim()}.`);
    // The tags go with the session to its new name.
    if (r && tags.length) {
      this.setTags(server, session.name, []);
      this.setTags(server, name.trim(), tags);
    }
  }

  async killSession(server: string, session: Session) {
    const snap = this.snapshots[server];
    if (!snap) return;
    const panes = session.windows.reduce((n, w) => n + w.panes.length, 0);
    const ok = await ask(
      `Kill session "${session.name}" on ${vaultStore.hostById.get(snap.host_id)?.data?.label ?? "this host"}?\n\nIt has ${session.windows.length} ${session.windows.length === 1 ? "window" : "windows"} and ${panes} ${panes === 1 ? "pane" : "panes"}. Programs running in them may be terminated, and anything unsaved in them is lost. This cannot be undone.`,
      { title: "Kill session", confirm: "Kill session", danger: true },
    );
    if (ok) await this.run(snap.host_id, snap.socket, { op: "kill_session", session: session.id }, `Killed ${session.name}.`);
  }

  async newWindow(server: string, session: Session) {
    const snap = this.snapshots[server];
    if (!snap) return;
    const name = await askText("Name for the new window (optional)", "", { title: "New window", confirm: "Create" });
    if (name === null) return;
    const r = await this.run(snap.host_id, snap.socket, { op: "new_window", session: session.id, name: name.trim() || null, cwd: null, command: null }, "Created the window.");
    if (r?.created) {
      this.expanded[nodeKey.session(server, session.id)] = true;
      this.selected = nodeKey.window(server, r.created);
    }
  }

  async renameWindow(server: string, windowId: string, current: string) {
    const snap = this.snapshots[server];
    if (!snap) return;
    const name = await askText("New name for the window", current, { title: "Rename window", confirm: "Rename" });
    if (name === null || !name.trim() || name === current) return;
    await this.run(snap.host_id, snap.socket, { op: "rename_window", window: windowId, name: name.trim() }, `Renamed to ${name.trim()}.`);
  }

  async killWindow(server: string, windowId: string) {
    const snap = this.snapshots[server];
    const located = snap ? snap.sessions.flatMap((s) => s.windows).find((w) => w.id === windowId) : undefined;
    if (!snap || !located) return;
    const busy = located.panes.filter((p) => !p.dead && !/^(bash|zsh|fish|sh|dash|ksh|tcsh|csh)$/.test(p.command));
    const ok = await ask(
      `Kill window "${located.name}" with ${located.panes.length} ${located.panes.length === 1 ? "pane" : "panes"}?${busy.length ? `\n\nRunning in it: ${[...new Set(busy.map((p) => p.command))].join(", ")}. These will be terminated.` : ""}`,
      { title: "Kill window", confirm: "Kill window", danger: true },
    );
    if (ok) await this.run(snap.host_id, snap.socket, { op: "kill_window", window: windowId }, "Killed the window.");
  }

  async killPane(server: string, paneId: string) {
    const snap = this.snapshots[server];
    const pane = snap?.sessions.flatMap((s) => s.windows.flatMap((w) => w.panes)).find((p) => p.id === paneId);
    if (!snap || !pane) return;
    const running = !pane.dead && !/^(bash|zsh|fish|sh|dash|ksh|tcsh|csh)$/.test(pane.command);
    const ok = await ask(`Kill pane ${paneId}${running ? ` (running ${pane.command})` : ""}? Anything running in it will be terminated.`, { title: "Kill pane", confirm: "Kill pane", danger: true });
    if (ok) await this.run(snap.host_id, snap.socket, { op: "kill_pane", pane: paneId }, "Killed the pane.");
  }

  // -- attaching ---------------------------------------------------------------------------------------------------

  /** The marker of the tmux client a terminal pane started: from the monitor's attach command, or from Auto tmux. */
  markerForPane(paneId: string): string | null {
    for (const t of ui.tabs) {
      const p = t.panes.find((x) => x.id === paneId);
      if (p?.target.kind === "host") return markerOf(p.target.command) ?? this.autoMarker[paneId] ?? null;
    }
    return null;
  }

  /** The session a terminal pane's tmux client was last put on by this app, if it was started by it. */
  attachmentOf(paneId: string): { server: string; sessionId: string; marker: string } | null {
    const marker = this.markerForPane(paneId);
    const a = marker ? this.attached[marker] : undefined;
    return marker && a ? { ...a, marker } : null;
  }

  /**
   * Auto tmux: the command to type into a shell that just connected, or null when the host has it off. The shell is checked
   * for being inside tmux already, and for tmux being installed, on the host (see `tmux::script::auto_command`).
   */
  async autoCommand(paneId: string, hostId: Uuid, auto: AutoTmux | undefined): Promise<string | null> {
    if (!auto?.enabled) return null;
    const socket: Socket = auto.socket?.trim() ? { kind: "name", value: auto.socket.trim() } : DEFAULT_SOCKET;
    const marker = newMarker();
    const command = await api.tmuxMonitor.autoCommand(socket, auto.session?.trim() || "main", auto.behavior !== "attach_existing", auto.behavior === "always_new", !!auto.mouse, marker);
    this.autoMarker[paneId] = marker;
    this.attached[marker] = { server: serverKey(hostId, socket), sessionId: "" };
    return command;
  }

  /**
   * Attach to a session in a terminal.
   * - `tab`: a new tab; `right`/`down`: split the active tab (up to its pane limit) and attach there;
   * - `replace`: move the active pane's tmux client to the session. If that pane has no client on this server it asks first, because
   *   the command is then typed into whatever the pane's shell is doing.
   */
  async attach(server: string, sessionId: string, mode: "tab" | "right" | "down" | "replace", opts: { detachOthers?: boolean } = {}) {
    const snap = this.snapshots[server];
    const session = snap?.sessions.find((s) => s.id === sessionId);
    if (!snap || !session) {
      ui.notify("error", "This session no longer exists on the remote host.");
      return false;
    }
    const hostLabel = vaultStore.hostById.get(snap.host_id)?.data?.label ?? "host";
    const title = `${hostLabel} · ${session.name}`;
    try {
      if (mode === "replace") return await this.#replace(snap, session, server);
      const marker = newMarker();
      const command = await api.tmuxMonitor.attachCommand(snap.host_id, snap.socket, sessionId, marker, !!opts.detachOthers);
      this.attached[marker] = { server, sessionId };
      if (mode === "tab" || !ui.activeTab) ui.openTerminal(snap.host_id, title, command);
      else if (!ui.splitWith(mode === "right" ? "vertical" : "horizontal", { kind: "host", hostId: snap.host_id, command })) {
        ui.notify("warning", "This tab is full. Opened the session in a new tab instead.");
        ui.openTerminal(snap.host_id, title, command);
      }
      this.record(true, `Attached ${title}${mode === "tab" ? " in a new tab" : mode === "right" ? " in a split to the right" : " in a split below"}`);
      return true;
    } catch (e) {
      ui.notify("error", `Unable to attach to the selected tmux session. ${errorMessage(e)}`);
      this.record(false, `Attach to ${title} failed`, errorMessage(e));
      return false;
    }
  }

  async #replace(snap: HostSnapshot, session: Session, server: string): Promise<boolean> {
    const tab = ui.activeTab;
    const pane = tab?.panes.find((p) => p.id === tab.activePaneId);
    if (!pane || pane.target.kind !== "host" || pane.target.hostId !== snap.host_id) {
      ui.notify("warning", "The active terminal is not on this host. Open the session in a new tab instead.");
      return false;
    }
    const marker = this.markerForPane(pane.id);
    if (marker) {
      const moved = await api.tmuxMonitor.switchClient(snap.host_id, snap.socket, marker, session.id);
      if (moved) {
        this.attached[marker] = { server, sessionId: session.id };
        return true;
      }
    }
    if (!(await ask(`This terminal is not attached to tmux on this host. Attach "${session.name}" here by typing the command into its shell?\n\nAnything in the foreground of that terminal will receive the text.`, { title: "Replace terminal", confirm: "Attach here" }))) return false;
    const m = newMarker();
    const command = await api.tmuxMonitor.attachCommand(snap.host_id, snap.socket, session.id, m, false);
    this.attached[m] = { server, sessionId: session.id };
    await ssh.write(pane.id, command + "\r");
    return true;
  }

  // -- one entry point for everything a menu can do ----------------------------------------------------------------

  /** True when a terminal tab with room is open, so a session can be opened beside it. */
  get canSplit(): boolean {
    return !!ui.activeTab && ui.activeTab.panes.length < 6;
  }

  canReplaceOn(hostId: Uuid): boolean {
    const tab = ui.activeTab;
    const pane = tab?.panes.find((p) => p.id === tab.activePaneId);
    return !!pane && pane.target.kind === "host" && pane.target.hostId === hostId;
  }

  async act(id: ActionId, row: Pick<Row, "kind" | "server" | "hostId" | "socket" | "sessionId" | "windowId" | "paneId">) {
    const snap = this.snapshots[row.server];
    const here = locate(snap, row);
    const run = (op: Op, done?: string) => this.run(row.hostId, row.socket, op, done);
    switch (id) {
      case "refresh": return void (await this.refresh(row.hostId, row.socket));
      case "new-session": return void (await this.newSession(row.hostId, row.socket));
      case "add-socket": {
        const name = await askText("Name of the tmux socket (the -L name; sockets are looked for in the same folder as the default one)", "", { title: "Add a tmux socket", confirm: "Add", placeholder: "work" });
        if (name?.trim()) this.addSocket(row.hostId, { kind: "name", value: name.trim() });
        return;
      }
      case "remove-host": return this.removeHost(row.hostId);
      case "attach-tab": return void (row.sessionId && (await this.attach(row.server, row.sessionId, "tab")));
      case "attach-right": return void (row.sessionId && (await this.attach(row.server, row.sessionId, "right")));
      case "attach-down": return void (row.sessionId && (await this.attach(row.server, row.sessionId, "down")));
      case "attach-replace": return void (row.sessionId && (await this.attach(row.server, row.sessionId, "replace")));
      case "rename-session": return void (here.session && (await this.renameSession(row.server, here.session)));
      case "new-window": return void (here.session && (await this.newWindow(row.server, here.session)));
      case "tags": {
        if (!here.session) return;
        const text = await askText("Tags for this session, separated by commas (for example: Production, Database)", this.tagsFor(row.server, here.session.name).join(", "), { title: "Session tags", confirm: "Save" });
        if (text !== null) this.setTags(row.server, here.session.name, text.split(","));
        return;
      }
      case "mouse-on":
      case "mouse-off": {
        if (!here.session) return;
        const on = id === "mouse-on";
        // The setting belongs to this session only; the server's other sessions, key bindings and config file are not touched.
        const ok = await ask(
          on
            ? `Turn tmux mouse mode on for "${here.session.name}"?\n\nOnly this session changes (its "mouse" option); other sessions, key bindings and the tmux config file are left alone. While it is on, tmux handles the mouse inside the session: select text with Shift held down, and use tmux's copy mode for scrolling. Turn it off here at any time.`
            : `Turn tmux mouse mode off for "${here.session.name}"? Only this session changes.`,
          { title: "tmux mouse mode", confirm: on ? "Turn on" : "Turn off" },
        );
        if (ok) await run({ op: "set_mouse", session: here.session.id, on }, `Mouse mode ${on ? "on" : "off"} for ${here.session.name}.`);
        return;
      }
      case "kill-session": return void (here.session && (await this.killSession(row.server, here.session)));
      case "select-window": return void (row.windowId && (await run({ op: "select_window", window: row.windowId })));
      case "rename-window": return void (here.window && row.windowId && (await this.renameWindow(row.server, row.windowId, here.window.name)));
      case "kill-window": return void (row.windowId && (await this.killWindow(row.server, row.windowId)));
      case "select-pane": return void (row.paneId && (await run({ op: "select_pane", pane: row.paneId })));
      case "split-right": return void (row.paneId && (await run({ op: "split_pane", pane: row.paneId, dir: "right", cwd: null, command: null }, "Split the pane to the right.")));
      case "split-down": return void (row.paneId && (await run({ op: "split_pane", pane: row.paneId, dir: "down", cwd: null, command: null }, "Split the pane downward.")));
      case "zoom-pane": return void (row.paneId && (await run({ op: "zoom_pane", pane: row.paneId })));
      case "title-pane": {
        if (!here.pane) return;
        const t = await askText("Title for the pane", here.pane.title, { title: "Pane title", confirm: "Set" });
        if (t !== null && row.paneId) await run({ op: "set_pane_title", pane: row.paneId, title: t });
        return;
      }
      case "copy-pane": {
        if (!row.paneId) return;
        const text = this.previews[paneKey(row.server, row.paneId)]?.capture?.text;
        if (text === undefined) return void ui.notify("warning", "There is no captured text for this pane yet.");
        try {
          await navigator.clipboard.writeText(text.replace(/\x1b\[[0-9;:]*m/g, ""));
          ui.notify("info", "Copied the visible text.");
        } catch {
          ui.notify("error", "The clipboard is not available.");
        }
        return;
      }
      case "kill-pane": return void (row.paneId && (await this.killPane(row.server, row.paneId)));
    }
  }

  // -- search ------------------------------------------------------------------------------------------------------

  /** The panes a search of the given scope covers, from the current selection. */
  searchTargets(): { server: string; hostId: Uuid; socket: Socket; targets: { session_id: string; window_id: string; pane_id: string }[] }[] {
    const sel = this.selectedRow;
    const out: { server: string; hostId: Uuid; socket: Socket; targets: { session_id: string; window_id: string; pane_id: string }[] }[] = [];
    for (const e of this.entries) {
      const server = serverKey(e.hostId, e.socket);
      const snap = this.snapshots[server];
      if (!snap || snap.state.kind !== "ready") continue;
      if (this.searchScope !== "all" && sel?.server !== server) continue;
      const targets: { session_id: string; window_id: string; pane_id: string }[] = [];
      for (const s of snap.sessions) {
        if ((this.searchScope === "session" || this.searchScope === "window" || this.searchScope === "pane") && sel?.sessionId !== s.id) continue;
        for (const w of s.windows) {
          if ((this.searchScope === "window" || this.searchScope === "pane") && sel?.windowId !== w.id) continue;
          for (const p of w.panes) {
            if (this.searchScope === "pane" && sel?.paneId !== p.id) continue;
            targets.push({ session_id: s.id, window_id: w.id, pane_id: p.id });
          }
        }
      }
      if (targets.length) out.push({ server, hostId: e.hostId, socket: e.socket, targets });
    }
    return out;
  }

  async search() {
    const text = this.searchText;
    if (!text) return;
    this.cancelSearch();
    const seq = ++this.#searchSeq;
    this.searching = true;
    this.searchError = null;
    this.searchReport = null;
    const groups = this.searchTargets();
    const merged: SearchSummary = { results: [], failed: [], searched: 0, limited: false, cancelled: false };
    try {
      const reports = await Promise.all(
        groups.map(async (g) => {
          const r = await api.tmuxMonitor.search(g.hostId, g.socket, `search:${seq}:${g.server}`, g.targets, { text, case_sensitive: this.searchCase, regex: this.searchRegex }, this.prefs.searchScrollback);
          return { g, r };
        }),
      );
      if (seq !== this.#searchSeq) return;
      for (const { g, r } of reports) {
        merged.searched += r.searched;
        merged.limited ||= r.limited;
        merged.cancelled ||= r.cancelled;
        merged.failed.push(...r.failed);
        // A hit is addressed by server as well as pane id: ids repeat across servers.
        for (const hit of r.results) merged.results.push({ ...hit, server: g.server });
      }
      this.searchReport = merged;
    } catch (e) {
      if (seq === this.#searchSeq) this.searchError = errorMessage(e);
    } finally {
      if (seq === this.#searchSeq) this.searching = false;
    }
  }

  cancelSearch() {
    const seq = this.#searchSeq;
    this.#searchSeq++;
    this.searching = false;
    for (const g of this.searchTargets()) void api.tmuxMonitor.searchCancel(`search:${seq}:${g.server}`).catch(() => {});
  }

  clearSearch() {
    this.cancelSearch();
    this.searchReport = null;
    this.searchError = null;
  }

  // -- the panel beside a terminal ----------------------------------------------------------------------------------

  panelEnabledFor(hostId: Uuid): boolean {
    return this.prefs.panel.enabled && !this.prefs.panel.offFor.includes(hostId);
  }

  setPanelHost(hostId: Uuid, on: boolean) {
    const off = new Set(this.prefs.panel.offFor);
    if (on) off.delete(hostId);
    else off.add(hostId);
    this.prefs.panel.offFor = [...off];
  }

  locate(server: string, row: Pick<Row, "sessionId" | "windowId" | "paneId">) {
    return locate(this.snapshots[server], row);
  }
}

export const tmuxMonitor = new TmuxMonitor();
