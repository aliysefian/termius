// The Containers view's live state: which computers are open, what each one
// runs, and the actions on it. Per window; it goes away when the vault locks.
import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { isLive, shellCommand } from "$lib/containerdata";
import { StatHistory, toSample, type PortTarget, portUrl } from "$lib/containerview";
import { openUrl } from "@tauri-apps/plugin-opener";
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
  type ContainerStat,
  type ResourceKind,
  type Uuid,
} from "$lib/types";

export const LOCAL = "local";
const PAST: Record<ContainerAction["action"], string> = { start: "Started", stop: "Stopped", restart: "Restarted", pause: "Paused", unpause: "Resumed", remove: "Removed" };
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
  /** Read CPU and memory use with each refresh (one more command, about a second). */
  wantStats: boolean;
  /** By container id (full and the 12-character form the runtime may print). */
  stats: Record<string, ContainerStat>;
  statsError: string | null;
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
  /** The container whose details are open, by full id, on the active source. */
  selectedId = $state<string | null>(null);
  /** Containers ticked for a bulk action. */
  checked = $state<Set<string>>(new Set());
  /** Recent CPU and memory readings of the active source's containers; `statsVersion` says when it changed. */
  #history = new Map<string, StatHistory>();
  statsVersion = $state(0);
  /** Tunnels opened to reach a container's published port on a host: "sourceKey:hostPort" → forward. */
  portTunnels = $state<Record<string, { id: Uuid; port: number | null }>>({});

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
      this.selectedId = null;
      this.checked = new Set();
      this.#history.clear();
      this.portTunnels = {};
    }
  }

  /** Make a source the one shown (opening it if it isn't yet). What was selected on the last one is let go. */
  async switchTo(key: string): Promise<Source | null> {
    if (this.activeKey !== key) {
      this.selectedId = null;
      this.checked = new Set();
    }
    return this.open(key);
  }

  /** The recent stats readings of one container on a source. */
  history(key: string, id: string) {
    void this.statsVersion;
    return this.#history.get(key)?.get(id) ?? [];
  }

  select(id: string | null) {
    this.selectedId = id;
  }

  toggleChecked(id: string) {
    const next = new Set(this.checked);
    if (!next.delete(id)) next.add(id);
    this.checked = next;
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
        wantStats: false,
        stats: {},
        statsError: null,
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
    if (this.activeKey === key) {
      this.activeKey = null;
      this.selectedId = null;
      this.checked = new Set();
    }
    this.#history.delete(key);
    for (const [k, t] of Object.entries(this.portTunnels)) {
      if (k.startsWith(`${key}:`)) {
        void api.forwards.stop(t.id).catch(() => {});
        delete this.portTunnels[k];
      }
    }
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

  setStats(key: string, on: boolean) {
    const s = this.sources[key];
    if (!s) return;
    s.wantStats = on;
    if (!on) {
      s.stats = {};
      s.statsError = null;
    }
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
      // Resource use only while someone asked for it: it is one more command and takes about a second.
      if (s.wantStats) {
        try {
          const list = await api.containers.stats(s.sessionId, s.runtime);
          s.stats = Object.fromEntries(list.flatMap((x) => [[x.id, x], [x.id.slice(0, 12), x]]));
          s.statsError = null;
          const hist = this.#history.get(key) ?? new StatHistory();
          const at = Date.now();
          const full = new Map((s.listing?.containers ?? []).map((c) => [c.id.slice(0, 12), c.id]));
          for (const x of list) hist.push(full.get(x.id.slice(0, 12)) ?? x.id, toSample(x, at));
          hist.keep(new Set((s.listing?.containers ?? []).map((c) => c.id)));
          this.#history.set(key, hist);
          this.statsVersion++;
        } catch (e) {
          s.stats = {};
          s.statsError = errorMessage(e);
        }
      }
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
      ui.notify("info", `${PAST[action.action]} "${c.name}".`);
      if (action.action === "remove" && this.selectedId === c.id) this.selectedId = null;
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

  /**
   * Start, stop, restart or remove several containers: one question (on a production host the host's name to type),
   * then each in turn. Returns how many went through; the rest are named in one message.
   */
  async bulk(key: string, list: ContainerInfo[], verb: "start" | "stop" | "restart" | "remove"): Promise<number> {
    const s = this.sources[key];
    const todo = list.filter((c) => !this.busy[c.id]);
    if (!s || todo.length === 0) return 0;
    const names = todo.slice(0, 4).map((c) => `"${c.name}"`).join(", ") + (todo.length > 4 ? ` and ${todo.length - 4} more` : "");
    const word = { start: "Start", stop: "Stop", restart: "Restart", remove: "Remove" }[verb];
    const name = this.label(key);
    const prod = this.isProduction(key);
    const text =
      verb === "remove"
        ? `Remove ${todo.length} container${todo.length === 1 ? "" : "s"}: ${names}? Running ones are stopped first, and each is removed with its writable layer. Volumes are kept.`
        : `${word} ${todo.length} container${todo.length === 1 ? "" : "s"}: ${names}?`;
    if ((verb === "remove" || prod) && !(await ask(prod ? `${text}\n\nThis is a PRODUCTION host (${name}).` : text, { title: `${word} ${todo.length} containers?`, confirm: word, danger: verb === "remove" || prod, requireText: prod ? name : undefined }))) return 0;
    const failures: string[] = [];
    let done = 0;
    for (const c of todo) {
      this.busy[c.id] = true;
      try {
        const action: ContainerAction = verb === "remove" ? { action: "remove", force: isLive(c.state) } : { action: verb };
        await api.containers.act(s.sessionId, s.runtime, action, c.id);
        done++;
      } catch (e) {
        failures.push(`${c.name} (${errorMessage(e).split("\n")[0]})`);
      } finally {
        delete this.busy[c.id];
      }
    }
    await this.refresh(key);
    this.checked = new Set([...this.checked].filter((id) => this.sources[key]?.listing?.containers.some((c) => c.id === id)));
    if (failures.length) ui.notify("error", `${word}: ${done} of ${todo.length} done. Failed: ${failures.slice(0, 3).join("; ")}${failures.length > 3 ? ` and ${failures.length - 3} more` : ""}.`);
    else ui.notify("info", `${PAST[verb]} ${done} container${done === 1 ? "" : "s"}.`);
    return done;
  }

  // -- reaching a published port -------------------------------------------------------------------------------

  /**
   * Open a container's published port in the browser. This computer's own runtime publishes on this computer, so the
   * address is used as it is. A host's port is on the host, not here: it is reached through a tunnel (SSHVault's own
   * forwarding), opened on demand and stopped with the source.
   */
  async openPort(key: string, t: PortTarget): Promise<void> {
    const s = this.sources[key];
    if (!s) return;
    try {
      if (!s.hostId) {
        await openUrl(portUrl(t.hostPort, t.containerPort));
        return;
      }
      const tk = `${key}:${t.hostPort}`;
      let tunnel = this.portTunnels[tk];
      const live = tunnel && vaultStore.forwardStatus[tunnel.id]?.state === "active";
      if (!live) {
        const id = await api.forwards.startAdhoc(s.hostId, { kind: "local", bind_addr: "127.0.0.1", bind_port: 0, dest_host: t.address, dest_port: t.hostPort });
        tunnel = { id, port: null };
        this.portTunnels[tk] = tunnel;
        const port = await this.#waitActive(id);
        this.portTunnels[tk] = { id, port };
        tunnel = this.portTunnels[tk];
      }
      await openUrl(portUrl(tunnel.port ?? t.hostPort, t.containerPort));
    } catch (e) {
      delete this.portTunnels[`${key}:${t.hostPort}`];
      ui.notify("error", `Couldn't open port ${t.hostPort}: ${errorMessage(e)}`);
    }
  }

  /** Stop the tunnel to a port, if one was opened. */
  async closePortTunnel(key: string, hostPort: number) {
    const tk = `${key}:${hostPort}`;
    const t = this.portTunnels[tk];
    delete this.portTunnels[tk];
    if (t) await api.forwards.stop(t.id).catch(() => {});
  }

  /** The tunnel's local port once the backend reports it active (from the status events the app already receives). */
  async #waitActive(id: Uuid, timeoutMs = 20_000): Promise<number> {
    const until = Date.now() + timeoutMs;
    while (Date.now() < until) {
      const st = vaultStore.forwardStatus[id];
      if (st?.state === "active") return st.port;
      if (st?.state === "error") throw new Error(st.message);
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error("the tunnel didn't come up in time");
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
