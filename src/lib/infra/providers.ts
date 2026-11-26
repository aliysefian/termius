// Turning what each owning module reports into inventory snapshots. Each function is pure: the store reads the
// module (the saved hosts, the Containers store, the service manager, the tmux monitor) and hands the result here.

import type { ContainerInfo, ContainerRuntime, ContainerStat, Host } from "$lib/types";
import type { Unit } from "$lib/ops/services";
import { socketKey, type HostSnapshot, type Session } from "$lib/tmux/monitor";
import { healthOf } from "$lib/containerdata";
import { containerKey, engineKey, haystack, hostKey, plain, portKey, scopeKeyOf, tmuxKey, unitKey, TYPE_LABEL, type Relationship, type Resource, type ResourceStatus, type Snapshot } from "./model";

export interface HostInput {
  id: string;
  data: Host;
  /** The environment after the group's default applies. */
  environment: string;
  /** A terminal of this host is connected right now. */
  connected: boolean;
  /** When the person last opened a terminal to it, from this computer's connection log. */
  lastConnectedAt: number | null;
}

const rel = (from: string, to: string, type: Relationship["type"], source: Relationship["source"], observedAt: number, evidence: string): Relationship => ({ from, to, type, source, observedAt, verified: true, evidence });

/** Saved hosts. Reading them opens no connection. */
export function hostsSnapshot(hosts: HostInput[], now: number): Snapshot {
  const resources: Resource[] = hosts.map((h) => {
    const label = plain(h.data.label || h.data.hostname);
    const protocol = h.data.protocol || "ssh";
    return {
      id: hostKey(h.id),
      type: "host",
      name: label,
      hostId: h.id,
      scopeKey: "hosts",
      status: h.connected ? "healthy" : "unknown",
      statusText: h.connected ? "Connected" : "Not connected",
      source: "hosts",
      attributes: {
        Address: `${plain(h.data.hostname)}:${h.data.port}`,
        Protocol: protocol,
        Group: plain(h.data.group || ""),
        Environment: h.environment || null,
        Tags: h.data.tags.map((t) => plain(t, 40)).join(", ") || null,
        "Last connected": h.lastConnectedAt,
      },
      haystack: haystack(label, h.data.hostname, h.data.group, h.data.tags, h.environment, protocol, "host"),
      ref: { protocol },
    };
  });
  return { scopeKey: "hosts", provider: "hosts", hostId: null, observedAt: now, resources, relationships: [] };
}

const CONTAINER_STATUS: Record<string, ResourceStatus> = { running: "running", paused: "warning", restarting: "warning", exited: "stopped", created: "stopped", dead: "critical", removing: "warning", unknown: "unknown" };

export function containerStatus(c: Pick<ContainerInfo, "state" | "status">): ResourceStatus {
  const health = healthOf(c.status);
  if (c.state === "running" && health === "unhealthy") return "critical";
  if (c.state === "running" && health === "starting") return "warning";
  return CONTAINER_STATUS[c.state] ?? "unknown";
}

/** The containers of one engine (one runtime on one host). */
export function dockerSnapshot(hostId: string, runtime: ContainerRuntime, containers: ContainerInfo[], observedAt: number, stats: Record<string, ContainerStat> = {}): Snapshot {
  const scopeKey = scopeKeyOf("docker", hostId);
  const engine = engineKey(hostId, runtime);
  const host = hostKey(hostId);
  const resources: Resource[] = [
    {
      id: engine,
      type: "docker_engine",
      name: runtime === "docker" ? "Docker" : runtime === "podman" ? "Podman" : "nerdctl",
      hostId,
      scopeKey,
      status: "running",
      statusText: `${containers.filter((c) => c.state === "running").length} of ${containers.length} containers running`,
      source: "docker",
      attributes: { Runtime: runtime, Containers: containers.length },
      haystack: haystack(runtime, "engine", "docker"),
      ref: { runtime },
    },
  ];
  const relationships: Relationship[] = [rel(host, engine, "contains", "docker", observedAt, "the runtime answered a list command over this host's connection")];
  for (const c of containers) {
    const id = containerKey(hostId, runtime, c.id);
    const stat = stats[c.id] ?? stats[c.id.slice(0, 12)];
    const published = c.ports.filter((p) => p.host_port !== "");
    resources.push({
      id,
      type: "container",
      name: plain(c.name),
      hostId,
      scopeKey,
      status: containerStatus(c),
      statusText: plain(c.status),
      source: "docker",
      attributes: {
        Image: plain(c.image),
        "Container ID": c.id.slice(0, 12),
        State: c.state,
        Health: healthOf(c.status),
        Created: c.created === null ? null : c.created * 1000,
        Pod: c.pod ? plain(c.pod) : null,
        Networks: c.networks.map((n) => plain(n, 60)).join(", ") || null,
        "CPU %": stat?.cpu_pct ?? null,
        Memory: stat ? plain(stat.mem_usage, 60) : null,
        "Memory %": stat?.mem_pct ?? null,
      },
      haystack: haystack(c.name, c.image, c.id.slice(0, 12), c.state, c.pod, "container"),
      ref: { runtime, containerId: c.id },
    });
    relationships.push(rel(engine, id, "contains", "docker", observedAt, "listed by the runtime"));
    for (const p of published) {
      const port = Number(p.host_port);
      if (!Number.isInteger(port)) continue;
      const pid = portKey(id, p.proto, p.host_ip, port);
      resources.push({
        id: pid,
        type: "port",
        name: `${p.proto.toUpperCase()} :${port}`,
        hostId,
        scopeKey,
        status: c.state === "running" ? "running" : "stopped",
        statusText: c.state === "running" ? "Published" : "Published while running",
        source: "docker",
        attributes: { Bound: p.host_ip || "all addresses", "Container port": `${p.container_port}/${p.proto}`, Protocol: p.proto },
        haystack: haystack(String(port), p.proto, "port", c.name),
        ref: { runtime, containerId: c.id },
      });
      relationships.push(rel(id, pid, "publishes_port", "docker", observedAt, "the runtime's published-port list for this container"));
    }
  }
  return { scopeKey, provider: "docker", hostId, observedAt, resources, relationships };
}

