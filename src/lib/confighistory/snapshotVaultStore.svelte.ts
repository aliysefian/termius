// The Time-Machine's store, as the interface sees it: the tracked files, how much disk they take, and the version
// that was just kept (for the status bar). The files themselves live encrypted on this computer (Rust:
// src-tauri/src/confighistory.rs); this is the reactive front of it.
import * as api from "$lib/api";
import type { ConfigSnapshot, TrackedConfigFile } from "$lib/api";
import { errorMessage } from "$lib/types";

/** How long the "snapshot saved" note stays in the status bar. */
export const NOTE_MS = 7000;

class SnapshotVault {
  files = $state<TrackedConfigFile[]>([]);
  usage = $state({ bytes: 0, snapshots: 0 });
  /** The version kept most recently, until its note fades. */
  note = $state<ConfigSnapshot | null>(null);
  error = $state<string | null>(null);
  #timer: ReturnType<typeof setTimeout> | undefined;

  async refresh() {
    try {
      [this.files, this.usage] = await Promise.all([api.configHistory.files(null), api.configHistory.usage()]);
      this.error = null;
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  /** Show that `snapshot` was kept. */
  announce(snapshot: ConfigSnapshot) {
    this.note = snapshot;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => (this.note = null), NOTE_MS);
    void this.refresh();
  }

  dismissNote() {
    clearTimeout(this.#timer);
    this.note = null;
  }

  async clear() {
    const n = await api.configHistory.clear();
    this.dismissNote();
    await this.refresh();
    return n;
  }

  async deleteFile(hostId: string, path: string) {
    await api.configHistory.deleteFile(hostId, path);
    await this.refresh();
  }
}

export const snapshotVault = new SnapshotVault();

/** `14.2 MB`. */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) (v /= 1024), i++;
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}
