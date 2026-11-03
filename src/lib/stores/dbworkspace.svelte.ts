// What the Databases workspace remembers about itself: the explorer's search and filter, what the inspector is
// looking at, which panel drawers are open on a narrow window, and the server-side search results. The sessions,
// trees and documents themselves are in the databases store.
import * as api from "$lib/api";
import type { TypeFilter } from "$lib/dbtree";
import { databases } from "$lib/stores/databases.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import type { DbSearchHit, DbTreeNode, Uuid } from "$lib/types";

export type Focus =
  | { kind: "connection"; connId: Uuid }
  | { kind: "node"; connId: Uuid; path: string[]; node: DbTreeNode }
  | { kind: "doc"; tabId: string };

export type InspectorTab = "overview" | "structure" | "activity" | "properties";

/** A search shorter than this isn't sent to the server: it would match half the catalog. */
export const MIN_SERVER_SEARCH = 2;

class DbWorkspace {
  query = $state("");
  type = $state<TypeFilter>("all");
  /** Show only the connections that are open. */
  connectedOnly = $state(false);
  hits = $state<Record<Uuid, DbSearchHit[]>>({});
  searching = $state<Record<Uuid, boolean>>({});
  focus = $state<Focus | null>(null);
  inspectorTab = $state<InspectorTab>("overview");
  explorerDrawerOpen = $state(false);
  inspectorDrawerOpen = $state(false);
  #timer: ReturnType<typeof setTimeout> | undefined;
  #generation = 0;

  constructor() {
    // What the explorer was looking at goes with the vault, like the sessions themselves.
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  /** The explorer's search text. Typing asks each connected server for matches, after a short pause. */
  setQuery(q: string) {
    this.query = q;
    clearTimeout(this.#timer);
    const term = q.trim();
    this.#generation++;
    this.hits = {};
    this.searching = {};
    if (term.length < MIN_SERVER_SEARCH) return;
    this.#timer = setTimeout(() => void this.#searchServers(term), 250);
  }

  async #searchServers(term: string) {
    const gen = this.#generation;
    await Promise.all(
      Object.entries(databases.sessions).map(async ([connId, s]) => {
        this.searching[connId] = true;
        try {
          const hits = await api.db.search(s.sessionId, term, 60);
          if (gen === this.#generation) this.hits[connId] = hits;
        } catch {
          // A server that can't search (no catalog, no permission) just has no extra results.
        } finally {
          if (gen === this.#generation) this.searching[connId] = false;
        }
      }),
    );
  }

  clearSearch() {
    this.setQuery("");
    this.type = "all";
  }

  selectConnection(connId: Uuid) {
    this.focus = { kind: "connection", connId };
    this.inspectorTab = "overview";
  }

  selectNode(connId: Uuid, path: string[], node: DbTreeNode) {
    this.focus = { kind: "node", connId, path, node };
    this.inspectorTab = "overview";
    // A table's details are read when it is selected, so the inspector has them.
    if ((node.kind === "table" || node.kind === "view") && path.length >= 2) void databases.tableMeta(connId, path[0], path[1]);
  }

  selectDoc(tabId: string) {
    this.focus = { kind: "doc", tabId };
  }

  reset() {
    this.query = "";
    this.type = "all";
    this.connectedOnly = false;
    this.explorerDrawerOpen = false;
    this.inspectorDrawerOpen = false;
    this.hits = {};
    this.searching = {};
    this.focus = null;
  }
}

export const dbWorkspace = new DbWorkspace();
