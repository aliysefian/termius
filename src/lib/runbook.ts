// Runbooks in the window: the shapes the backend sends (src-tauri/src/runbook.rs), the example runbooks, and
// what a run looks like while it is going. The checking and the running are the backend's.

export type ParamKind = "text" | "number" | "boolean" | "file" | "dir" | "secret";

export interface RunbookParam {
  name: string;
  label: string;
  description?: string;
  default: string | null;
  optional: boolean;
  choices: string[];
  kind: ParamKind;
}

export interface Problem {
  step: number | null;
  /** Where the parser gave up, for a document that couldn't be read (1-based). */
  line?: number | null;
  column?: number | null;
  message: string;
}

export type OnError = "stop" | "continue";
export type Notify = "always" | "failure" | "never";

/** How a run takes its hosts (src-tauri/src/runbook.rs `Strategy`). */
export type Strategy =
  | "parallel"
  | { concurrent: { limit: number; stop_on_failure?: boolean } }
  | { sequential: { stop_on_failure?: boolean } }
  | { rolling: { batch_size: number; concurrency?: number | null; stop_on_failure?: boolean } };

export interface RunbookDefaults {
  timeout_secs?: number | null;
  retries?: number | null;
  retry_delay_secs?: number | null;
  on_error?: OnError | null;
  strategy?: Strategy | null;
  hosts?: string[];
  notify?: Notify | null;
}

export interface RunbookCheck {
  name: string | null;
  description: string;
  category?: string | null;
  defaults?: RunbookDefaults | null;
  /** How the document is written. */
  format?: "json" | "yaml";
  params: RunbookParam[];
  steps: number;
  /** Steps that run only on a host where the run failed, when a run is started with rollback on. */
  rollback_steps: number;
  problems: Problem[];
}

export interface PlannedStep {
  phase: "run" | "rollback";
  index: number;
  name: string;
  kind: "run" | "wait" | "upload" | "download" | "delay" | "http" | "tcp" | "docker";
  text: string;
  condition: string | null;
  on_error: "stop" | "continue";
  /** Switched off: it will be skipped. */
  disabled?: boolean;
  /** Acts on this computer, not on the host. */
  local?: boolean;
}

export type StepStatus = "ok" | "failed" | "skipped" | "timed_out" | "error";

export interface Output {
  stdout: string;
  stderr: string;
  exit_code: number | null;
  truncated: boolean;
  duration_ms: number;
}

export interface StepResult {
  /** Missing in records made before rollback existed: those are all "run". */
  phase?: "run" | "rollback";
  index: number;
  name: string;
  status: StepStatus;
  output: Output;
  note: string;
}

export type HostStatus = "pending" | "connecting" | "running" | "success" | "failed" | "skipped" | "cancelled" | "timed_out";

export type RunbookEvent =
  | { event: "host_started"; host_id: string }
  | { event: "connected"; host_id: string }
  | { event: "step_started"; host_id: string; index: number }
  | { event: "output"; host_id: string; index: number; stream: "stdout" | "stderr"; text: string; seq: number }
  | { event: "step"; host_id: string; result: StepResult }
  | { event: "host_done"; host_id: string; ok: boolean; error: string | null; status?: HostStatus; duration_ms?: number }
  | { event: "done"; cancelled: boolean };

export interface HostRecord {
  host_id: string;
  label: string;
  ok: boolean | null;
  error: string | null;
  steps: StepResult[];
  status?: HostStatus;
  started_ms?: number;
  finished_ms?: number;
}

export interface RunRecord {
  id: string;
  runbook: string;
  started_at: number;
  finished_at: number | null;
  scheduled: boolean;
  /** Rollback was on for this run. */
  rollback?: boolean;
  cancelled: boolean;
  params: Record<string, string>;
  hosts: HostRecord[];
  strategy?: Strategy;
  /** "manual", "schedule" or "retry". */
  trigger?: string;
  retry_of?: string;
  from_step?: number;
  step_names?: string[];
}

export interface RunSummary {
  id: string;
  runbook: string;
  started_at: number;
  scheduled: boolean;
  cancelled: boolean;
  running: boolean;
  hosts: number;
  ok: number;
  failed: number;
  finished_at?: number | null;
  trigger?: string | null;
  strategy?: Strategy | null;
  failed_hosts?: string[];
}

export function statusLabel(s: StepStatus): string {
  return { ok: "done", failed: "failed", skipped: "skipped", timed_out: "timed out", error: "error" }[s];
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  return s < 60 ? `${s.toFixed(1)} s` : `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`;
}

export const EXAMPLES: { label: string; body: string }[] = [
  {
    label: "Restart a service and wait for it",
    body: JSON.stringify(
      {
        name: "Restart a service",
        description: "Restarts a systemd service and waits until it is active again.",
        params: [{ name: "service", label: "Service", default: "nginx" }],
        steps: [
          { name: "Is it running now?", id: "before", run: "systemctl is-active {{service|q}}", on_error: "continue" },
          { name: "Restart", run: "sudo systemctl restart {{service|q}}" },
          { name: "Wait until it is active", wait: { run: "systemctl is-active {{service|q}}", every_secs: 2, timeout_secs: 60 } },
        ],
      },
      null,
      2,
    ),
  },
  {
    label: "Check disk space and clean up if it is low",
    body: JSON.stringify(
      {
        name: "Disk check",
        description: "Shows disk use, and only if a mount is over the limit, cleans the package cache.",
        params: [{ name: "limit", label: "Clean up above this percent", default: "85" }],
        steps: [
          { name: "Disk use", run: "df -h --output=target,pcent | tail -n +2" },
          { name: "Is any mount over the limit?", id: "full", run: "df --output=pcent | tail -n +2 | tr -d ' %' | awk -v l={{limit|q}} '$1 > l { found=1 } END { exit found ? 0 : 1 }'", on_error: "continue" },
          { name: "Clean the package cache", run: "sudo apt-get clean", when: { step: "full", exit: 0 } },
        ],
      },
      null,
      2,
    ),
  },
  {
    label: "Upload a file and reload",
    body: JSON.stringify(
      {
        name: "Deploy a config file",
        description: "Uploads a file you choose when you run it, checks it, and reloads the service.",
        params: [
          { name: "config", label: "Config file", kind: "file" },
          { name: "remote", label: "Where it goes", default: "/etc/myapp/app.conf" },
        ],
        steps: [
          { name: "Upload", upload: { local: "{{config}}", remote: "/tmp/app.conf.new", mode: "0644" } },
          { name: "Check it", run: "myapp --check-config /tmp/app.conf.new" },
          { name: "Put it in place", run: "sudo mv /tmp/app.conf.new {{remote|q}}" },
          { name: "Reload", run: "sudo systemctl reload myapp" },
        ],
      },
      null,
      2,
    ),
  },
];
