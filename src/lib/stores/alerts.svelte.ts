// Tells the person when a monitored host stops answering or runs hot, while the app is open. Which hosts
// are watched is the same opt-in as monitoring (hostmetrics); the rules and the muted hosts are per
// computer. Nothing leaves this computer: alerts are a toast, an operating-system notification, and a list.
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import * as api from "$lib/api";
import { AlertEngine, type Alert, type AlertRules } from "$lib/ops/alerts";
import { settings } from "$lib/stores/settings.svelte";
import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import type { Uuid } from "$lib/types";

export const CHECK_EVERY_MS = 60_000;
const KEEP_ALERTS = 100;

export function rulesFromPrefs(p: typeof settings.prefs): AlertRules {
  const limit = (v: number) => (v > 0 ? v : null);
  return {
    down: p.alertDown,
    cpu: limit(p.alertCpu),
    mem: limit(p.alertMem),
    disk: limit(p.alertDisk),
    samples: p.alertSamples,
    quiet: { on: p.alertQuiet, from: p.alertQuietFrom, to: p.alertQuietTo },
  };
}

class AlertsStore {
  /** Newest first. Not saved: it is what happened since the app was opened. */
  log = $state<Alert[]>([]);
  unread = $state(0);
  checking = $state(false);
  readonly engine = new AlertEngine(
    () => rulesFromPrefs(settings.prefs),
    (id) => settings.prefs.alertMuted.includes(id),
  );
  #timer: ReturnType<typeof setInterval> | undefined;

  constructor() {
    hostMetrics.listeners.push((id, m) => this.#metrics(id, m));
    $effect.root(() => {
      $effect(() => {
        const on = vaultStore.unlocked && settings.prefs.alerts && settings.prefs.alertDown && Object.values(hostMetrics.monitored).some(Boolean);
        if (on && !this.#timer) {
          void this.check();
          this.#timer = setInterval(() => void this.check(), CHECK_EVERY_MS);
        } else if (!on && this.#timer) {
          clearInterval(this.#timer);
          this.#timer = undefined;
        }
      });
      // A lock leaves nothing about the hosts behind.
      $effect(() => {
        if (!vaultStore.unlocked) {
          this.log = [];
          this.unread = 0;
        }
      });
    });
  }

  /** The hosts watched: monitored, and not muted. */
  watched(): Uuid[] {
    return Object.keys(hostMetrics.monitored).filter((id) => hostMetrics.monitored[id] && !settings.prefs.alertMuted.includes(id));
  }

  isMuted(id: Uuid) {
    return settings.prefs.alertMuted.includes(id);
  }

  setMuted(id: Uuid, muted: boolean) {
    const rest = settings.prefs.alertMuted.filter((x) => x !== id);
    settings.prefs.alertMuted = muted ? [...rest, id] : rest;
    if (muted) this.engine.forget(id);
  }

  /** Check the watched hosts for reachability now. */
  async check() {
    if (this.checking || !settings.prefs.alerts) return;
    const ids = this.watched();
    if (!ids.length) return;
    this.checking = true;
    try {
      const results = await api.health.check(ids);
      const at = Date.now();
      for (const r of results) {
        const { host_id, ...h } = r;
        vaultStore.health[host_id] = { ...h, at };
        // "Behind a jump host" was not probed: it says nothing either way.
        if (r.state === "via_jump") continue;
        const name = vaultStore.hostById.get(host_id)?.data?.label ?? "A host";
        this.#announce(this.engine.health(host_id, name, r.state === "up", at));
      }
    } catch {
      // The check itself failed (the vault was locked, say): not the hosts' fault, so no alert.
    } finally {
      this.checking = false;
    }
  }

  #metrics(id: Uuid, m: { cpuPct: number | null; memPct: number | null; diskPct: number | null }) {
    if (!settings.prefs.alerts) return;
    const name = vaultStore.hostById.get(id)?.data?.label ?? "A host";
    this.#announce(this.engine.metrics(id, name, m, Date.now()));
  }

  #announce(alerts: Alert[]) {
    for (const a of alerts) {
      this.log = [a, ...this.log].slice(0, KEEP_ALERTS);
      this.unread++;
      if (a.quiet) continue;
      ui.notify(a.kind === "up" || a.kind === "cleared" ? "info" : "error", a.message);
      void this.#os(a);
    }
  }

  async #os(a: Alert) {
    // The toast is enough while the window is in front.
    if (document.hasFocus() && !document.hidden) return;
    try {
      let ok = await isPermissionGranted();
      if (!ok) ok = (await requestPermission()) === "granted";
      if (ok) sendNotification({ title: "SSHVault", body: a.message });
    } catch {
      // No notification service; the list still has it.
    }
  }

  clear() {
    this.log = [];
    this.unread = 0;
  }
}

export const alerts = new AlertsStore();
