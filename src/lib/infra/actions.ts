// What can be done from the explorer. Every action opens the module that owns the thing; none of them does the work.
import type { ResourceView } from "./inventory";

export type ActionId = "open-host" | "connect" | "host-metrics" | "open-container" | "open-service" | "view-logs" | "open-tmux" | "open-engine";

export interface ActionSpec {
  id: ActionId;
  label: string;
  enabled: boolean;
  /** Why it is not available, shown instead of hiding it silently. */
  reason?: string;
}

/**
 * The actions for a resource. `known` says whether the resource is one the inventory holds, so an id that came from
 * somewhere else gets none. `protocol` is the host's own ("ssh", "ftp"…).
 */
export function actionsFor(r: ResourceView | undefined, protocol: string | undefined): ActionSpec[] {
  if (!r || r.hostId === null) return [];
  const gone = r.freshness === "unsupported" ? "Not supported on this host" : null;
  switch (r.type) {
    case "host": {
      const ssh = !protocol || protocol === "ssh";
      return [
        { id: "connect", label: protocol === "ftp" ? "Open files" : "Connect terminal", enabled: protocol === "ssh" || protocol === "ftp" || !protocol, reason: protocol && !["ssh", "ftp"].includes(protocol) ? `A ${protocol} host opens from the Hosts list` : undefined },
        { id: "host-metrics", label: "View monitoring", enabled: ssh, reason: ssh ? undefined : "Monitoring reads SSH hosts" },
      ];
    }
    case "container":
    case "docker_engine":
    case "port":
      return [{ id: r.type === "docker_engine" ? "open-engine" : "open-container", label: r.type === "docker_engine" ? "Open in Containers" : "Open container", enabled: gone === null, reason: gone ?? undefined }];
    case "systemd_service":
      return [
        { id: "open-service", label: "Open service", enabled: gone === null, reason: gone ?? undefined },
        { id: "view-logs", label: "View logs", enabled: gone === null, reason: gone ?? undefined },
      ];
    case "tmux_session":
      return [{ id: "open-tmux", label: "Open Tmux Monitor", enabled: gone === null, reason: gone ?? undefined }];
  }
}
