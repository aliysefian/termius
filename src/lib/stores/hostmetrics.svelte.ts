// Opt-in host monitoring: periodically runs the small portable sampling
// script (src/lib/hostmetrics.ts) over the existing run-on-hosts command,
// no new backend code. Which hosts are monitored, and the readings
// themselves, live only on this computer and are never written to the
// vault.
import * as api from "$lib/api";
import { vaultStore } from "$lib/stores/vault.svelte";
import { METRICS_SCRIPT, parseMetricsOutput, type HostMetrics } from "$lib/hostmetrics";
import type { HostDetail } from "$lib/hostdetail";
import { addSample, fromDetail, fromSummary, type Sample } from "$lib/hosthistory";
import { settings } from "$lib/stores/settings.svelte";
import { KEEP_MS, fold, idbStore, toSamples, within, type ArchiveStore, type Bucket } from "$lib/ops/archive";
import type { Uuid } from "$lib/types";

const MONITORED_KEY = "sshvault.monitored.v1";
const POLL_INTERVAL_MS = 30_000;
const TIMEOUT_SECS = 10;

function load(): Record<Uuid, boolean> {
  try {
    const raw = localStorage.getItem(MONITORED_KEY);
    return raw ? (JSON.parse(raw) as Record<Uuid, boolean>) : {};
  } catch {
    return {};
  }
}

function persist(v: Record<Uuid, boolean>) {
  try {
    localStorage.setItem(MONITORED_KEY, JSON.stringify(v));
  } catch {
    // Not fatal: monitoring just won't resume selected next launch.
  }
}

export interface MetricsReading {
  metrics: HostMetrics | null;
  /** Set when the run itself failed (auth, timeout, ...); the script's own fields are still null in that case. */
  error: string | null;
  at: number;
}

class HostMetricsStore {
  /** Host ids the user turned monitoring on for. Local only, never synced. */
  monitored = $state<Record<Uuid, boolean>>(load());
  /** Last reading per host; absent until the first successful poll. */
  readings = $state<Record<Uuid, MetricsReading>>({});
  /** The last 15 minutes per host, in memory only: never written to disk, never synced. */
  history = $state<Record<Uuid, Sample[]>>({});
  #timer: ReturnType<typeof setInterval> | undefined;
  #polling = false;
  /** Whoever wants to know about each summary reading (the alerts). */
  listeners: ((hostId: Uuid, metrics: HostMetrics, at: number) => void)[] = [];
  /** The kept history, per host, loaded the first time it is needed and saved a minute after it changes. */
  #archive = new Map<Uuid, Bucket[]>();
  #saving = new Map<Uuid, ReturnType<typeof setTimeout>>();
  store: ArchiveStore = idbStore;

  constructor() {
    $effect.root(() => {
      $effect(() => persist($state.snapshot(this.monitored)));
      // Readings are not kept past a lock, so locking leaves nothing about a host's load behind.
      $effect(() => {
        if (!vaultStore.unlocked) {
          this.history = {};
          this.readings = {};
        }
      });
      // Turning "keep history" off deletes what was kept.
      $effect(() => {
        if (settings.prefs.metricsKeep === "off") void this.clearArchive();
      });
      $effect(() => {
        const anyOn = vaultStore.unlocked && Object.values(this.monitored).some(Boolean);
        if (anyOn && !this.#timer) {
          void this.poll();
          this.#timer = setInterval(() => void this.poll(), POLL_INTERVAL_MS);
        } else if (!anyOn && this.#timer) {
          clearInterval(this.#timer);
          this.#timer = undefined;
        }
      });
    });
  }

  /** Add a reading to a host's history, and to the kept history if the person asked for one. */
  record(id: Uuid, sample: Sample) {
    this.history[id] = addSample(this.history[id] ?? [], sample);
    const keep = settings.prefs.metricsKeep;
    if (keep !== "off") void this.#keep(id, sample, KEEP_MS[keep]);
  }

  async #buckets(id: Uuid): Promise<Bucket[]> {
    let b = this.#archive.get(id);
    if (!b) {
      b = await this.store.load(id);
      // A reading may have been folded in while this was loading; keep that.
      this.#archive.set(id, this.#archive.get(id) ?? b);
      b = this.#archive.get(id)!;
    }
    return b;
  }

  async #keep(id: Uuid, sample: Sample, keepMs: number) {
    const before = await this.#buckets(id);
    this.#archive.set(id, fold(before, sample, keepMs));
    if (!this.#saving.has(id)) {
      this.#saving.set(
        id,
        setTimeout(() => {
          this.#saving.delete(id);
          void this.store.save(id, this.#archive.get(id) ?? []);
        }, 60_000),
      );
    }
  }

  /** What was kept for a host over the last `windowMs`, as chart samples. Empty if nothing is kept. */
  async archived(id: Uuid, windowMs: number): Promise<Sample[]> {
    if (settings.prefs.metricsKeep === "off") return [];
    return toSamples(within(await this.#buckets(id), Date.now(), windowMs));
  }

  /** Delete everything kept, here and on disk. */
  async clearArchive() {
    for (const t of this.#saving.values()) clearTimeout(t);
    this.#saving.clear();
    this.#archive.clear();
    await this.store.clear();
  }

  /** A reading from the detail view: CPU, memory and network throughput. */
  recordDetail(id: Uuid, detail: HostDetail) {
    this.record(id, fromDetail(detail, Date.now()));
  }

  isMonitored(id: Uuid): boolean {
    return !!this.monitored[id];
  }

  /** A host needs its own saved credentials to be polled unattended. */
  canMonitor(id: Uuid): boolean {
    return !!vaultStore.effectiveIdentity(vaultStore.hostById.get(id)?.data);
  }

  setMonitored(id: Uuid, on: boolean) {
    const next = { ...this.monitored };
    if (on) next[id] = true;
    else delete next[id];
    this.monitored = next;
    if (on) void this.poll();
  }

  /** Polls every monitored, pollable host right away, outside the regular interval. */
  async poll() {
    if (this.#polling || !vaultStore.unlocked) return;
    const ids = Object.keys(this.monitored).filter((id) => this.monitored[id] && this.canMonitor(id));
    if (!ids.length) return;
    this.#polling = true;
    const runId = `metrics-${Date.now().toString(36)}`;
    try {
      await api.runs.start(
        runId,
        ids.map((id) => ({ host_id: id, command: METRICS_SCRIPT })),
        TIMEOUT_SECS,
        (e) => {
          if (e.event === "finished") {
            const metrics = parseMetricsOutput(e.output.stdout);
            this.readings[e.host_id] = { metrics, error: null, at: Date.now() };
            this.record(e.host_id, fromSummary(metrics, Date.now()));
            for (const l of this.listeners) l(e.host_id, metrics, Date.now());
          } else if (e.event === "failed") this.readings[e.host_id] = { metrics: null, error: e.message, at: Date.now() };
        },
      );
    } catch {
      // Leave the previous readings in place rather than clearing them on a transient failure.
    } finally {
      this.#polling = false;
    }
  }
}

export const hostMetrics = new HostMetricsStore();
