// The Infrastructure Explorer's state. It reads what the owning modules already hold (saved hosts, the Containers
// store, the tmux monitor's snapshots) and, when a host is opened or refreshed, asks those same modules for more:
// there is no second connection manager, container client or tmux poller here, and nothing runs on a timer except
// the clock the ages are drawn from.
import * as api from "$lib/api";
import { Inventory, NO_FILTERS, search, summarize, type Filters, type ResourceView } from "$lib/infra/inventory";
import { hostKey, scopeKeyOf } from "$lib/infra/model";
import { dockerSnapshot, hostsSnapshot, systemdSnapshot, tmuxSnapshot, tmuxUnsupported, type HostInput } from "$lib/infra/providers";
import { buildRows, groupKey, type Row } from "$lib/infra/tree";
import { neighborhood, parentsOf } from "$lib/infra/graph";
import { actionsFor, type ActionId } from "$lib/infra/actions";
import { runBounded, SingleFlight } from "$lib/infra/coordinator";
import { parseUnits, LIST_SCRIPT } from "$lib/ops/services";
import { LOCAL, containers } from "$lib/stores/containers.svelte";
import { connectionLog } from "$lib/stores/connectionlog.svelte";
import { hostSamples } from "$lib/stores/hostsamples.svelte";
import { ops } from "$lib/stores/ops.svelte";
import { tmuxMonitor } from "$lib/stores/tmuxmonitor.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage } from "$lib/types";
import type { UnlistenFn } from "@tauri-apps/api/event";

/** How many hosts are read at once when the whole fleet is refreshed. */
export const DISCOVERY_CONCURRENCY = 4;

const isSsh = (protocol?: string) => !protocol || protocol === "ssh";

