// The views that are read when asked rather than with every reading: a host's processes, listening ports and logins. Like
// the host-samples store it keeps ONE timer per host and view however many places look at it, takes nothing while the
// window is hidden or the vault locked, and keeps the last good list when a reading fails.
import * as api from "$lib/api";
import type { LoginReport, PortList, ProcessList } from "$lib/hostsample";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type Uuid } from "$lib/types";

export type DetailKind = "processes" | "ports" | "logins";

/** How often each is read while something is looking at it: processes change fast, logins hardly at all. */
export const EVERY_MS: Record<DetailKind, number> = { processes: 3000, ports: 10_000, logins: 30_000 };

interface Watch {
  n: number;
  timer?: ReturnType<typeof setInterval>;
  inflight: boolean;
}

class HostDetailsStore {
  processes = $state<Record<Uuid, ProcessList>>({});
  ports = $state<Record<Uuid, PortList>>({});
  logins = $state<Record<Uuid, LoginReport>>({});
  /** The last failure per `kind:host`, null after a reading worked. */
  errors = $state<Record<string, string | null>>({});
  /** True from the first request of a `kind:host` until its first answer or failure. */
  loading = $state<Record<string, boolean>>({});
  #watches = new Map<string, Watch>();

  constructor() {
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", () => {
        if (document.hidden) return;
        for (const k of this.#watches.keys()) {
          const [kind, id] = split(k);
          void this.refresh(kind, id);
        }
      });
    }
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  /** Start keeping a view of a host fresh. Returns what to call when the view goes away. */
  watch(kind: DetailKind, id: Uuid): () => void {
    const key = `${kind}:${id}`;
    let w = this.#watches.get(key);
    if (!w) {
      w = { n: 0, inflight: false };
      this.#watches.set(key, w);
      w.timer = setInterval(() => void this.refresh(kind, id), EVERY_MS[kind]);
      this.loading[key] = !this.#have(kind, id);
      void this.refresh(kind, id);
    }
    w.n++;
    let done = false;
    return () => {
      if (done) return;
      done = true;
      const cur = this.#watches.get(key);
      if (!cur || --cur.n > 0) return;
      if (cur.timer) clearInterval(cur.timer);
      this.#watches.delete(key);
    };
  }

  #have(kind: DetailKind, id: Uuid): boolean {
    return !!this[kind][id];
  }

  /** One reading now, unless one is already on its way or the window is hidden (a manual refresh passes `force`). */
  async refresh(kind: DetailKind, id: Uuid, force = false) {
    const key = `${kind}:${id}`;
    const w = this.#watches.get(key);
    if (!vaultStore.unlocked || (w?.inflight && !force) || (!force && typeof document !== "undefined" && document.hidden)) return;
    if (w) w.inflight = true;
    // Shared with other views: ask for a list a little fresher than the interval.
    const maxAge = Math.round(EVERY_MS[kind] * 0.75);
    try {
      if (kind === "processes") this.processes[id] = await api.hostSamples.processes(id, force ? 0 : maxAge);
      else if (kind === "ports") this.ports[id] = await api.hostSamples.ports(id, force ? 0 : maxAge);
      else this.logins[id] = await api.hostSamples.logins(id, force ? 0 : maxAge);
      this.errors[key] = null;
    } catch (e) {
      if (!(isApiError(e) && (e.code === "metrics_cooling" || e.code === "metrics_cancelled"))) this.errors[key] = errorMessage(e);
    } finally {
      this.loading[key] = false;
      if (w) w.inflight = false;
    }
  }

  /** Forget everything and stop: for a locked vault. */
  reset() {
    for (const w of this.#watches.values()) if (w.timer) clearInterval(w.timer);
    this.#watches.clear();
    this.processes = {};
    this.ports = {};
    this.logins = {};
    this.errors = {};
    this.loading = {};
  }
}

const split = (k: string): [DetailKind, Uuid] => {
  const i = k.indexOf(":");
  return [k.slice(0, i) as DetailKind, k.slice(i + 1)];
};

export const hostDetails = new HostDetailsStore();
