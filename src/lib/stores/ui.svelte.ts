// Ephemeral UI state: which sidebar view is active, open tabs, modals.
import { MAX_PANES, grid, leaf, paneIds, remove, setRatio, split, type LayoutNode } from "$lib/layout";
import { settings } from "$lib/stores/settings.svelte";
import type { AdhocTarget, SessionStatus } from "$lib/ssh";
import type { Uuid } from "$lib/types";

export type View =
  | "hosts"
  | "favorites"
  | "groups"
  | "keys"
  | "keychain"
  | "forwarding"
  | "snippets"
  | "knownhosts"
  | "sftp"
  | "vault"
  | "settings";

/** Views that fill the window instead of sitting beside the terminals. */
export const PAGE_VIEWS: View[] = ["groups", "keys", "knownhosts", "vault", "settings"];

/** What a pane connects to: a saved host, or an unsaved quick connection. */
export type PaneTarget =
  | { kind: "host"; hostId: Uuid; /** Typed into the shell once connected, after the host's own startup command. */ command?: string }
  | { kind: "adhoc"; adhoc: AdhocTarget }
  | { kind: "local" };

export interface Pane {
  id: string;
  target: PaneTarget;
}

export interface Tab {
  id: string;
  title: string;
  /** Set when the user renames the tab; wins over `title`. */
  customTitle?: string;
  /** Every pane in the tab, flat and stable (see lib/layout.ts). */
  panes: Pane[];
  /** How the panes are arranged. */
  layout: LayoutNode;
  /** Typing in one pane goes to every pane in the tab. */
  syncInput?: boolean;
  activePaneId: string;
  /** One pane shown full size; the others stay connected underneath. */
  zoomedPaneId?: string;
}

/** Live per-pane info, written by TerminalPane and read by tabs and palette. */
export interface PaneInfo {
  status: SessionStatus["kind"];
  /** Window title set by the remote shell (OSC 0/2), if any. */
  remoteTitle?: string;
  /** Path of the session log being written, if recording. */
  recording?: string;
  /** Remote working directory, when the shell reports it (OSC 7). */
  cwd?: string;
  /** The remote program (tmux, vim, htop…) asked for mouse events. */
  mouseTracked?: boolean;
  /** Mouse selects text even while the program wants the mouse. */
  selectMode?: boolean;
}

export type Modal =
  | { kind: "host"; id: Uuid | null; group?: string }
  | { kind: "identity"; id: Uuid | null }
  | { kind: "snippet"; id: Uuid | null }
  | { kind: "forward"; id: Uuid | null }
  | { kind: "quick-connect"; initial?: string }
  | { kind: "import-ssh-config" }
  | { kind: "snippet-vars"; command: string; names: string[]; opts: SnippetRunOpts }
  | { kind: "run-on-hosts"; command?: string }
  | { kind: "save-workspace" }
  | { kind: "shortcuts" }
  | { kind: "bulk-edit" }
  | { kind: "host-details"; id: Uuid }
  | null;

export interface ToastAction {
  label: string;
  run: () => void;
}

/** A tab as saved in a workspace: what each pane connects to, never secrets. */
export interface WorkspaceTab {
  title: string;
  targets: PaneTarget[];
  syncInput?: boolean;
}

export interface SnippetRunOpts {
  execute: boolean;
  scope: "pane" | "tab";
}

let counter = 0;
const nextId = (prefix: string) => `${prefix}-${++counter}-${Date.now().toString(36)}`;

export function adhocLabel(a: Omit<AdhocTarget, "password">) {
  return `${a.username}@${a.hostname}${a.port !== 22 ? `:${a.port}` : ""}`;
}

class UiStore {
  view = $state<View>("hosts");
  tabs = $state<Tab[]>([]);
  activeTabId = $state<string | null>(null);
  modal = $state<Modal>(null);
  paletteOpen = $state(false);
  search = $state("");
  collapsedGroups = $derived(new Set(settings.collapsedGroups));
  paneInfo = $state<Record<string, PaneInfo>>({});
  /** Bumped to ask the active pane to open its find bar. */
  findRequest = $state(0);

