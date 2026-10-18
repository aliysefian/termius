// What the terminal panes know about the network around them: ports that servers announced (offered for forwarding
// and the forwards started from them), the route of each SSH session with live round trips, each session's agent
// forwarding, and the signatures forwarded agents were asked for. State only; the components draw it.

import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "$lib/api";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type AgentForwardState, type AgentSignatureEvent, type ForwardKind, type HopInfo, type HopRtt, type Uuid } from "$lib/types";

/** A forward started from a detected port. The forward's live state is `vaultStore.forwardStatus[id]`. */
export interface PaneForward {
  id: Uuid;
  hostId: Uuid;
  remotePort: number;
  /** The first attempt asked for the same port locally; if that was taken, one retry lets the system pick. */
  retried: boolean;
}

export interface PaneHops {
  info: HopInfo[];
  rtts: HopRtt[];
  /** Polls in a row that got no answer, per hop. */
  misses: number[];
  /** When the round trips were measured (ms). */
  at: number;
}

const MAX_SIGNATURES = 50;
/// Signatures for the same key from the same host within this long are one notice with a count.
const GROUP_MS = 4000;

const kindFor = (port: number, localPort: number): ForwardKind => ({
  kind: "local",
  bind_addr: "127.0.0.1",
  bind_port: localPort,
  dest_host: "localhost",
  dest_port: port,
});

class NetworkStore {
  /** Ports detected in a pane's output that are not forwarded or dismissed. */
  offers = $state<Record<string, number[]>>({});
  forwards = $state<Record<string, PaneForward[]>>({});
  hops = $state<Record<string, PaneHops>>({});
  agent = $state<Record<string, { state: AgentForwardState; at: number }>>({});
  signatures = $state<AgentSignatureEvent[]>([]);

  #dismissed = new Map<string, Set<number>>();
  #forgetHooks = new Map<string, (port: number) => void>();
  #probing = new Set<string>();
  #recent: { key: string; at: number; count: number; toast: number } | null = null;
  #unlistenSignature: UnlistenFn | null = null;
  #unlistenStatus: UnlistenFn | null = null;

  async init() {
    if (!this.#unlistenSignature) this.#unlistenSignature = await api.network.onSignature((e) => this.#signed(e));
    if (!this.#unlistenStatus) this.#unlistenStatus = await api.forwards.onStatus(({ rule_id, status }) => void this.#forwardChanged(rule_id, status));
  }

  // -- ports ------------------------------------------------------------------------------------------------------

  /** A pane's detector tells the store about a hook that lets it be offered the same port again later. */
  registerDetector(paneId: string, forget: (port: number) => void) {
    this.#forgetHooks.set(paneId, forget);
  }

  /** A server in this pane announced `port`. Offered once: not if it is already forwarded, offered, or dismissed. */
  offerPort(paneId: string, port: number) {
    if (this.#dismissed.get(paneId)?.has(port)) return;
    if ((this.forwards[paneId] ?? []).some((f) => f.remotePort === port)) return;
    const list = this.offers[paneId] ?? [];
    if (list.includes(port)) return;
    this.offers[paneId] = [...list, port];
  }

  /** Say no to a port: it is not offered again in this pane. */
  dismissOffer(paneId: string, port: number) {
    const set = this.#dismissed.get(paneId) ?? new Set<number>();
    set.add(port);
    this.#dismissed.set(paneId, set);
    this.#dropOffer(paneId, port);
  }

  #dropOffer(paneId: string, port: number) {
    const left = (this.offers[paneId] ?? []).filter((p) => p !== port);
    if (left.length) this.offers[paneId] = left;
    else delete this.offers[paneId];
  }

  async forwardPort(paneId: string, hostId: Uuid, port: number) {
    this.#dropOffer(paneId, port);
    await this.#start(paneId, hostId, port, port, false);
  }

  async #start(paneId: string, hostId: Uuid, remotePort: number, localPort: number, retried: boolean) {
    try {
      const id = await api.forwards.startAdhoc(hostId, kindFor(remotePort, localPort));
      vaultStore.forwardStatus[id] ??= { state: "starting" };
      this.forwards[paneId] = [...(this.forwards[paneId] ?? []), { id, hostId, remotePort, retried }];
    } catch (e) {
      ui.notify("error", `Could not forward port ${remotePort}: ${errorMessage(e)}`);
    }
  }

  async #forwardChanged(ruleId: Uuid, status: { state: string; message?: string }) {
    for (const [paneId, list] of Object.entries(this.forwards)) {
      const f = list.find((x) => x.id === ruleId);
      if (!f) continue;
      // The local port was taken (or not allowed): once, let the system choose one.
      if (status.state === "error" && !f.retried && /in use|address|bind|permission|denied/i.test(status.message ?? "")) {
        this.forwards[paneId] = list.filter((x) => x.id !== ruleId);
        void api.forwards.stop(ruleId).catch(() => {});
        delete vaultStore.forwardStatus[ruleId];
        await this.#start(paneId, f.hostId, f.remotePort, 0, true);
      }
      return;
    }
  }