export function unitStatus(u: Pick<Unit, "active" | "sub" | "load">): ResourceStatus {
  if (u.load === "not-found" || u.load === "error") return "warning";
  if (u.active === "failed") return "critical";
  if (u.active === "activating" || u.active === "deactivating" || u.active === "reloading") return "warning";
  if (u.active === "active") return u.sub === "exited" ? "healthy" : "running";
  if (u.active === "inactive") return "stopped";
  return "unknown";
}

/** The system manager's services on one host. Other unit kinds (sockets, timers…) are not services. */
export function systemdSnapshot(hostId: string, units: Unit[], observedAt: number): Snapshot {
  const scopeKey = scopeKeyOf("systemd", hostId);
  const host = hostKey(hostId);
  const services = units.filter((u) => u.name.endsWith(".service"));
  const resources: Resource[] = services.map((u) => ({
    id: unitKey(hostId, u.name),
    type: "systemd_service",
    name: plain(u.name),
    hostId,
    scopeKey,
    status: unitStatus(u),
    statusText: `${u.active} (${u.sub})`,
    source: "systemd",
    attributes: { Unit: plain(u.name), Description: plain(u.description) || null, Load: u.load, Active: u.active, Sub: u.sub, "Unit file": u.enabled || null, "Last change": u.since || null },
    haystack: haystack(u.name, u.description, u.active, u.sub, "service systemd"),
    ref: { unit: u.name },
  }));
  const relationships = resources.map((r) => rel(host, r.id, "contains", "systemd", observedAt, "listed by systemctl on this host"));
  return { scopeKey, provider: "systemd", hostId, observedAt, resources, relationships };
}

/** The sessions of one tmux server. No pane contents are read: names, counts and the attached flag only. */
export function tmuxSnapshot(snap: HostSnapshot): Snapshot {
  const hostId = snap.host_id;
  const sock = socketKey(snap.socket);
  const scopeKey = scopeKeyOf("tmux", hostId, sock);
  const host = hostKey(hostId);
  const sessions: Session[] = snap.sessions ?? [];
  const resources: Resource[] = sessions.map((s) => ({
    id: tmuxKey(hostId, sock, s.id),
    type: "tmux_session",
    name: plain(s.name),
    hostId,
    scopeKey,
    status: s.attached > 0 ? "running" : "stopped",
    statusText: s.attached > 0 ? `attached (${s.attached})` : "detached",
    source: "tmux",
    attributes: {
      "Session ID": s.id,
      Attached: s.attached > 0,
      Clients: s.attached,
      Windows: s.windows.length,
      Panes: s.windows.reduce((n, w) => n + w.panes.length, 0),
      Socket: sock,
      Created: s.created ? s.created * 1000 : null,
    },
    haystack: haystack(s.name, s.id, "tmux session", s.attached > 0 ? "attached" : "detached"),
    ref: { sessionId: s.id, socket: sock },
  }));
  const relationships = resources.map((r) => rel(host, r.id, "contains", "tmux", snap.taken_at_ms, "listed by tmux on this host"));
  return { scopeKey, provider: "tmux", hostId, observedAt: snap.taken_at_ms, resources, relationships };
}

/** Why a tmux snapshot is not a list of sessions, or `null` when it is (an empty server is a list of none). */
export function tmuxUnsupported(snap: HostSnapshot): string | null {
  switch (snap.state.kind) {
    case "not_installed":
      return "tmux is not installed on this host";
    case "unsupported":
      return `tmux ${snap.state.version} is too old to read`;
    case "permission_denied":
      return "this account may not use that tmux socket";
    default:
      return null;
  }
}

export const typeLabel = (t: Resource["type"]) => TYPE_LABEL[t];
