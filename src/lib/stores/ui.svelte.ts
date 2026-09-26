// Ephemeral UI state: which sidebar view is active, open tabs, modals.
import { ssh, type AdhocTarget, type SessionStatus } from "$lib/ssh";
import { errorMessage, type Uuid } from "$lib/types";

export type View = "hosts" | "keychain" | "sftp" | "forwarding" | "snippets" | "settings";

/** What a pane connects to: a saved host, or an unsaved quick connection. */
export type PaneTarget = { kind: "host"; hostId: Uuid } | { kind: "adhoc"; adhoc: AdhocTarget };

export interface Pane {
  id: string;
  target: PaneTarget;
}

export interface Tab {
  id: string;
  title: string;
  /** Set when the user renames the tab; wins over `title`. */
  customTitle?: string;
  /** Panes inside the tab. One pane = no split; two = side by side. */
  panes: Pane[];
  split: "none" | "vertical" | "horizontal";
  activePaneId: string;
}

/** Live per-pane info, written by TerminalPane and read by tabs and palette. */
export interface PaneInfo {
  status: SessionStatus["kind"];
  /** Window title set by the remote shell (OSC 0/2), if any. */
  remoteTitle?: string;
}

export type Modal =
  | { kind: "host"; id: Uuid | null; group?: string }
  | { kind: "identity"; id: Uuid | null }
  | { kind: "snippet"; id: Uuid | null }
  | { kind: "forward"; id: Uuid | null }
  | { kind: "quick-connect"; initial?: string }
  | { kind: "import-ssh-config" }
  | null;

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
  collapsedGroups = $state<Set<string>>(new Set());
  paneInfo = $state<Record<string, PaneInfo>>({});
  /** Bumped to ask the active pane to open its find bar. */
  findRequest = $state(0);

  activeTab = $derived(this.tabs.find((t) => t.id === this.activeTabId) ?? null);
  /** Mounted lazily on first visit, then kept alive so sessions survive view switches. */
  sftpVisited = $state(false);
  toast = $state<{ kind: "info" | "error"; text: string } | null>(null);
  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  notify(kind: "info" | "error", text: string) {
    this.toast = { kind, text };
    clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => (this.toast = null), 4000);
  }

  /** Aggregate connection state for a tab's status dot. */
  tabStatus(tab: Tab): PaneInfo["status"] {
    const kinds = tab.panes.map((p) => this.paneInfo[p.id]?.status ?? "connecting");
    if (kinds.some((k) => k === "error" || k === "host_key_changed")) return "error";
    if (kinds.every((k) => k === "connected")) return "connected";
    if (kinds.some((k) => k === "connecting")) return "connecting";
    return "disconnected";
  }

  /**
   * Send a snippet to terminals. `execute` appends Enter; `scope: "tab"`
   * broadcasts to every pane in the active tab (e.g. both halves of a split).
   */
  async runSnippet(command: string, opts: { execute: boolean; scope: "pane" | "tab" }) {
    const tab = this.activeTab;
    if (!tab) {
      this.notify("error", "Open a terminal first, then run the snippet.");
      return;
    }
    const panes = opts.scope === "tab" ? tab.panes.map((p) => p.id) : [tab.activePaneId];
    // Terminals expect CR for Enter; normalise multi-line snippets.
    let text = command.replace(/\r?\n/g, "\r");
    if (opts.execute && !text.endsWith("\r")) text += "\r";
    if (!opts.execute) text = text.replace(/\r$/, "");
    const results = await Promise.allSettled(panes.map((id) => ssh.write(id, text)));
    const failed = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
    if (failed.length === panes.length) {
      this.notify("error", `Snippet not sent: ${errorMessage(failed[0].reason)}`);
    } else if (failed.length) {
      this.notify("info", `Sent to ${panes.length - failed.length} of ${panes.length} panes.`);
    }
  }

  #openTab(target: PaneTarget, title: string) {
    const pane: Pane = { id: nextId("pane"), target };
    const tab: Tab = {
      id: nextId("tab"),
      title,
      panes: [pane],
      split: "none",
      activePaneId: pane.id,
    };
    this.tabs.push(tab);
    this.activeTabId = tab.id;
    // Terminals live in the Hosts/Keychain/... layouts, not SFTP or Settings.
    if (this.view === "sftp" || this.view === "settings") this.view = "hosts";
  }

  openTerminal(hostId: Uuid, title: string) {
    this.#openTab({ kind: "host", hostId }, title);
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

  /** Split the active tab, opening the same target in the new pane. */
  splitActive(direction: "vertical" | "horizontal") {
    const tab = this.activeTab;
    if (!tab || tab.panes.length >= 2) return;
    const source = tab.panes.find((p) => p.id === tab.activePaneId) ?? tab.panes[0];
    const pane: Pane = { id: nextId("pane"), target: structuredClone($state.snapshot(source.target)) as PaneTarget };
    tab.panes.push(pane);
    tab.split = direction;
    tab.activePaneId = pane.id;
  }

  closePane(tabId: string, paneId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    delete this.paneInfo[paneId];
    tab.panes = tab.panes.filter((p) => p.id !== paneId);
    if (tab.panes.length === 0) return this.closeTab(tabId);
    tab.split = "none";
    tab.activePaneId = tab.panes[0].id;
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
    const next = new Set(this.collapsedGroups);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    this.collapsedGroups = next;
  }
}

export const ui = new UiStore();
