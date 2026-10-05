// The Containers view's live state: which computers are open, what each one
// runs, and the actions on it. Per window; it goes away when the vault locks.
import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { isLive, shellCommand } from "$lib/containerdata";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import {
  CONTAINER_RUNTIMES,
  errorMessage,
  isApiError,
  type ContainerAction,
  type ContainerInfo,
  type ComposeVerb,
  type ContainerListing,
  type ContainerResources,
  type ContainerRuntime,
  type ResourceKind,
  type Uuid,
} from "$lib/types";

export const LOCAL = "local";
export const REFRESH_CHOICES = [2, 5, 10, 30] as const;

export interface Source {
  /** `"local"` or a host's id. */
  key: string;
  hostId: Uuid | null;
  label: string;
  sessionId: Uuid;
  /** What was found installed there. */
  detected: ContainerRuntime[];
  runtime: ContainerRuntime;
  listing: ContainerListing | null;
  error: string | null;
  loading: boolean;
  updatedAt: number;
  /** Ask the runtime for container and volume sizes (slower on big hosts). */
  sizes: boolean;
  /** Volumes and networks (Docker). Only fetched while one of their tabs is open. */
  resources: ContainerResources | null;
  resourcesError: string | null;
  wantResources: boolean;
}

class ContainersStore {
  sources = $state<Record<string, Source>>({});
  connecting = $state<Record<string, boolean>>({});
  activeKey = $state<string | null>(null);
  /** Pauses the periodic refresh (a manual refresh still works). */
  paused = $state(false);
  intervalSecs = $state<number>(5);
  /** Container ids with an action in flight, so a button can't be pressed twice. */
  busy = $state<Record<string, boolean>>({});

  active = $derived(this.activeKey ? (this.sources[this.activeKey] ?? null) : null);

  constructor() {
    // The backend closes every session when the vault locks; forget ours too.
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  reset() {
    if (Object.keys(this.sources).length) {
      this.sources = {};
      this.connecting = {};
      this.activeKey = null;
      this.busy = {};
    }
  }

  label(key: string): string {
    if (key === LOCAL) return "This computer";
    return vaultStore.hostById.get(key)?.data?.label ?? "host";
  }

  /** Production hosts get the careful prompts. */
  isProduction(key: string): boolean {
    if (key === LOCAL) return false;
    return vaultStore.effectiveEnv(vaultStore.hostById.get(key)?.data) === "production";
  }

  // -- sources -------------------------------------------------------------

  /** Open a source (this computer or a host) and make it the active one. */
  async open(key: string): Promise<Source | null> {
    this.activeKey = key;
    if (this.sources[key]) return this.sources[key];
    if (this.connecting[key]) return null;
    this.connecting[key] = true;
    try {
      const opened = await api.containers.open(key === LOCAL ? null : key);
      const detected = opened.runtimes;
      this.sources[key] = {
        key,
        hostId: key === LOCAL ? null : key,
        label: this.label(key),
        sessionId: opened.session_id,
        detected,
        runtime: detected[0] ?? "docker",
        listing: null,
        error: detected.length ? null : "No container runtime was found here (looked for docker, podman and nerdctl). If one is installed in an unusual place, pick it by hand and try again.",
        loading: false,
        updatedAt: 0,
        sizes: false,
        resources: null,
        resourcesError: null,
        wantResources: false,
      };
      if (detected.length) await this.refresh(key);
      return this.sources[key];
    } catch (e) {
      ui.notify("error", `${this.label(key)}: ${errorMessage(e)}`);
      if (this.activeKey === key) this.activeKey = null;
      return null;
    } finally {
      this.connecting[key] = false;
    }
  }

  async close(key: string) {
    const s = this.sources[key];
    delete this.sources[key];
    if (this.activeKey === key) this.activeKey = null;
    if (s) await api.containers.close(s.sessionId).catch(() => {});
  }

  setRuntime(key: string, runtime: ContainerRuntime) {
    const s = this.sources[key];
    if (!s || !CONTAINER_RUNTIMES.includes(runtime)) return;
    s.runtime = runtime;
    s.listing = null;
    s.error = null;
    void this.refresh(key);
  }

  setSizes(key: string, on: boolean) {
    const s = this.sources[key];
    if (!s) return;
    s.sizes = on;
    void this.refresh(key);
  }

  // -- refreshing ------------------------------------------------------------

  /**
   * Reload the lists. A refresh already running is not doubled up; a quiet one
   * (the timer) keeps the old list on screen if it fails, and says so only once.
   */
  async refresh(key: string, quiet = false) {
    const s = this.sources[key];
    if (!s || s.loading) return;
    s.loading = true;
    try {
      s.listing = await api.containers.list(s.sessionId, s.runtime, s.sizes);
      s.updatedAt = Date.now();
      s.error = null;
      // Volumes and networks only while someone is looking at them: it is two more commands.
      if (s.wantResources && s.runtime === "docker") {
        try {
          s.resources = await api.containers.resources(s.sessionId, s.runtime, s.sizes);
          s.resourcesError = s.resources.sizes_error;
        } catch (e) {
          s.resourcesError = errorMessage(e);
        }
      }
    } catch (e) {
      const message = errorMessage(e);
      if (!quiet || s.error !== message) s.error = message;
      if (isApiError(e) && e.code === "no_session") {
        // The session is gone (the app side closed it); reopen on the next use.
        delete this.sources[key];
        if (this.activeKey === key) void this.open(key);
      }
    } finally {
      s.loading = false;
    }
  }

  // -- actions ---------------------------------------------------------------

  async #confirmProduction(key: string, what: string): Promise<boolean> {
    const name = this.label(key);
    return ask(`${what}\n\nThis is a PRODUCTION host (${name}).`, {
      title: `Change a container on ${name}?`,
      confirm: "Continue",
      danger: true,
      requireText: name,
    });
  }