  async stopForward(paneId: string, id: Uuid) {
    const f = (this.forwards[paneId] ?? []).find((x) => x.id === id);
    try {
      await api.forwards.stop(id);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
    const left = (this.forwards[paneId] ?? []).filter((x) => x.id !== id);
    if (left.length) this.forwards[paneId] = left;
    else delete this.forwards[paneId];
    delete vaultStore.forwardStatus[id];
    // Stopped by the person: the server may start again, and should then be offered again.
    if (f) {
      this.#dismissed.get(paneId)?.delete(f.remotePort);
      this.#forgetHooks.get(paneId)?.(f.remotePort);
    }
  }

  /** A pane is going away: its forwards go with it (their controls live in its header), and its notes are dropped. */
  closePane(paneId: string) {
    for (const f of this.forwards[paneId] ?? []) {
      void api.forwards.stop(f.id).catch(() => {});
      delete vaultStore.forwardStatus[f.id];
    }
    delete this.forwards[paneId];
    delete this.offers[paneId];
    delete this.hops[paneId];
    delete this.agent[paneId];
    this.#dismissed.delete(paneId);
    this.#forgetHooks.delete(paneId);
  }

  // -- route ------------------------------------------------------------------------------------------------------

  /** The hosts on the way to `hostId`, outermost jump first and the host last, after group defaults. */
  chainOf(hostId: Uuid): Uuid[] {
    const chain: Uuid[] = [hostId];
    let cur = vaultStore.hostById.get(hostId)?.data;
    let id = hostId;
    while (chain.length <= 8) {
      const next = vaultStore.effectiveJump(cur, id);
      if (!next || chain.includes(next)) break;
      chain.unshift(next);
      id = next;
      cur = vaultStore.hostById.get(next)?.data;
    }
    return chain;
  }

  /** A pane that is connected to `hostId` now, so its measurements can be shown wherever that host's route is. */
  livePaneFor(hostId: Uuid): string | undefined {
    for (const tab of ui.tabs) {
      for (const p of tab.panes) {
        if (p.target.kind === "host" && p.target.hostId === hostId && ui.paneInfo[p.id]?.status === "connected" && this.hops[p.id]) return p.id;
      }
    }
    return undefined;
  }

  /** Read the hops of a connected session (once; they don't change while it lives). */
  async loadHops(paneId: string) {
    try {
      const info = await api.network.hops(paneId);
      const prev = this.hops[paneId];
      this.hops[paneId] = { info, rtts: prev?.rtts ?? [], misses: prev?.misses ?? info.map(() => 0), at: prev?.at ?? 0 };
    } catch {
      // Not an SSH session, or not connected yet: nothing to show.
    }
  }

  /** Measure every hop now. Quiet on failure: a probe that fails is a data point (a miss), not an error to show. */
  async probeHops(paneId: string) {
    if (this.#probing.has(paneId)) return;
    this.#probing.add(paneId);
    try {
      const rtts = await api.network.hopRtts(paneId);
      const cur = this.hops[paneId];
      if (!cur || !rtts.length) return;
      const misses = rtts.map((r, i) => (r.total_ms === null ? (cur.misses[i] ?? 0) + 1 : 0));
      this.hops[paneId] = { ...cur, rtts, misses, at: Date.now() };
    } catch {
      const cur = this.hops[paneId];
      if (cur) this.hops[paneId] = { ...cur, misses: cur.misses.map((m) => m + 1) };
    } finally {
      this.#probing.delete(paneId);
    }
  }

  // -- agent forwarding -------------------------------------------------------------------------------------------

  async loadAgent(paneId: string) {
    try {
      const state = await api.network.agentForwarding(paneId);
      if (state) this.agent[paneId] = { state, at: Date.now() };
      else delete this.agent[paneId];
    } catch {
      delete this.agent[paneId];
    }
  }

  async setAgent(paneId: string, mode: "off" | "on" | "timed", secs?: number) {
    try {
      this.agent[paneId] = { state: await api.network.setAgentForwarding(paneId, mode, secs), at: Date.now() };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  /** Seconds left on a timed grant, from when the state was read. */
  agentSecondsLeft(paneId: string, nowMs: number): number | null {
    const a = this.agent[paneId];
    if (!a || a.state.state !== "timed") return null;
    return Math.max(0, a.state.expires_in_secs - (nowMs - a.at) / 1000);
  }

  // -- signatures -------------------------------------------------------------------------------------------------

  #signed(e: AgentSignatureEvent) {
    this.signatures = [e, ...this.signatures].slice(0, MAX_SIGNATURES);
    const key = `${e.pane_id}|${e.fingerprint}|${e.refused}`;
    const now = Date.now();
    const r = this.#recent;
    const again = r && r.key === key && now - r.at < GROUP_MS;
    const count = again ? r.count + 1 : 1;
    if (again) ui.dismissToast(r.toast);
    const who = e.key_name ?? e.fingerprint.slice(0, 20);
    const time = new Date(e.at * 1000).toLocaleTimeString();
    const text = `Remote host “${e.host || e.origin}” ${e.refused ? "was refused" : "requested"} an SSH key signature (${who}) · ${time}${count > 1 ? ` · ×${count}` : ""}`;
    ui.notify("warning", text, undefined, 8000);
    // The toast's id is the one just added.
    const id = ui.toasts[ui.toasts.length - 1]?.id ?? 0;
    this.#recent = { key, at: now, count, toast: id };
  }
}

export const networkStore = new NetworkStore();
