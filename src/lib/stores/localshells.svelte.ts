// The shells this computer can start for a local terminal. Detected by the
// backend once and kept; "refresh" looks again (a shell may have been installed).
import * as api from "$lib/api";
import type { LocalShell } from "$lib/types";
import { settings } from "$lib/stores/settings.svelte";

class LocalShells {
  list = $state<LocalShell[]>([]);
  loaded = $state(false);
  #pending: Promise<void> | null = null;

  /** Detect once; later calls reuse the answer. */
  ensure(): Promise<void> {
    if (this.loaded) return Promise.resolve();
    return (this.#pending ??= this.refresh());
  }

  async refresh() {
    try {
      const found = await api.localTerm.shells();
      this.list = Array.isArray(found) ? found : [];
    } catch {
      this.list = [];
    }
    this.loaded = true;
    this.#pending = null;
  }

  /** The id a plain "new local terminal" uses: your chosen default if it is still here. */
  get defaultId(): string | null {
    const want = settings.prefs.localShellId;
    return want && this.list.some((s) => s.id === want) ? want : null;
  }
}

export const localShells = new LocalShells();
