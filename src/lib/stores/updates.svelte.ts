// Self-update: check the release feed, download, verify (the plugin checks
// the signature against the key built into the app), install, restart.
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";
import { settings } from "$lib/stores/settings.svelte";
import { errorMessage } from "$lib/types";

export interface UpdaterInfo {
  enabled: boolean;
  can_install: boolean;
  version: string;
}

const LAST_CHECK = "sshvault.updatecheck.v1";
/** Automatic checks happen at most this often. */
const CHECK_EVERY_MS = 12 * 60 * 60 * 1000;
export const RELEASES_URL = "https://github.com/aliysefian/termius/releases/latest";

class UpdateStore {
  info = $state<UpdaterInfo | null>(null);
  available = $state<Update | null>(null);
  checking = $state(false);
  installing = $state(false);
  /** 0..1 while downloading, null before the size is known. */
  progress = $state<number | null>(null);
  error = $state<string | null>(null);
  /** The user closed the banner for this version. */
  dismissed = $state<string | null>(null);
  lastResult = $state<string | null>(null);

  async init() {
    try {
      this.info = await invoke<UpdaterInfo>("updater_info");
    } catch {
      this.info = null;
    }
    if (!this.info?.enabled || !settings.prefs.autoUpdateCheck) return;
    let last = 0;
    try {
      last = Number(localStorage.getItem(LAST_CHECK) ?? 0);
    } catch {
      // Storage unavailable: just check.
    }
    if (Date.now() - last >= CHECK_EVERY_MS) await this.check(true);
  }

  /** `quiet` checks don't report "you're up to date" or errors loudly. */
  async check(quiet = false) {
    if (!this.info?.enabled || this.checking) return;
    this.checking = true;
    this.error = null;
    try {
      const u = await check();
      this.available = u;
      this.lastResult = u ? `Version ${u.version} is available.` : "You have the latest version.";
      try {
        localStorage.setItem(LAST_CHECK, String(Date.now()));
      } catch {
        // Not remembering the time only means checking again next start.
      }
    } catch (e) {
      // Offline or the feed is unreachable: never nag on automatic checks.
      if (!quiet) this.error = `Couldn't check for updates: ${errorMessage(e)}`;
    } finally {
      this.checking = false;
    }
  }

  /** Download, verify and install, then restart into the new version. */
  async install() {
    const u = this.available;
    if (!u || this.installing) return;
    if (!this.info?.can_install) {
      await openUrl(RELEASES_URL);
      return;
    }
    this.installing = true;
    this.error = null;
    this.progress = null;
    let total = 0;
    let done = 0;
    try {
      await u.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          this.progress = total ? Math.min(1, done / total) : null;
        } else if (ev.event === "Finished") this.progress = 1;
      });
      await relaunch();
    } catch (e) {
      this.error = `The update couldn't be installed: ${errorMessage(e)}`;
      this.installing = false;
    }
  }

  get showBanner() {
    return !!this.available && this.dismissed !== this.available.version;
  }
}

export const updates = new UpdateStore();
