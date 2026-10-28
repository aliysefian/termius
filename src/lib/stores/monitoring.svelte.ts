// The monitoring page's data. The checks themselves run in the backend while the vault is unlocked; this holds what
// they read, the alerts they raised, and the stored hosts, rules and channels, and keeps them up to date from the
// events the backend sends. A lock empties it all: nothing about the hosts is kept in the window once the vault is shut.
import { untrack } from "svelte";
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "$lib/api";
import type { AlertEntry, AlertRule, ChannelKind, ChannelsRecord, MonitorTarget, NotifyChannels, TargetStatus } from "$lib/monitoring";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Uuid, type VaultRecord } from "$lib/types";

const KEEP_ALERTS = 200;

/** What comes from the vault may lack fields that were never set. */
function fillTarget(t: MonitorTarget): MonitorTarget {
  return { ...t, tags: t.tags ?? [], ssh_host_id: t.ssh_host_id ?? null, notes: t.notes ?? "" };
}
function fillRule(r: AlertRule): AlertRule {
  return { ...r, warn: r.warn ?? null, crit: r.crit ?? null, channels: r.channels ?? [] };
}

class MonitoringStore {
  targets = $state<VaultRecord<MonitorTarget>[]>([]);
  rules = $state<VaultRecord<AlertRule>[]>([]);
  statuses = $state<Record<Uuid, TargetStatus>>({});
  /** Newest first. What was raised since the vault was unlocked. */
  history = $state<AlertEntry[]>([]);
  running = $state(false);
  paused = $state(false);
  configError = $state<string | null>(null);
  channels = $state<ChannelsRecord | null>(null);
  error = $state<string | null>(null);
  /** Alerts raised since the page was last looked at. */
  unread = $state(0);
  #unlisten: UnlistenFn[] = [];
  #listening = false;

  constructor() {
    $effect.root(() => {
      $effect(() => {
        if (vaultStore.unlocked) untrack(() => void this.#start());
        else untrack(() => this.#stop());
      });
    });
  }

  targetName = (id: Uuid) => this.targets.find((t) => t.id === id)?.data?.label ?? "a host that was removed";

  async #start() {
    await Promise.all([this.refreshLists(), this.refreshSnapshot(), this.loadChannels()]);
    if (this.#listening || !vaultStore.unlocked) return;
    this.#listening = true;
    this.#unlisten = await Promise.all([
      api.monitoring.onStatus((s) => (this.statuses[s.id] = s)),
      api.monitoring.onAlert((a) => {
        this.history = [a, ...this.history].slice(0, KEEP_ALERTS);
        this.unread += 1;
      }),
      api.vault.onChanged((c) => {
        if (c.collection === "monitors" || c.collection === "alert_rules") void this.refreshLists();
        if (c.collection === "notify_channels") void this.loadChannels();
      }),
    ]);
  }

  #stop() {
    for (const off of this.#unlisten) off();
    this.#unlisten = [];
    this.#listening = false;
    this.targets = [];
    this.rules = [];
    this.statuses = {};
    this.history = [];
    this.running = false;
    this.paused = false;
    this.channels = null;
    this.unread = 0;
  }

  async refreshLists() {
    try {
      const [targets, rules] = await Promise.all([api.monitoring.listTargets(), api.monitoring.listRules()]);
      this.targets = (targets ?? []).map((r) => (r.data ? { ...r, data: fillTarget(r.data) } : r));
      this.rules = (rules ?? []).map((r) => (r.data ? { ...r, data: fillRule(r.data) } : r));
      this.error = null;
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async refreshSnapshot() {
    try {
      const s = await api.monitoring.snapshot();
      if (!s) return;
      this.running = s.running;
      this.paused = s.paused;
      this.configError = s.config_error;
      this.statuses = Object.fromEntries(s.statuses.map((t) => [t.id, t]));
      this.history = s.history;
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async loadChannels() {
    try {
      this.channels = (await api.monitoring.getChannels()) ?? null;
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async saveChannels(channels: NotifyChannels) {
    this.channels = await api.monitoring.saveChannels(this.channels?.rev ?? 0, channels);
  }

  testChannel(kind: ChannelKind) {
    return api.monitoring.testChannel(kind);
  }

  async saveTarget(id: Uuid | null, baseRev: number | null, target: MonitorTarget) {
    await api.monitoring.saveTarget(id, baseRev, target);
    await this.refreshLists();
    // The engine picks the new host up at its next pass; show its first reading soon.
    setTimeout(() => void this.refreshSnapshot(), 1500);
  }

  async deleteTarget(id: Uuid, baseRev: number | null) {
    await api.monitoring.deleteTarget(id, baseRev);
    delete this.statuses[id];
    await this.refreshLists();
  }

  async saveRule(id: Uuid | null, baseRev: number | null, rule: AlertRule) {
    await api.monitoring.saveRule(id, baseRev, rule);
    await this.refreshLists();
  }

  async deleteRule(id: Uuid, baseRev: number | null) {
    await api.monitoring.deleteRule(id, baseRev);
    await this.refreshLists();
  }

  async runNow(id: Uuid) {
    await api.monitoring.runNow(id);
  }
}

export const monitoring = new MonitoringStore();
