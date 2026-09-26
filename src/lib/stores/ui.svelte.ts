// Ephemeral UI state: which sidebar view is active, open tabs, modals.
import { ssh } from "$lib/ssh";
import { errorMessage, type Uuid } from "$lib/types";

export type View = "hosts" | "keychain" | "sftp" | "forwarding" | "snippets" | "settings";

export interface Pane {
  id: string;
  hostId: Uuid;
}

export interface Tab {
  id: string;
  title: string;
  /** Panes inside the tab. One pane = no split; two = side by side. */
  panes: Pane[];
  split: "none" | "vertical" | "horizontal";
  activePaneId: string;
}

export type Modal =
  | { kind: "host"; id: Uuid | null; group?: string }
  | { kind: "identity"; id: Uuid | null }
  | { kind: "snippet"; id: Uuid | null }
  | { kind: "forward"; id: Uuid | null }
  | null;

let counter = 0;
const nextId = (prefix: string) => `${prefix}-${++counter}-${Date.now().toString(36)}`;

class UiStore {
  view = $state<View>("hosts");
  tabs = $state<Tab[]>([]);
  activeTabId = $state<string | null>(null);
  modal = $state<Modal>(null);
  search = $state("");
  collapsedGroups = $state<Set<string>>(new Set());

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

  openTerminal(hostId: Uuid, title: string) {
    const pane: Pane = { id: nextId("pane"), hostId };
    const tab: Tab = {
      id: nextId("tab"),
      title,
      panes: [pane],
      split: "none",
      activePaneId: pane.id,
    };
    this.tabs.push(tab);
    this.activeTabId = tab.id;
  }

  closeTab(id: string) {
    const idx = this.tabs.findIndex((t) => t.id === id);
    if (idx < 0) return;
    this.tabs.splice(idx, 1);
    if (this.activeTabId === id) {
      this.activeTabId = this.tabs[Math.min(idx, this.tabs.length - 1)]?.id ?? null;
    }
  }

  /** Split the active tab and put `hostId` in the new pane. */
  splitActive(direction: "vertical" | "horizontal", hostId: Uuid) {
    const tab = this.activeTab;
    if (!tab || tab.panes.length >= 2) return;
    const pane: Pane = { id: nextId("pane"), hostId };
    tab.panes.push(pane);
    tab.split = direction;
    tab.activePaneId = pane.id;
  }

  closePane(tabId: string, paneId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    tab.panes = tab.panes.filter((p) => p.id !== paneId);
    if (tab.panes.length === 0) return this.closeTab(tabId);
    tab.split = "none";
    tab.activePaneId = tab.panes[0].id;
  }

  toggleGroup(path: string) {
    const next = new Set(this.collapsedGroups);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    this.collapsedGroups = next;
  }
}

export const ui = new UiStore();
