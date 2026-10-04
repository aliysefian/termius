// Opt-in host monitoring: periodically runs the small portable sampling
// script (src/lib/hostmetrics.ts) over the existing run-on-hosts command,
// no new backend code. Which hosts are monitored, and the readings
// themselves, live only on this computer and are never written to the
// vault.
import * as api from "$lib/api";
import { vaultStore } from "$lib/stores/vault.svelte";
import { METRICS_SCRIPT, parseMetricsOutput, type HostMetrics } from "$lib/hostmetrics";
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
  #timer: ReturnType<typeof setInterval> | undefined;
  #polling = false;

  constructor() {
    $effect.root(() => {
      $effect(() => persist($state.snapshot(this.monitored)));
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
          if (e.event === "finished") this.readings[e.host_id] = { metrics: parseMetricsOutput(e.output.stdout), error: null, at: Date.now() };
          else if (e.event === "failed") this.readings[e.host_id] = { metrics: null, error: e.message, at: Date.now() };
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
