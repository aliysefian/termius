// What the Monitoring page knows about each host's network traffic: the latest reading of every interface, the recent
// readings kept for the chart, and the live reading of the host that is open. Readings come from two places: the
// background checks (each host's own interval, at least 30 s apart, with the rest of its readings) and, while a host's
// Network tab is open, a short loop that reads just the interface counters every few seconds over one SSH connection.
// Nothing is kept past a lock, and nothing here is invented: a host without counters has no series.
import * as api from "$lib/api";
import type { TargetStatus } from "$lib/monitoring";
import { addPoint, type NetInterface, type NetPoint, type NetReading } from "$lib/netdata";
import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type Uuid } from "$lib/types";

export interface Latest {
  /** When it was read, on this computer's clock. */
  at: number;
  interfaces: NetInterface[];
  headline: string | null;
  windowSecs: number;
  source: "background" | "live";
}

class NetStore {
  /** Per host (monitor target id), per interface: the recent readings, newest last. */
  series = $state<Record<Uuid, Record<string, NetPoint[]>>>({});
  latest = $state<Record<Uuid, Latest>>({});
  liveTarget = $state<Uuid | null>(null);
  liveError = $state<string | null>(null);
  liveRunning = $state(false);
  #generation = 0;

  constructor() {
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  reset() {
    this.#generation++;
    this.series = {};
    this.latest = {};
    this.liveTarget = null;
    this.liveError = null;
    this.liveRunning = false;
  }

  forget(target: Uuid) {
    delete this.series[target];
    delete this.latest[target];
  }

  /** A reading, from either source: keep each interface's point and remember the latest. */
  ingest(target: Uuid, reading: NetReading, headline: string | null, at: number, source: Latest["source"]) {
    const by = this.series[target] ?? {};
    for (const i of reading.interfaces) by[i.name] = addPoint(by[i.name] ?? [], { t: at, rx: i.rx_bps, tx: i.tx_bps });
    this.series[target] = by;
    this.latest[target] = { at, interfaces: reading.interfaces, headline: headline ?? reading.primary, windowSecs: reading.window_secs, source };
  }

  /** The background check's result for a host. `sshHostId` lets its CPU, memory and speed feed the kept history too. */
  ingestStatus(st: TargetStatus, sshHostId: Uuid | null, at: number = Date.now()) {
    const m = st.metrics;
    // A reading already taken in (the same check, seen again in a snapshot) isn't added twice.
    if (!m || (this.latest[st.id] && this.latest[st.id].at >= at)) return;
    if (m.net) this.ingest(st.id, m.net, m.net_headline, at, "background");
    if (sshHostId) {
      const head = m.net?.interfaces.find((i) => i.name === m.net_headline);
      hostMetrics.record(sshHostId, { t: at, cpu: m.cpu, mem: m.mem, rx: head?.rx_bps ?? null, tx: head?.tx_bps ?? null });
    }
  }

  /** The recent readings of one interface of one host. */
  points(target: Uuid, iface: string): NetPoint[] {
    return this.series[target]?.[iface] ?? [];
  }

  // -- the live reading of the host that is open ----------------------------------------------------------------------

  /** Read `target`'s interface counters every `secs` seconds until [`stopLive`]. One reading takes about a second. */
  async startLive(target: Uuid, sshHostId: Uuid, secs: number) {
    this.stopLive();
    const gen = ++this.#generation;
    this.liveTarget = target;
    this.liveError = null;
    this.liveRunning = true;
    let session: Uuid | null = null;
    const closeSession = () => {
      if (session) void api.monitor.close(session).catch(() => {});
      session = null;
    };
    try {
      while (gen === this.#generation) {
        // Nothing is read while the window is hidden: nobody is looking, and it costs the host a second of work.
        if (!document.hidden) {
          try {
            session ??= await api.monitor.open(sshHostId);
            if (gen !== this.#generation) break;
            const r = await api.monitor.netSample(session);
            if (gen !== this.#generation) break;
            if (r) {
              this.ingest(target, r, r.headline, Date.now(), "live");
              this.liveError = null;
            } else this.liveError = "This host doesn't report network counters (they come from /proc/net/dev on Linux).";
          } catch (e) {
            this.liveError = errorMessage(e);
            // The connection was lost: open a fresh one next time.
            if (isApiError(e) && e.code === "no_session") session = null;
            else closeSession();
          }
        }
        await new Promise((r) => setTimeout(r, Math.max(500, (secs - 1) * 1000)));
      }
    } finally {
      closeSession();
      if (gen === this.#generation || this.liveTarget === target) this.liveRunning = false;
    }
  }

  stopLive() {
    this.#generation++;
    this.liveRunning = false;
    this.liveTarget = null;
  }
}

export const netStore = new NetStore();
