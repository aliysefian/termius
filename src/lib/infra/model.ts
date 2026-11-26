// The Infrastructure Explorer's vocabulary: what a resource is, how it is identified, and how its data is described.
//
// The explorer reads what other parts of the app already know (saved hosts, the Containers store, the service
// manager's unit list, the tmux monitor's snapshots) and joins it. It never owns a lifecycle: a container is stopped
// in Containers, a service restarted in Ops. So everything here is a read model, and every resource says where it
// came from and when.

export type ResourceType = "host" | "docker_engine" | "container" | "port" | "systemd_service" | "tmux_session";

/** What the resource itself reports. Whether the report is current is `Freshness`, which is separate. */
export type ResourceStatus = "healthy" | "running" | "stopped" | "warning" | "critical" | "unknown" | "offline";

export type Freshness = "live" | "fresh" | "stale" | "unavailable" | "unsupported";

export type Provider = "hosts" | "docker" | "systemd" | "tmux";

export type RelationshipType = "contains" | "publishes_port";

export type Scalar = string | number | boolean | null;

export interface Resource {
  /** Stable and scoped: see the builders below. Never a bare display name. */
  id: string;
  type: ResourceType;
  name: string;
  /** The saved host it belongs to, `null` for something that is not on a saved host. */
  hostId: string | null;
  /** The discovery scope that reported it (`docker:<host>`): what a refresh replaces as a whole. */
  scopeKey: string;
  status: ResourceStatus;
  /** The status in the owning module's words ("Up 3 hours", "active (running)"). */
  statusText: string;
  /** The module that is authoritative for it. */
  source: Provider;
  /** Properties worth showing, by label. Never a secret, never a whole inspect document. */
  attributes: Record<string, Scalar>;
  /** Lower-cased text the search looks in. */
  haystack: string;
  /** What the owning module needs to open it (a full container id, a unit name). Never shown, never parsed from an id. */
  ref: Record<string, string>;
}

export interface Relationship {
  from: string;
  to: string;
  type: RelationshipType;
  source: Provider;
  observedAt: number;
  /** True when the owning module said so. Nothing is inferred from similar names, so today every edge is verified. */
  verified: boolean;
  evidence: string;
}

export interface Snapshot {
  scopeKey: string;
  provider: Provider;
  hostId: string | null;
  observedAt: number;
  resources: Resource[];
  relationships: Relationship[];
}

// -- identity ---------------------------------------------------------------------------------------------------------
// Each part is escaped so a name with a separator in it cannot make two different things one id, or one thing two.

const part = (s: string | number) => encodeURIComponent(String(s));
const join = (...parts: (string | number)[]) => parts.map(part).join(":");

export const hostKey = (hostId: string) => `host:${part(hostId)}`;
/** The runtime on a host or on this computer (`local`). Two runtimes on one host are two engines. */
export const engineKey = (source: string, runtime: string) => `engine:${join(source, runtime)}`;
/** The engine plus the full container id, which a runtime keeps unique within itself. */
export const containerKey = (source: string, runtime: string, containerId: string) => `container:${join(source, runtime, containerId)}`;
export const portKey = (containerId: string, proto: string, hostIp: string, hostPort: number) => `port:${part(containerId)}:${join(proto, hostIp || "*", hostPort)}`;
/** The unit on the system manager of a host. A user manager would be a different scope, so a different id. */
export const unitKey = (hostId: string, unit: string, manager = "system") => `unit:${join(hostId, manager, unit)}`;
/** The session on one tmux server (a host and a socket), by tmux's own session id (`$3`), which survives a rename. */
export const tmuxKey = (hostId: string, socket: string, sessionId: string) => `tmux:${join(hostId, socket, sessionId)}`;

export const scopeKeyOf = (provider: Provider, hostId?: string, extra?: string) => (provider === "hosts" ? "hosts" : `${provider}:${part(hostId ?? "")}${extra ? `|${part(extra)}` : ""}`);

export const TYPE_LABEL: Record<ResourceType, string> = {
  host: "Host",
  docker_engine: "Docker engine",
  container: "Container",
  port: "Published port",
  systemd_service: "systemd service",
  tmux_session: "Tmux session",
};

export const PROVIDER_LABEL: Record<Provider, string> = { hosts: "Hosts", docker: "Containers", systemd: "systemd", tmux: "Tmux" };

export const STATUS_LABEL: Record<ResourceStatus, string> = {
  healthy: "Healthy",
  running: "Running",
  stopped: "Stopped",
  warning: "Warning",
  critical: "Critical",
  unknown: "Unknown",
  offline: "Offline",
};

/** Text for search: one string per resource, lower-cased, whatever a person might type to find it. */
export function haystack(...bits: (string | number | null | undefined | string[])[]): string {
  return bits
    .flat()
    .filter((b): b is string | number => b !== null && b !== undefined && b !== "")
    .join(" ")
    .toLowerCase();
}

/** A name from a host, shown as one line of plain text: anything that could draw on the screen is made a space. */
export function plain(s: string, max = 200): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/[\u0000-\u001f\u007f-\u009f‪-‮⁦-⁩]/g, " ").slice(0, max).trim();
}