class InfraStore {
  #inv = new Inventory();
  /** Bumped on every change to the inventory, so what is derived from it is drawn again. */
  version = $state(0);
  now = $state(Date.now());
  filters = $state<Filters>({ ...NO_FILTERS });
  expanded = $state<Set<string>>(new Set());
  selectedId = $state<string | null>(null);
  /** Hosts being read right now. */
  busy = $state<Record<string, boolean>>({});
  discovering = $state(false);
  view = $state<"graph" | "list">("graph");
  tab = $state<"overview" | "relationships" | "metrics">("overview");
  #flight = new SingleFlight();
  #changes = 0;
  /** Writes `version` without reading it, so an effect that feeds the inventory does not depend on what it changes. */
  #bump() {
    this.version = ++this.#changes;
  }
  #abort: AbortController | null = null;
  #unlisten: UnlistenFn | null = null;
  #clock: ReturnType<typeof setInterval> | null = null;

  constructor() {
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked) this.reset();
      });
    });
  }

  reset() {
    this.cancel();
    this.#inv.clear();
    this.selectedId = null;
    this.expanded = new Set();
    this.busy = {};
    this.#bump();
  }

  // -- reading ----------------------------------------------------------------------------------------------------

  views = $derived.by<ResourceView[]>(() => {
    void this.version;
    return this.#inv.resources(this.now);
  });
  edges = $derived.by(() => {
    void this.version;
    void this.now;
    return this.#inv.relationships(this.now);
  });
  byId = $derived(new Map(this.views.map((v) => [v.id, v])));
  selected = $derived(this.selectedId ? (this.byId.get(this.selectedId) ?? null) : null);
  hostIds = $derived(vaultStore.hosts.filter((h) => h.data && !h.deleted && isSsh(h.data.protocol)).map((h) => h.id));
  counts = $derived.by(() => {
    void this.version;
    return summarize(this.#inv, this.now, this.hostIds);
  });
  rows = $derived.by<Row[]>(() => {
    void this.version;
    return buildRows({ inv: this.#inv, views: this.views, filters: this.filters, envOf: (h) => this.envOf(h), expanded: this.expanded, now: this.now });
  });
  results = $derived.by(() => (this.filters.query.trim() ? search(this.views, this.filters, (h) => this.envOf(h)) : []));
  graph = $derived.by(() => {
    if (!this.selectedId) return null;
    return neighborhood(this.views, this.edges, this.selectedId, 2, 40);
  });
  parents = $derived(this.selectedId ? parentsOf(this.selectedId, this.edges).map((id) => this.byId.get(id)).filter((v): v is ResourceView => !!v) : []);
  children = $derived(this.selectedId ? this.edges.filter((e) => e.from === this.selectedId).map((e) => this.byId.get(e.to)).filter((v): v is ResourceView => !!v) : []);
  /** The filters can match nothing yet because most hosts have not been read: the screen says so. */
  coverage = $derived.by(() => {
    const s = this.counts;
    return { read: Math.max(s.containers.hostsObserved, s.services.hostsObserved, s.tmux.hostsObserved), total: this.hostIds.length };
  });

  envOf(hostId: string | null): string {
    return hostId ? vaultStore.effectiveEnv(vaultStore.hostById.get(hostId)?.data) || "" : "";
  }
  protocolOf(hostId: string | null): string | undefined {
    return hostId ? (vaultStore.hostById.get(hostId)?.data?.protocol ?? "ssh") : undefined;
  }
  hostLabel(hostId: string | null): string {
    return hostId ? (vaultStore.hostById.get(hostId)?.data?.label ?? "host") : "";
  }
  actions = $derived(actionsFor(this.selected ?? undefined, this.protocolOf(this.selected?.hostId ?? null)));
  /** CPU and memory for the selected resource, from the module that already reads them; `null` when nothing was read. */
  metrics = $derived.by(() => {
    const r = this.selected;
    if (!r) return null;
    if (r.type === "host" && r.hostId) {
      const l = hostSamples.latest[r.hostId];
      if (!l) return null;
      return { kind: "host" as const, at: l.taken_at_ms, cpu: l.cpu?.total_pct ?? null, memPct: l.memory?.used_pct ?? null, load: l.load?.m1 ?? null };
    }
    if (r.type === "container") {
      const cpu = r.attributes["CPU %"];
      const mem = r.attributes["Memory"];
      if (cpu === null && mem === null) return null;
      return { kind: "container" as const, at: r.observedAt ?? 0, cpu: cpu as number | null, mem: mem as string | null, memPct: (r.attributes["Memory %"] as number | null) ?? null };
    }
    return null;
  });

  // -- feeding the inventory ----------------------------------------------------------------------------------------

  /** Saved hosts and which are connected. Pure reads: call it from an effect and it follows the vault and the panes. */
  syncHosts() {
    const connected = new Set<string>();
    for (const t of ui.tabs) for (const p of t.panes) if (p.target.kind === "host" && ui.paneInfo[p.id]?.status === "connected") connected.add(p.target.hostId);
    const inputs: HostInput[] = vaultStore.hosts
      .filter((h) => h.data && !h.deleted)
      .map((h) => ({
        id: h.id,
        data: h.data!,
        environment: vaultStore.effectiveEnv(h.data) || "",
        connected: connected.has(h.id),
        lastConnectedAt: connectionLog.forHost(h.id, 1)[0]?.startedAt ?? null,
      }));
    const known = new Set(inputs.map((i) => i.id));
    for (const s of [...this.#inv.scopes.values()]) if (s.hostId && !known.has(s.hostId)) this.#inv.forgetHost(s.hostId);
    this.#inv.apply(hostsSnapshot(inputs, Date.now()), Date.now());
    this.#bump();
  }

  /** What the Containers view already holds, for hosts it has open. No command is sent. */
  syncContainers() {
    const now = Date.now();
    for (const s of Object.values(containers.sources)) {
      if (!s.hostId || s.key === LOCAL) continue;
      const scope = scopeKeyOf("docker", s.hostId);
      if (s.detected.length === 0) this.#inv.markUnsupported(scope, "docker", s.hostId, "No container runtime (docker, podman or nerdctl) was found on this host", now);
      else if (s.listing) this.#inv.apply(dockerSnapshot(s.hostId, s.runtime, s.listing.containers, s.updatedAt || now, s.stats), now);
      else if (s.error) this.#inv.fail(scope, "docker", s.hostId, s.error, now);
    }
    this.#bump();
  }

  /** The tmux monitor's cache (it never connects) and its own change events. */
  async listenTmux() {
    try {
      for (const snap of await api.tmuxMonitor.latest()) this.#ingestTmux(snap);
      this.#unlisten ??= await api.tmuxMonitor.onSnapshot((s) => this.#ingestTmux(s));
    } catch {
      // No cache to read: the tmux group stays "not read yet".
    }
  }

  #ingestTmux(snap: import("$lib/tmux/monitor").HostSnapshot) {
    const now = Date.now();
    const why = tmuxUnsupported(snap);
    const scope = scopeKeyOf("tmux", snap.host_id, snapshotSocket(snap));
    if (why) this.#inv.markUnsupported(scope, "tmux", snap.host_id, why, now);
    else if (snap.stale) this.#inv.fail(scope, "tmux", snap.host_id, "the host could not be reached", now);
    else this.#inv.apply(tmuxSnapshot(snap), now);
    this.#bump();
  }

  start() {
    this.now = Date.now();
    // Only the age text moves with this; it asks nothing of any host.
    this.#clock ??= setInterval(() => (this.now = Date.now()), 10_000);
    void this.listenTmux();
  }

  stop() {
    if (this.#clock) clearInterval(this.#clock);
    this.#clock = null;
    this.#unlisten?.();
    this.#unlisten = null;
  }

  // -- discovery ----------------------------------------------------------------------------------------------------

  /** Read one host: its containers, services and tmux sessions, side by side. One at a time per host; a repeat joins it. */
  discoverHost(hostId: string): Promise<void> {
    return this.#flight.run(hostId, async () => {
      const host = vaultStore.hostById.get(hostId)?.data;
      if (!host) return;
      this.busy[hostId] = true;
      const now = Date.now();
      try {
        if (!isSsh(host.protocol)) {
          const why = `A ${host.protocol} host has no shell to read`;
          for (const p of ["docker", "systemd", "tmux"] as const) this.#inv.markUnsupported(scopeKeyOf(p, hostId, p === "tmux" ? "default" : undefined), p, hostId, why, now);
          return;
        }
        await Promise.all([this.#readContainers(hostId), this.#readServices(hostId), this.#readTmux(hostId)]);
      } finally {
        delete this.busy[hostId];
        this.#bump();
      }
    });
  }

  async #readContainers(hostId: string) {
    const prev = containers.activeKey;
    const existed = !!containers.sources[hostId];
    try {
      // The Containers store owns the connection; opening it here is what opening Containers on this host would do.
      const s = await containers.open(hostId);
      if (!s) return this.#inv.fail(scopeKeyOf("docker", hostId), "docker", hostId, "the host could not be reached", Date.now());
      if (existed) await containers.refresh(hostId, true);
      this.syncContainers();
    } finally {
      // Reading must not change which host the Containers view is showing.
      containers.activeKey = prev;
    }
  }

  async #readServices(hostId: string) {
    const scope = scopeKeyOf("systemd", hostId);
    let session: string | null = null;
    try {
      session = await api.monitor.open(hostId);
      const out = await api.monitor.exec(session, LIST_SCRIPT, 30);
      const parsed = parseUnits(out.stdout);
      const now = Date.now();
      if (!parsed.systemd) this.#inv.markUnsupported(scope, "systemd", hostId, "systemd is not running on this host", now);
      else this.#inv.apply(systemdSnapshot(hostId, parsed.units, now), now);
    } catch (e) {
      this.#inv.fail(scope, "systemd", hostId, errorMessage(e), Date.now());
    } finally {
      if (session) await api.monitor.close(session).catch(() => {});
    }
  }

  async #readTmux(hostId: string) {
    try {
      this.#ingestTmux(await api.tmuxMonitor.snapshot(hostId, { kind: "default" }, 5000));
    } catch (e) {
      this.#inv.fail(scopeKeyOf("tmux", hostId, "default"), "tmux", hostId, errorMessage(e), Date.now());
    }
  }

  /** Every saved SSH host, a few at a time, so a slow or dead one holds up only itself. */
  async discoverAll() {
    if (this.discovering) return;
    this.discovering = true;
    this.#abort = new AbortController();
    try {
      await runBounded(this.hostIds, DISCOVERY_CONCURRENCY, (id) => this.discoverHost(id), this.#abort.signal);
    } finally {
      this.discovering = false;
      this.#abort = null;
    }
  }

  /** Stops starting new hosts. A host already being read finishes. */
  cancel() {
    this.#abort?.abort();
  }

  /** Refresh what the person is looking at: the selected resource's host, or every host when nothing is selected. */
  async refresh() {
    const host = this.selected?.hostId;
    if (host) await this.discoverHost(host);
    else await this.discoverAll();
  }

  // -- interaction ----------------------------------------------------------------------------------------------------

  select(id: string | null) {
    this.selectedId = id;
    this.tab = "overview";
  }

  toggle(key: string, hostId?: string | null) {
    const next = new Set(this.expanded);
    if (next.has(key)) next.delete(key);
    else {
      next.add(key);
      // Opening a host that was never read reads it; this is the "lazy" in lazy discovery.
      if (hostId && key === hostKey(hostId) && !this.#inv.scopesOf(hostId).length && isSsh(this.protocolOf(hostId))) void this.discoverHost(hostId);
    }
    this.expanded = next;
  }

  /** Make a resource visible in the tree: its host and its group open, and it is selected. */
  reveal(id: string) {
    const r = this.byId.get(id);
    if (!r?.hostId) return;
    const next = new Set(this.expanded);
    next.add(hostKey(r.hostId));
    next.add(groupKey(r.hostId, r.source === "docker" ? "docker" : r.source === "systemd" ? "systemd" : "tmux"));
    this.expanded = next;
    this.select(id);
  }

  /** Run an action by opening the module that owns the resource. Only an id of a resource held here does anything. */
  async act(action: ActionId) {
    let r = this.selected;
    if (!r?.hostId) return;
    const spec = this.actions.find((a) => a.id === action);
    if (!spec?.enabled) return;
    const hostId = r.hostId;
    if (r.type === "port") {
      const parent = this.parents.at(-1);
      if (parent?.type === "container") r = parent;
    }
    switch (action) {
      case "connect":
        ui.openTerminal(hostId, this.hostLabel(hostId));
        break;
      case "host-metrics":
        ui.modal = { kind: "host-metrics", id: hostId };
        break;
      case "open-container":
      case "open-engine":
        ui.view = "containers";
        await containers.switchTo(hostId);
        containers.select(action === "open-container" ? (r.ref.containerId ?? null) : null);
        break;
      case "open-service":
        if (r.ref.unit) ops.viewService(hostId, r.ref.unit);
        break;
      case "view-logs":
        if (r.ref.unit) void ops.viewLogs(hostId, r.ref.unit);
        break;
      case "open-tmux":
        tmuxMonitor.addHost(hostId);
        ui.view = "tmux";
        break;
    }
  }
}

function snapshotSocket(snap: import("$lib/tmux/monitor").HostSnapshot): string {
  const s = snap.socket;
  return s.kind === "default" ? "default" : s.kind === "name" ? `L:${s.value}` : `S:${s.value}`;
}

export const infra = new InfraStore();
