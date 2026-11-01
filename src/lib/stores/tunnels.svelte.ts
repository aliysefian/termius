// The Tunnels workspace's state: what is selected, how the list is filtered, which operations are in flight, and what
// the backend has reported about each tunnel's uptime and log. The rules themselves and their live status come from
// the vault store (pushed by the backend); nothing here simulates a state. It lives outside the page so that opening
// a terminal, and coming back, finds the selection, filters and tab where they were.

import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { bulkMessage, buildRow, canRestart, canStart, canStop, type Pending, type StatusFilter, type TunnelRow, type TypeFilter } from "$lib/tunnels";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type ForwardLog, type Uuid } from "$lib/types";

export type DetailTab = "overview" | "configuration" | "logs";
/** Lines kept per tunnel in the window; the backend keeps its own bounded copy. */
const MAX_LOGS = 200;

class TunnelsStore {
  selectedId = $state<Uuid | null>(null);
  query = $state("");
  typeFilter = $state<TypeFilter>("all");
  statusFilter = $state<StatusFilter>("all");
  tab = $state<DetailTab>("overview");
  checked = $state<Set<Uuid>>(new Set());
  /** Operations asked for and not yet answered by the backend: what stops a second click doing it twice. */
  pending = $state<Record<Uuid, Pending>>({});
  activeSince = $state<Record<Uuid, number>>({});
  logs = $state<Record<Uuid, ForwardLog[]>>({});
  /** Whether the details panel is open as a slide-over (on a narrow window). */
  slideOver = $state(false);
  /** Whether the page has room to show the details beside the list; the page keeps this up to date. */
  docked = $state(true);

  /** Show the selected tunnel's details: they are already beside the list when docked, else they slide over it. */
  reveal() {
    this.slideOver = !this.docked;
  }

  /** What the person asked for, so the result is announced once, with the backend's own words on a failure. */
  #expect: Record<Uuid, "start" | "restart" | "stop"> = {};
  #unlisten: (() => void)[] = [];
  #subscribers = 0;

  /** Every configured tunnel as a row, in name order. */
  get rows(): TunnelRow[] {
    return vaultStore.forwards
      .filter((f) => f.data)
      .map((f) => buildRow(f.id, f.data!, vaultStore.hostById.get(f.data!.host_id)?.data, vaultStore.forwardStatus[f.id], this.pending[f.id], this.activeSince[f.id]))
      .sort((a, b) => a.rule.label.localeCompare(b.rule.label));
  }

  get selected(): TunnelRow | undefined {
    return this.rows.find((r) => r.id === this.selectedId);
  }

  get filtering(): boolean {
    return !!this.query.trim() || this.typeFilter !== "all" || this.statusFilter !== "all";
  }

  clearFilters() {
    this.query = "";
    this.typeFilter = "all";
    this.statusFilter = "all";
  }

  select(id: Uuid | null, tab?: DetailTab) {
    this.selectedId = id;
    if (tab) this.tab = tab;
    if (id) void this.loadLogs(id);
  }

  // -- backend subscriptions, held while the page is open ------------------------------------------------------