  activeTab = $derived(this.tabs.find((t) => t.id === this.activeTabId) ?? null);
  /** Mounted lazily on first visit, then kept alive so sessions survive view switches. */
  sftpVisited = $state(false);
  toast = $state<{ kind: "info" | "error"; text: string; action?: ToastAction } | null>(null);
  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  notify(kind: "info" | "error", text: string, action?: ToastAction, ms = action ? 8000 : 4000) {
    this.toast = { kind, text, action };
    clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => (this.toast = null), ms);
  }

  /** Run the toast's action (e.g. Undo) and dismiss it. */
  runToastAction() {
    const a = this.toast?.action;
    this.toast = null;
    clearTimeout(this.#toastTimer);
    a?.run();
  }

  /** Show one pane full size, or restore the split. */
  toggleZoom(tabId: string, paneId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab || tab.panes.length < 2) return;
    tab.zoomedPaneId = tab.zoomedPaneId === paneId ? undefined : paneId;
    tab.activePaneId = paneId;
  }

  toggleZoomActive() {
    const tab = this.activeTab;
    if (tab) this.toggleZoom(tab.id, tab.activePaneId);
  }

  /** Aggregate connection state for a tab's status dot. */
  tabStatus(tab: Tab): PaneInfo["status"] {
    const kinds = tab.panes.map((p) => this.paneInfo[p.id]?.status ?? "connecting");
    if (kinds.some((k) => k === "error" || k === "host_key_changed")) return "error";
    if (kinds.every((k) => k === "connected")) return "connected";
    if (kinds.some((k) => k === "connecting")) return "connecting";
    return "disconnected";
  }

  #openTab(target: PaneTarget, title: string) {
    const pane: Pane = { id: nextId("pane"), target };
    const tab: Tab = {
      id: nextId("tab"),
      title,
      panes: [pane],
      layout: leaf(pane.id),
      activePaneId: pane.id,
    };
    this.tabs.push(tab);
    this.activeTabId = tab.id;
    // Terminals live in the Hosts/Keychain/... layouts, not SFTP or Settings.
    if (this.view === "sftp" || PAGE_VIEWS.includes(this.view)) this.view = "hosts";
  }

  openTerminal(hostId: Uuid, title: string, command?: string) {
    this.#openTab(command ? { kind: "host", hostId, command } : { kind: "host", hostId }, title);
  }

  /** Hosts ticked in the tree for bulk actions (Ctrl/Shift+click). */
  selectedHosts = $state<Set<Uuid>>(new Set());

  toggleHostSelected(id: Uuid) {
    const next = new Set(this.selectedHosts);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selectedHosts = next;
  }

  /** Ask the SFTP view to connect to a host and show a directory. */
  sftpRequest = $state<{ hostId: Uuid; path: string; n: number } | null>(null);
  openSftpAt(hostId: Uuid, path: string) {
    this.sftpRequest = { hostId, path, n: (this.sftpRequest?.n ?? 0) + 1 };
    this.sftpVisited = true;
    this.view = "sftp";
  }

  openLocal() {
    this.#openTab({ kind: "local" }, "Local");
  }

  /**
   * Open several hosts at once: one tab each, or tiled in a single tab
   * (at most MAX_PANES) with synchronized input, like cluster SSH.
   */
  openMany(hosts: { id: Uuid; label: string }[], mode: "tabs" | "tiled", title = "Cluster") {
    if (mode === "tabs") {
      for (const h of hosts) this.openTerminal(h.id, h.label);
      return;
    }
    const picked = hosts.slice(0, MAX_PANES);
    const panes: Pane[] = picked.map((h) => ({ id: nextId("pane"), target: { kind: "host", hostId: h.id } }));
    const tab: Tab = {
      id: nextId("tab"),
      title,
      panes,
      layout: grid(panes.map((p) => p.id)),
      activePaneId: panes[0].id,
      syncInput: true,
    };
    this.tabs.push(tab);
    this.activeTabId = tab.id;
    if (this.view === "sftp" || PAGE_VIEWS.includes(this.view)) this.view = "hosts";
  }

  /** The open tabs as a workspace. Quick-connect passwords are never saved. */
  snapshotWorkspace(): WorkspaceTab[] {
    return this.tabs.map((t) => ({
      title: t.customTitle ?? t.title,
      syncInput: t.syncInput || undefined,
      targets: t.panes.map((p) => {
        const target = $state.snapshot(p.target) as PaneTarget;
        if (target.kind === "adhoc") {
          const { password: _omit, ...rest } = target.adhoc;
          return { kind: "adhoc", adhoc: rest } as PaneTarget;
        }
        return target;
      }),
    }));
  }

  /** Reopen a saved workspace's tabs next to the ones already open. */
  openWorkspace(tabs: WorkspaceTab[]) {
    for (const wt of tabs) {
      const targets = wt.targets.slice(0, MAX_PANES);
      if (!targets.length) continue;
      const panes: Pane[] = targets.map((target) => ({ id: nextId("pane"), target }));
      const tab: Tab = {
        id: nextId("tab"),
        title: wt.title,
        panes,
        layout: panes.length === 1 ? leaf(panes[0].id) : grid(panes.map((p) => p.id)),
        activePaneId: panes[0].id,
        syncInput: wt.syncInput,
      };
      this.tabs.push(tab);
      this.activeTabId = tab.id;
    }
    if (this.view === "sftp" || PAGE_VIEWS.includes(this.view)) this.view = "hosts";
  }

  openAdhoc(adhoc: AdhocTarget) {
    this.#openTab({ kind: "adhoc", adhoc }, adhocLabel(adhoc));
  }

  closeTab(id: string) {
    const idx = this.tabs.findIndex((t) => t.id === id);
    if (idx < 0) return;
    for (const p of this.tabs[idx].panes) delete this.paneInfo[p.id];
    this.tabs.splice(idx, 1);
    if (this.activeTabId === id) {
      this.activeTabId = this.tabs[Math.min(idx, this.tabs.length - 1)]?.id ?? null;
    }
  }

  closeOtherTabs(keepId: string) {
    for (const t of [...this.tabs]) if (t.id !== keepId) this.closeTab(t.id);
    this.activeTabId = keepId;
  }

  duplicateTab(id: string) {
    const tab = this.tabs.find((t) => t.id === id);
    if (!tab) return;
    const source = tab.panes.find((p) => p.id === tab.activePaneId) ?? tab.panes[0];
    this.#openTab(structuredClone($state.snapshot(source.target)) as PaneTarget, tab.customTitle ?? tab.title);
  }

  renameTab(id: string, title: string) {
    const tab = this.tabs.find((t) => t.id === id);
    if (tab) tab.customTitle = title.trim() || undefined;
  }

  cycleTab(delta: number) {
    if (this.tabs.length < 2) return;
    const idx = this.tabs.findIndex((t) => t.id === this.activeTabId);
    this.activeTabId = this.tabs[(idx + delta + this.tabs.length) % this.tabs.length].id;
  }

  selectTab(index: number) {
    const t = this.tabs[index];
    if (t) this.activeTabId = t.id;
  }

  /**
   * Split the active pane, opening the same target in the new pane.
   * "vertical" puts the new pane to the right, "horizontal" below.
   */
  splitActive(direction: "vertical" | "horizontal") {
    const tab = this.activeTab;
    if (!tab || tab.panes.length >= MAX_PANES) return;
    const source = tab.panes.find((p) => p.id === tab.activePaneId) ?? tab.panes[0];
    const pane: Pane = { id: nextId("pane"), target: structuredClone($state.snapshot(source.target)) as PaneTarget };
    tab.panes.push(pane);
    tab.layout = split($state.snapshot(tab.layout) as LayoutNode, source.id, pane.id, direction === "vertical" ? "row" : "column");
    tab.activePaneId = pane.id;
  }

  closePane(tabId: string, paneId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    delete this.paneInfo[paneId];
    if (tab.zoomedPaneId === paneId) tab.zoomedPaneId = undefined;
    const layout = remove($state.snapshot(tab.layout) as LayoutNode, paneId);
    tab.panes = tab.panes.filter((p) => p.id !== paneId);
    if (!layout || tab.panes.length === 0) return this.closeTab(tabId);
    tab.layout = layout;
    if (tab.activePaneId === paneId) tab.activePaneId = paneIds(layout)[0];
  }

  /** Asks the tab area to toggle synchronized input (it owns the prod check). */
  syncRequest = $state(0);

  resizeSplit(tabId: string, splitId: string, ratio: number) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (tab) tab.layout = setRatio($state.snapshot(tab.layout) as LayoutNode, splitId, ratio);
  }

  /** Move a tab to `toIndex` (drag to reorder). */
  moveTab(tabId: string, toIndex: number) {
    const from = this.tabs.findIndex((t) => t.id === tabId);
    if (from < 0) return;
    const [tab] = this.tabs.splice(from, 1);
    this.tabs.splice(Math.max(0, Math.min(toIndex, this.tabs.length)), 0, tab);
  }

  /** Forget every open tab and transient UI; used when the vault locks. */
  resetSession() {
    this.tabs = [];
    this.activeTabId = null;
    this.paneInfo = {};
    this.modal = null;
    this.paletteOpen = false;
    this.sftpVisited = false;
  }

  toggleGroup(path: string) {
    const next = new Set(settings.collapsedGroups);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    settings.collapsedGroups = [...next];
  }

  collapseGroups(paths: string[]) {
    settings.collapsedGroups = [...new Set([...settings.collapsedGroups, ...paths])];
  }

  expandAllGroups() {
    settings.collapsedGroups = [];
  }

  toggleSidebar() {
    settings.prefs.sidebarHidden = !settings.prefs.sidebarHidden;
  }
}

export const ui = new UiStore();
