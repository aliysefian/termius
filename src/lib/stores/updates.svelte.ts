// Self-update. Two routes to the same result:
//  - signed: builds with an update-signing key use the Tauri updater, which
//    checks each download against that key (latest.json in the release);
//  - GitHub: every other build checks the public Releases page, downloads the
//    installer that matches how it was installed, verifies it against the
//    SHA-256 GitHub publishes, and installs it (release.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { ask } from "$lib/dialogs.svelte";
import { errorMessage } from "$lib/types";

export interface UpdaterInfo {
  enabled: boolean;
  can_install: boolean;
  version: string;
}

export type InstallKind = "appimage" | "deb" | "rpm" | "nsis" | "manual";

export interface ReleaseCheck {
  current: string;
  latest: string;
  newer: boolean;
  url: string;
  notes: string;
  published_at: string | null;
  install: InstallKind;
  asset: { name: string; size: number } | null;
}

export interface AvailableUpdate {
  version: string;
  /** The release page. */
  url: string;
  notes: string;
  date: string | null;
  /** Downloaded size in bytes, when known. */
  size: number | null;
  /** How it will be installed; "manual" opens the release page. */
  install: InstallKind | "signed";
  signed?: Update;
}

const LAST_CHECK = "sshvault.updatecheck.v1";
/** Automatic checks happen at most this often. */
const CHECK_EVERY_MS = 12 * 60 * 60 * 1000;
export const RELEASES_URL = "https://github.com/aliysefian/termius/releases/latest";

class UpdateStore {
  info = $state<UpdaterInfo | null>(null);
  available = $state<AvailableUpdate | null>(null);
  checking = $state(false);
  installing = $state(false);
  /** 0..1 while downloading, null before the size is known. */
  progress = $state<number | null>(null);
  /** Human-readable step while installing. */
  stage = $state("");
  error = $state<string | null>(null);
  /** The user closed the banner for this version. */
  dismissed = $state<string | null>(null);
  lastResult = $state<string | null>(null);
  lastChecked = $state<number | null>(null);

  async loadInfo() {
    if (this.info) return;
    try {
      this.info = await invoke<UpdaterInfo>("updater_info");
    } catch {
      this.info = null;
    }
  }

  async init() {
    await this.loadInfo();
    let last = 0;
    try {
      last = Number(localStorage.getItem(LAST_CHECK) ?? 0);
    } catch {
      // Storage unavailable: just check.
    }
    this.lastChecked = last || null;
    if (settings.prefs.autoUpdateCheck && Date.now() - last >= CHECK_EVERY_MS) await this.check(true);
  }

  /** `quiet` checks don't report "you're up to date" or errors loudly. */
  async check(quiet = false) {
    if (this.checking || this.installing) return;
    this.checking = true;
    await this.loadInfo();
    this.error = null;
    try {
      this.available = (await this.#checkSigned()) ?? (await this.#checkGitHub());
      this.lastResult = this.available ? `Version ${this.available.version} is available.` : "You have the latest version.";
      this.lastChecked = Date.now();
      // A manual check shows the banner again even if it was dismissed.
      if (!quiet) this.dismissed = null;
      try {
        localStorage.setItem(LAST_CHECK, String(this.lastChecked));
      } catch {
        // Not remembering the time only means checking again next start.
      }
    } catch (e) {
      // Offline or GitHub unreachable: never nag on automatic checks.
      if (!quiet) this.error = `Couldn't check for updates: ${errorMessage(e)}`;
    } finally {
      this.checking = false;
    }
  }

  /** The signed feed, when this build has a key and the release has one. */
  async #checkSigned(): Promise<AvailableUpdate | null> {
    if (!this.info?.enabled || !this.info.can_install) return null;
    try {
      const u = await check();
      if (!u) return null;
      return { version: u.version, url: RELEASES_URL, notes: u.body ?? "", date: u.date ?? null, size: null, install: "signed", signed: u };
    } catch {
      // No latest.json in this release: fall back to GitHub.
      return null;
    }
  }

  async #checkGitHub(): Promise<AvailableUpdate | null> {
    const r = await invoke<ReleaseCheck>("release_check");
    if (!r.newer) return null;
    return {
      version: r.latest,
      url: r.url,
      notes: r.notes,
      date: r.published_at,
      size: r.asset?.size ?? null,
      install: r.asset ? r.install : "manual",
    };
  }

  get canInstall() {
    return !!this.available && this.available.install !== "manual";
  }

  /** Download, verify and install, then restart into the new version. */
  async install() {
    const u = this.available;
    if (!u || this.installing) return;
    if (u.install === "manual") {
      await openUrl(u.url);
      return;
    }
    this.installing = true;
    this.error = null;
    this.progress = null;
    this.stage = "Downloading…";
    try {
      if (u.install === "signed") await this.#installSigned(u.signed!);
      else await this.#installGitHub(u.install);
    } catch (e) {
      this.error = `The update couldn't be installed: ${errorMessage(e)}`;
      this.installing = false;
      this.stage = "";
    }
  }

  async #installSigned(u: Update) {
    let total = 0;
    let done = 0;
    await u.downloadAndInstall((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? 0;
      else if (ev.event === "Progress") {
        done += ev.data.chunkLength;
        this.progress = total ? Math.min(1, done / total) : null;
      } else if (ev.event === "Finished") {
        this.progress = 1;
        this.stage = "Installing…";
      }
    });
    await relaunch();
  }

  async #installGitHub(kind: InstallKind) {
    const unlisten = await listen<{ done: number; total: number }>("update://progress", (ev) => {
      const { done, total } = ev.payload;
      this.progress = total ? Math.min(1, done / total) : null;
      if (total && done >= total) this.stage = kind === "deb" || kind === "rpm" ? "Waiting for your password to install…" : "Installing…";
    });
    try {
      // Returns only on failure: on success the app exits and restarts.
      await invoke("release_install");
    } finally {
      unlisten();
    }
  }

  get showBanner() {
    return !!this.available && this.dismissed !== this.available.version;
  }
}

export const updates = new UpdateStore();

/** Install, after confirming when that would close open sessions. */
export async function installUpdate() {
  const sessions = ui.tabs.reduce((n, t) => n + t.panes.length, 0);
  if (
    updates.canInstall &&
    sessions > 0 &&
    !await ask(`Updating restarts SSHVault and closes ${sessions} open session${sessions === 1 ? "" : "s"}. Continue?`)
  )
    return;
  await updates.install();
}

/** "7.3 MB" */
export function formatSize(bytes: number | null | undefined): string {
  if (!bytes) return "";
  const mb = bytes / 1024 / 1024;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** One line on what installing will do, for this kind of install. */
export function installHint(kind: AvailableUpdate["install"] | undefined): string {
  switch (kind) {
    case "deb":
    case "rpm":
      return `Installs the new .${kind} package; your system asks for your password.`;
    case "appimage":
      return "Replaces this AppImage with the new one.";
    case "nsis":
      return "Runs the Windows installer; your settings and vault are kept.";
    case "signed":
      return "Downloaded and checked against the key built into this app.";
    case "manual":
      return "This copy can't update itself. Download the new version from the release page.";
    default:
      return "";
  }
}
