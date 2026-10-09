// Kubernetes in the window: checking names before they go anywhere, reading a pod's state, and the
// command that opens a shell in one. Pure, so it is tested without a cluster.
import { shq } from "./ops/quote";

/** A namespace, pod, container or service name, as Kubernetes allows. */
export const K8S_NAME = /^[a-z0-9]([a-z0-9.-]{0,251}[a-z0-9])?$/;
/** A kubeconfig context: free-form, but nothing a shell or an option parser could take for something else. */
export const K8S_CONTEXT = /^[A-Za-z0-9][A-Za-z0-9_.:/@=,+-]{0,252}$/;

export const isName = (s: string) => K8S_NAME.test(s);
export const isContext = (s: string) => K8S_CONTEXT.test(s);

export interface KubePod {
  name: string;
  namespace: string;
  status: string;
  ready: string;
  restarts: number;
  node: string | null;
  ip: string | null;
  created: string | null;
  containers: string[];
  owner: string | null;
}

export interface KubeInfo {
  version: string;
  contexts: string[];
  current: string | null;
}

export type Scope = { scope: "all" } | { scope: "namespace"; name: string };

/** The kinds besides pods that can be listed (read-only; Secrets are deliberately not among them). Backend: `kube::Kind`. */
export const RESOURCE_KINDS = [
  { value: "deployments", label: "Deployments" },
  { value: "stateful_sets", label: "StatefulSets" },
  { value: "daemon_sets", label: "DaemonSets" },
  { value: "services", label: "Services" },
  { value: "config_maps", label: "ConfigMaps" },
  { value: "jobs", label: "Jobs" },
  { value: "cron_jobs", label: "CronJobs" },
  { value: "ingresses", label: "Ingresses" },
  { value: "events", label: "Events" },
  { value: "nodes", label: "Nodes" },
] as const;
export type ResourceKind = (typeof RESOURCE_KINDS)[number]["value"];

export interface KubeResource {
  name: string;
  /** Empty for a node. */
  namespace: string;
  status: string;
  created: string | null;
  details: { label: string; value: string }[];
}

/** Whether a listed resource contains the search text in its name, status or any column. */
export function matchesResource(r: KubeResource, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [r.name, r.namespace, r.status, ...r.details.map((d) => d.value)].some((x) => x.toLowerCase().includes(q));
}

/** A colour for a resource's status line. */
export function resourceTone(kind: ResourceKind, r: Pick<KubeResource, "status">): Tone {
  const s = r.status;
  if (kind === "events") return s === "Warning" ? "warn" : "muted";
  if (kind === "nodes") return s === "Ready" ? "good" : "bad";
  if (kind === "jobs") return s === "Failed" ? "bad" : s === "Complete" ? "good" : "warn";
  if (kind === "cron_jobs") return s === "Suspended" ? "muted" : "good";
  const m = /^(\d+)\/(\d+) ready$/.exec(s);
  if (m) return m[2] === "0" || m[1] === m[2] ? "good" : Number(m[1]) === 0 ? "bad" : "warn";
  return "muted";
}

export type Tone = "good" | "warn" | "bad" | "muted";

const BAD = /^(CrashLoopBackOff|Error|ErrImagePull|ImagePullBackOff|InvalidImageName|CreateContainerConfigError|CreateContainerError|OOMKilled|Evicted|Failed|ContainerCannotRun|DeadlineExceeded|RunContainerError)$/;
const WARN = /^(Pending|ContainerCreating|PodInitializing|Terminating|Unknown|NotReady|Init:.*)$/;

/** A colour for a pod's status. Running is good only when every container is ready. */
export function toneOf(p: Pick<KubePod, "status" | "ready">): Tone {
  if (BAD.test(p.status)) return "bad";
  if (WARN.test(p.status)) return "warn";
  if (p.status === "Succeeded" || p.status === "Completed") return "muted";
  if (p.status === "Running") {
    const [a, b] = p.ready.split("/").map(Number);
    return a === b ? "good" : "warn";
  }
  return "muted";
}

/** "5m", "3h", "12d": how long ago, in the one unit that fits (what `kubectl get pods` shows as AGE). */
export function ageOf(created: string | null, now = Date.now()): string {
  if (!created) return "—";
  const t = Date.parse(created);
  if (Number.isNaN(t)) return "—";
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 90) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 120) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h`;
  return `${Math.round(h / 24)}d`;
}

export function matches(p: KubePod, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [p.name, p.namespace, p.status, p.node ?? "", p.owner ?? ""].some((x) => x.toLowerCase().includes(q));
}

/**
 * The line typed into a terminal to get a shell in a pod: bash where there is one, `sh` where there isn't.
 * Null when a name isn't one, so nothing odd is ever typed.
 */
export function shellLine(context: string, namespace: string, pod: string, container?: string): string | null {
  if (!isContext(context) || !isName(namespace) || !isName(pod) || (container !== undefined && !isName(container))) return null;
  const c = container ? ` -c ${container}` : "";
  return `kubectl --context ${shq(context)} -n ${namespace} exec -it ${pod}${c} -- sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'`;
}

export function portProblem(v: string): string | null {
  if (!/^\d{1,5}$/.test(v)) return "A port is a number from 1 to 65535.";
  const n = Number(v);
  return n >= 1 && n <= 65535 ? null : "A port is a number from 1 to 65535.";
}