  async act(key: string, c: ContainerInfo, action: ContainerAction): Promise<boolean> {
    const s = this.sources[key];
    if (!s || this.busy[c.id]) return false;
    const prod = this.isProduction(key);
    const verb = action.action === "remove" ? "Remove" : action.action[0].toUpperCase() + action.action.slice(1);
    if (action.action === "remove") {
      const running = isLive(c.state);
      const ok = running
        ? await ask(`Remove the running container "${c.name}"? It will be stopped first, and removed with its writable layer. Volumes are kept.`, { title: "Force remove?", confirm: "Stop and remove", danger: true, requireText: prod ? this.label(key) : undefined })
        : await ask(`Remove the container "${c.name}"? Its writable layer goes with it. Volumes are kept.`, { title: "Remove container?", confirm: "Remove", danger: true, requireText: prod ? this.label(key) : undefined });
      if (!ok) return false;
      action = { action: "remove", force: running };
    } else if (prod && !(await this.#confirmProduction(key, `${verb} "${c.name}"?`))) {
      return false;
    }
    this.busy[c.id] = true;
    try {
      await api.containers.act(s.sessionId, s.runtime, action, c.id);
      await this.refresh(key);
      return true;
    } catch (e) {
      ui.notify("error", `${verb} "${c.name}": ${errorMessage(e)}`);
      await this.refresh(key);
      return false;
    } finally {
      delete this.busy[c.id];
    }
  }

  // -- images, volumes, networks -----------------------------------------------

  /** One careful question: red button, Cancel focused, and on a production host the host's name to type. */
  async #confirmChange(key: string, title: string, message: string, confirm: string): Promise<boolean> {
    const name = this.label(key);
    const prod = this.isProduction(key);
    return ask(prod ? `${message}\n\nThis is a PRODUCTION host (${name}).` : message, { title, confirm, danger: true, requireText: prod ? name : undefined });
  }

  /** Remove one image, volume or network after asking. Returns whether it went. */
  async removeResource(key: string, kind: ResourceKind, id: string, label: string): Promise<boolean> {
    const s = this.sources[key];
    const busyKey = `${kind}:${id}`;
    if (!s || this.busy[busyKey]) return false;
    const what = {
      image: `Remove the image "${label}"? If another tag still names the same image, only this tag goes. Containers keep working.`,
      volume: `Remove the volume "${label}"? Everything stored in it is deleted and can't be recovered.`,
      network: `Remove the network "${label}"?`,
    }[kind];
    if (!(await this.#confirmChange(key, `Remove ${kind}?`, what, "Remove"))) return false;
    this.busy[busyKey] = true;
    try {
      await api.containers.remove(s.sessionId, s.runtime, kind, id, false);
      await this.refresh(key);
      return true;
    } catch (e) {
      ui.notify("error", `Remove ${label}: ${errorMessage(e)}`);
      return false;
    } finally {
      delete this.busy[busyKey];
    }
  }

  /** Ask before a prune actually removes `count` items. */
  confirmPrune(key: string, what: string, count: number): Promise<boolean> {
    return this.#confirmChange(key, "Remove these?", `Remove ${count} ${what}? This can't be undone.`, `Remove ${count}`);
  }

  // -- Compose --------------------------------------------------------------------

  /** Start, stop, restart or take down every container of a Compose project. */
  async compose(key: string, project: string, verb: ComposeVerb, count: number): Promise<boolean> {
    const s = this.sources[key];
    const busyKey = `project:${project}`;
    if (!s || this.busy[busyKey]) return false;
    if (verb === "down") {
      const ok = await this.#confirmChange(
        key,
        `Take down "${project}"?`,
        `This stops and removes the ${count} container${count === 1 ? "" : "s"} of "${project}" and its networks. Its volumes, and so its data, are kept.`,
        "Take down",
      );
      if (!ok) return false;
    } else if (this.isProduction(key) && verb !== "start") {
      const name = this.label(key);
      const ok = await ask(`${verb === "stop" ? "Stop" : "Restart"} all ${count} containers of "${project}"?\n\nThis is a PRODUCTION host (${name}).`, {
        title: `Change "${project}" on ${name}?`,
        confirm: "Continue",
        danger: true,
        requireText: name,
      });
      if (!ok) return false;
    }
    this.busy[busyKey] = true;
    try {
      await api.containers.compose(s.sessionId, project, verb);
      await this.refresh(key);
      return true;
    } catch (e) {
      ui.notify("error", `${project}: ${errorMessage(e)}`);
      await this.refresh(key);
      return false;
    } finally {
      delete this.busy[busyKey];
    }
  }

  /** A terminal tab with a shell inside the container. */
  openShell(key: string, c: ContainerInfo) {
    const s = this.sources[key];
    const line = s && shellCommand(s.runtime, c.id);
    if (!s || !line) {
      ui.notify("error", "That container's name or ID can't be used in a command.");
      return;
    }
    if (!isLive(c.state)) {
      ui.notify("error", `"${c.name}" isn't running, so there's nothing to open a shell in. Start it first.`);
      return;
    }
    const title = `${c.name} · ${s.label}`;
    if (s.hostId) ui.openTerminal(s.hostId, title, line);
    else ui.openLocal(line, title);
  }
}

export const containers = new ContainersStore();