  /** Start listening for the backend's log lines and state changes. Returns the function that stops. */
  async subscribe(): Promise<() => void> {
    this.#subscribers++;
    if (this.#subscribers === 1) {
      void this.refreshRuntime();
      const offLog = await api.forwards.onLog(({ rule_id, entry }) => this.#append(rule_id, entry));
      const offStatus = await api.forwards.onStatus(({ rule_id, status }) => this.#onStatus(rule_id, status.state, status.state === "error" ? status.message : ""));
      this.#unlisten = [offLog, offStatus];
    }
    return () => {
      this.#subscribers--;
      if (this.#subscribers === 0) {
        for (const off of this.#unlisten) off();
        this.#unlisten = [];
      }
    };
  }

  async refreshRuntime() {
    try {
      const rt = await api.forwards.runtime();
      this.activeSince = Object.fromEntries(Object.entries(rt).flatMap(([id, r]) => (r.active_since_ms ? [[id, r.active_since_ms]] : [])));
    } catch {
      // Without it the uptime is simply not shown.
    }
  }

  async loadLogs(id: Uuid) {
    try {
      this.logs[id] = (await api.forwards.logs(id)).slice(-MAX_LOGS);
    } catch {
      this.logs[id] = [];
    }
  }

  #append(id: Uuid, entry: ForwardLog) {
    const list = this.logs[id] ?? [];
    // A line that arrived while the history was loading may be in both.
    if (list.some((l) => l.at_ms === entry.at_ms && l.message === entry.message && l.level === entry.level)) return;
    this.logs[id] = [...list, entry].slice(-MAX_LOGS);
  }

  #onStatus(id: Uuid, state: string, message: string) {
    const name = vaultStore.forwards.find((f) => f.id === id)?.data?.label ?? "Tunnel";
    const pending = this.pending[id];
    const expect = this.#expect[id];
    if (state === "active") {
      void this.refreshRuntime();
      if (pending === "starting" || pending === "restarting") delete this.pending[id];
      if (expect === "start" || expect === "restart") ui.notify("info", `${expect === "restart" ? "Restarted" : "Started"} tunnel "${name}".`);
      delete this.#expect[id];
    } else if (state === "error") {
      delete this.pending[id];
      delete this.activeSince[id];
      if (expect) ui.notify("error", `Tunnel "${name}" failed: ${message}`);
      else if (pending !== "stopping") ui.notify("error", `Tunnel "${name}" stopped working: ${message}`);
      delete this.#expect[id];
    } else if (state === "stopped") {
      delete this.activeSince[id];
      // A restart passes through "stopped" on its way: only a stop that was asked for ends here.
      if (pending === "stopping") delete this.pending[id];
      if (expect === "stop") {
        ui.notify("info", `Stopped tunnel "${name}".`);
        delete this.#expect[id];
      }
    }
  }

  // -- operations ----------------------------------------------------------------------------------------------

  /** Whether a stop or restart is underway for this tunnel (a start in progress can still be cancelled by stopping). */
  busy(id: Uuid): boolean {
    const p = this.pending[id];
    return p === "stopping" || p === "restarting";
  }

  async start(id: Uuid): Promise<string | null> {
    const row = this.rows.find((r) => r.id === id);
    if (!row || this.pending[id] || !canStart(row)) return "not startable";
    this.pending[id] = "starting";
    this.#expect[id] = "start";
    try {
      await api.forwards.start(id);
      return null;
    } catch (e) {
      delete this.pending[id];
      delete this.#expect[id];
      const message = errorMessage(e);
      vaultStore.forwardStatus[id] = { state: "error", message };
      ui.notify("error", `Couldn't start "${row.rule.label}": ${message}`);
      return message;
    }
  }

  async stop(id: Uuid): Promise<string | null> {
    const row = this.rows.find((r) => r.id === id);
    if (!row || this.busy(id) || !canStop(row)) return "not running";
    this.pending[id] = "stopping";
    this.#expect[id] = "stop";
    try {
      await api.forwards.stop(id);
      return null;
    } catch (e) {
      delete this.pending[id];
      delete this.#expect[id];
      ui.notify("error", `Couldn't stop "${row.rule.label}": ${errorMessage(e)}`);
      return errorMessage(e);
    }
  }

  async restart(id: Uuid): Promise<string | null> {
    const row = this.rows.find((r) => r.id === id);
    if (!row || this.busy(id) || !canRestart(row)) return "not restartable";
    this.pending[id] = "restarting";
    this.#expect[id] = "restart";
    try {
      await api.forwards.restart(id);
      return null;
    } catch (e) {
      delete this.pending[id];
      delete this.#expect[id];
      const message = errorMessage(e);
      vaultStore.forwardStatus[id] = { state: "error", message };
      ui.notify("error", `Couldn't restart "${row.rule.label}": ${message}`);
      return message;
    }
  }

  async remove(id: Uuid): Promise<boolean> {
    const row = this.rows.find((r) => r.id === id);
    if (!row) return false;
    const running = row.status === "active" || row.status === "connecting";
    const text = `Delete the tunnel "${row.rule.label}"?${running ? " It is running and will be stopped." : ""} You can undo this for a few seconds afterwards.`;
    if (!(await ask(text, { title: "Delete tunnel", confirm: "Delete", danger: true }))) return false;
    try {
      await vaultStore.deleteForward(id);
      this.#forget(id);
      return true;
    } catch (e) {
      ui.notify("error", `Couldn't delete "${row.rule.label}": ${errorMessage(e)}`);
      return false;
    }
  }

  #forget(id: Uuid) {
    if (this.selectedId === id) this.selectedId = null;
    this.checked.delete(id);
    this.checked = new Set(this.checked);
    delete this.pending[id];
    delete this.logs[id];
    delete this.activeSince[id];
  }

  // -- selection for bulk actions -----------------------------------------------------------------------------

  toggleChecked(id: Uuid) {
    const next = new Set(this.checked);
    if (!next.delete(id)) next.add(id);
    this.checked = next;
  }

  setAllChecked(ids: Uuid[], on: boolean) {
    this.checked = on ? new Set(ids) : new Set();
  }

  /** The checked tunnels that still exist, so a deleted one never lingers in the count. */
  get checkedRows(): TunnelRow[] {
    return this.rows.filter((r) => this.checked.has(r.id));
  }

  /** Run `op` for each of these and say, truthfully, how many it was accepted for and which it was not. */
  async #bulk(verb: string, rows: TunnelRow[], op: (id: Uuid) => Promise<string | null>) {
    if (!rows.length) {
      ui.notify("info", `None of the selected tunnels can be ${verb.toLowerCase()} right now.`);
      return;
    }
    const results = await Promise.all(rows.map(async (r) => ({ label: r.rule.label, error: await op(r.id) })));
    const failures = results.filter((r) => r.error !== null).map((r) => ({ label: r.label, error: r.error! }));
    const m = bulkMessage(verb, rows.length, failures);
    ui.notify(m.kind, m.text);
  }

  startChecked() {
    return this.#bulk("Started", this.checkedRows.filter(canStart), (id) => this.start(id));
  }

  stopChecked() {
    return this.#bulk("Stopped", this.checkedRows.filter(canStop), (id) => this.stop(id));
  }

  async deleteChecked() {
    const rows = this.checkedRows;
    if (!rows.length) return;
    const names = rows.slice(0, 5).map((r) => `"${r.rule.label}"`).join(", ") + (rows.length > 5 ? ` and ${rows.length - 5} more` : "");
    if (!(await ask(`Delete ${rows.length} tunnel${rows.length === 1 ? "" : "s"}? ${names}. Running ones are stopped.`, { title: "Delete tunnels", confirm: "Delete", danger: true }))) return;
    const failures: { label: string; error: string }[] = [];
    for (const r of rows) {
      try {
        await vaultStore.deleteForward(r.id);
        this.#forget(r.id);
      } catch (e) {
        failures.push({ label: r.rule.label, error: errorMessage(e) });
      }
    }
    const m = bulkMessage("Deleted", rows.length, failures);
    ui.notify(m.kind, m.text);
  }
}

export const tunnels = new TunnelsStore();
