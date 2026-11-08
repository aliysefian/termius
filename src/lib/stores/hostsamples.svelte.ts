// What the Host Metrics views show: the shared engine's latest reading and a short run of frames per host, and the
// one timer that keeps them fresh.
//
// Any number of views may want the same host (the metrics window, a dashboard card, the terminal's panel). They each
// `subscribe` and the store runs ONE timer per host at the fastest interval anyone asked for; the engine itself shares a
// recent reading between callers, so this adds no collection of its own. A hidden window takes no readings, and a locked
// vault forgets everything.
import * as api from "$lib/api";
import { FRAME_WINDOW_MS, addFrame, backfill, frameOf, type Frame } from "$lib/hostframes";
import type { HostSample } from "$lib/hostsample";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type Uuid } from "$lib/types";

/** The live rates a view may ask for, in milliseconds. 0 is "off": one reading on opening, then only on request. */
export const LIVE_CHOICES = [0, 1000, 2000, 5000] as const;
export type Live = (typeof LIVE_CHOICES)[number];

interface Watch {
  /** What each subscriber asked for. */
  wants: Map<symbol, number>;
  timer?: ReturnType<typeof setInterval>;
  every: number;
  inflight: boolean;
}

class HostSamplesStore {
  latest = $state<Record<Uuid, HostSample>>({});
  frames = $state<Record<Uuid, Frame[]>>({});
  /** The last failure per host, null after a reading worked. */
  errors = $state<Record<Uuid, string | null>>({});
  /** True from the first request until the first reading (or failure) of a host. */
  loading = $state<Record<Uuid, boolean>>({});
  /** When this window last got a reading of the host, on this computer's clock. */
  receivedAt = $state<Record<Uuid, number>>({});
  #watches = new Map<Uuid, Watch>();
  #unlisten: (() => void)[] = [];
  #listening = false;

  constructor() {
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", () => !document.hidden && this.#wake());
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  /** Take a reading into the store. The same reading arriving twice (event and reply) is one. */
  ingest(s: HostSample) {
    const id = s.host_id;
    const cur = this.latest[id];
    if (!cur || s.taken_at_ms >= cur.taken_at_ms) this.latest[id] = s;
    this.frames[id] = addFrame(this.frames[id] ?? [], frameOf(s));
    this.errors[id] = null;
    this.loading[id] = false;
    this.receivedAt[id] = Date.now();
  }

  async #listen() {
    if (this.#listening) return;
    this.#listening = true;
    try {
      this.#unlisten.push(await api.hostSamples.onSample((s) => this.ingest(s)));
    } catch {
      // Without events the timer's own replies still fill the store.
      this.#listening = false;
    }
  }

  /**
   * Ask for a host to be kept fresh every `every` ms (0: not on a timer). Returns what to call when the view goes
   * away. The first call for a host also loads what the engine already kept, so a chart opens with a past.
   */
  subscribe(id: Uuid, every: number): (() => void) & { set: (ms: number) => void } {
    void this.#listen();
    let w = this.#watches.get(id);
    const first = !w;
    if (!w) {
      w = { wants: new Map(), every: 0, inflight: false };
      this.#watches.set(id, w);
    }
    const key = Symbol();
    w.wants.set(key, every);
    this.#retime(id);
    if (first) void this.#start(id);
    const stop = () => {
      const cur = this.#watches.get(id);
      if (!cur) return;
      cur.wants.delete(key);
      if (cur.wants.size === 0) {
        if (cur.timer) clearInterval(cur.timer);
        this.#watches.delete(id);
      } else this.#retime(id);
    };
    /** `set` changes how fast this subscriber wants readings; calling the function itself stops it. */
    return Object.assign(stop, {
      set: (ms: number) => {
        const cur = this.#watches.get(id);
        if (cur?.wants.has(key)) {
          cur.wants.set(key, ms);
          this.#retime(id);
        }
      },
    });
  }

  #retime(id: Uuid) {
    const w = this.#watches.get(id);
    if (!w) return;
    const asks = [...w.wants.values()].filter((v) => v > 0);
    const every = asks.length ? Math.min(...asks) : 0;
    if (every === w.every && (w.timer || !every)) return;
    if (w.timer) clearInterval(w.timer);
    w.timer = undefined;
    w.every = every;
    if (every) w.timer = setInterval(() => void this.refresh(id), every);
  }

  async #start(id: Uuid) {
    this.loading[id] = !this.latest[id];
    // What the engine has kept already (from other views, or an earlier visit) gives the charts a past.
    try {
      const points = await api.hostSamples.history(id, Date.now() - FRAME_WINDOW_MS);
      if (points.length) this.frames[id] = backfill(this.frames[id] ?? [], points);
    } catch {
      // No history is not an error.
    }
    await this.refresh(id);
  }

  /** One reading now, unless one is already on its way or the window is hidden (a manual refresh passes `force`). */
  async refresh(id: Uuid, force = false) {
    const w = this.#watches.get(id);
    if (!vaultStore.unlocked || (w?.inflight && !force) || (!force && typeof document !== "undefined" && document.hidden)) return;
    if (w) w.inflight = true;
    try {
      // A bit under the interval, so the tick finds a reading that is due, while another view's recent one is reused.
      const maxAge = w && w.every ? Math.max(500, Math.round(w.every * 0.75)) : 2000;
      this.ingest(await api.hostSamples.sample(id, maxAge));
    } catch (e) {
      // "Cooling" is the engine sparing a struggling host: the last failure is already on screen.
      if (!(isApiError(e) && (e.code === "metrics_cooling" || e.code === "metrics_cancelled"))) this.errors[id] = errorMessage(e);
      this.loading[id] = false;
    } finally {
      if (w) w.inflight = false;
    }
  }

  #wake() {
    for (const id of this.#watches.keys()) void this.refresh(id);
  }

  /** Forget everything: for a locked vault. Watching stops too. */
  reset() {
    for (const w of this.#watches.values()) if (w.timer) clearInterval(w.timer);
    this.#watches.clear();
    this.latest = {};
    this.frames = {};
    this.errors = {};
    this.loading = {};
    this.receivedAt = {};
  }

  /** Stop reading a host and drop what was kept (also in the engine). */
  async forget(id: Uuid) {
    delete this.latest[id];
    delete this.frames[id];
    delete this.errors[id];
    delete this.receivedAt[id];
    await api.hostSamples.forget(id).catch(() => {});
  }
}

export const hostSamples = new HostSamplesStore();
