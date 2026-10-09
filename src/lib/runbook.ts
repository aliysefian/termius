// Runbooks in the window: the shapes the backend sends (src-tauri/src/runbook.rs), the example runbooks, and
// what a run looks like while it is going. The checking and the running are the backend's.

export interface RunbookParam {
  name: string;
  label: string;
  default: string | null;
  optional: boolean;
  choices: string[];
  kind: "text" | "file";
}

export interface Problem {
  step: number | null;
  message: string;
}

export interface RunbookCheck {
  name: string | null;
  description: string;
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
  kind: "run" | "wait" | "upload";
  text: string;
  condition: string | null;
  on_error: "stop" | "continue";
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

export type RunbookEvent =
  | { event: "host_started"; host_id: string }
  | { event: "step_started"; host_id: string; index: number }
  | { event: "step"; host_id: string; result: StepResult }
  | { event: "host_done"; host_id: string; ok: boolean; error: string | null }
  | { event: "done"; cancelled: boolean };

export interface HostRecord {
  host_id: string;
  label: string;
  ok: boolean | null;
  error: string | null;
  steps: StepResult[];
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
}

/** What one host is doing in a run that is going. */
export interface HostRun {
  hostId: string;
  label: string;
  state: "waiting" | "running" | "ok" | "failed";
  /** The step running now, if any. */
  current: number | null;
  steps: StepResult[];
  error: string | null;
}

/** Fold an event into the hosts' state. Pure, so the live view and a test use the same rules. */
export function applyEvent(hosts: HostRun[], e: RunbookEvent): HostRun[] {
  if (e.event === "done") return hosts;
  return hosts.map((h) => {
    if (h.hostId !== e.host_id) return h;
    switch (e.event) {
      case "host_started":
        return { ...h, state: "running" };
      case "step_started":
        return { ...h, current: e.index };
      case "step":
        return { ...h, steps: [...h.steps, e.result], current: h.current === e.result.index ? null : h.current };
      case "host_done":
        return { ...h, state: e.ok ? "ok" : "failed", current: null, error: e.error };
    }
  });
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
