// What the Builder knows about each kind of step: its name, its fields, and how to say what it does in a line. The backend's
// checking (runbook_check) is the authority; this only says which fields to draw, with what help text, and which keys the YAML
// editor offers where.
import type { Json, Obj, StepKind } from "./runbookdoc";

export type FieldType = "text" | "multiline" | "number" | "select" | "bool" | "variable";

export interface Field {
  key: string;
  label: string;
  type: FieldType;
  help?: string;
  placeholder?: string;
  required?: boolean;
  /** For a select: the values and how they read. */
  options?: { value: string; label: string }[];
  min?: number;
  max?: number;
  /** For a variable picker: which kinds of variable fit. */
  variableKinds?: ("file" | "dir")[];
  mono?: boolean;
}

export interface KindInfo {
  kind: StepKind;
  label: string;
  /** What it is for, in a sentence, for the "add a step" menu. */
  about: string;
  /** The name of a lucide icon. */
  icon: string;
  /** Acts on this computer, not on the host. */
  local: boolean;
  fields: Field[];
}

const secs = (key: string, label: string, help: string, max = 3600): Field => ({ key, label, type: "number", help, min: 1, max });

export const KINDS: KindInfo[] = [
  {
    kind: "run",
    label: "SSH command",
    about: "Run a command on each host.",
    icon: "Terminal",
    local: false,
    fields: [{ key: "run", label: "Command", type: "multiline", required: true, mono: true, help: "Runs through the host's shell. {{name}} puts in a variable; {{name|q}} quotes it for the shell. {{host}} and {{label}} are the host's." }],
  },
  {
    kind: "wait",
    label: "Wait until a command passes",
    about: "Run a command again and again until it succeeds (or prints something), up to a time limit.",
    icon: "Hourglass",
    local: false,
    fields: [
      { key: "run", label: "Command", type: "multiline", required: true, mono: true },
      { key: "until_exit", label: "Ready when it exits with", type: "number", min: 0, max: 255, help: "0 unless you say otherwise." },
      { key: "contains", label: "…and prints", type: "text", help: "Optional text its output must contain." },
      secs("every_secs", "Try every (seconds)", "Default 5."),
      secs("timeout_secs", "Give up after (seconds)", "Default 60. The step fails if it isn't ready by then."),
    ],
  },
  {
    kind: "upload",
    label: "Upload a file",
    about: "Copy a file you choose when the runbook runs to each host.",
    icon: "Upload",
    local: true,
    fields: [
      { key: "local", label: "Local file", type: "variable", variableKinds: ["file"], required: true, help: "A variable of kind \"file\": the file is chosen when the runbook runs." },
      { key: "remote", label: "Remote path", type: "text", required: true, mono: true, help: "Written beside the target and moved into place, so a failed upload leaves no half file." },
      { key: "mode", label: "Permissions", type: "text", placeholder: "0644", mono: true, help: "Octal, like 0644." },
    ],
  },
  {
    kind: "download",
    label: "Download a file",
    about: "Copy a file from each host into a folder you choose when the runbook runs.",
    icon: "Download",
    local: true,
    fields: [
      { key: "remote", label: "Remote path", type: "text", required: true, mono: true, help: "Up to 8 MB." },
      { key: "to", label: "Save into", type: "variable", variableKinds: ["dir"], required: true, help: "A variable of kind \"dir\". The file is saved as <host>__<name> and never anywhere else." },
      { key: "on_conflict", label: "If the file exists", type: "select", options: [{ value: "rename", label: "Keep both (add a number)" }, { value: "skip", label: "Leave the existing file" }, { value: "overwrite", label: "Replace it" }] },
    ],
  },
  {
    kind: "delay",
    label: "Pause",
    about: "Wait a fixed time.",
    icon: "Timer",
    local: false,
    fields: [secs("secs", "Seconds", "Up to an hour. To wait for something to be ready, use \"Wait until a command passes\" instead.")],
  },
  {
    kind: "http",
    label: "HTTP health check",
    about: "Ask a web address from this computer and check its status and content.",
    icon: "Globe",
    local: true,
    fields: [
      { key: "url", label: "Address", type: "text", required: true, mono: true, placeholder: "https://{{host}}/health", help: "Asked from this computer, so it shows what a person on this network sees." },
      { key: "method", label: "Method", type: "select", options: [{ value: "get", label: "GET" }, { value: "head", label: "HEAD" }] },
      { key: "expect_status", label: "Expected status", type: "number", min: 0, max: 599, help: "Empty or 0 accepts any 2xx or 3xx." },
      { key: "contains", label: "Page contains", type: "text" },
      secs("timeout_secs", "Wait for each answer (seconds)", "Default 10.", 120),
      secs("every_secs", "Ask again every (seconds)", "Only with a time limit below. Default 5."),
      secs("wait_secs", "Keep trying for (seconds)", "Without it the address is asked once."),
    ],
  },
  {
    kind: "tcp",
    label: "TCP port check",
    about: "Open a connection to a port from this computer.",
    icon: "Plug",
    local: true,
    fields: [
      { key: "host", label: "Host", type: "text", mono: true, help: "The host the step runs on, when left empty." },
      { key: "port", label: "Port", type: "number", required: true, min: 1, max: 65535 },
      secs("timeout_secs", "Wait for the connection (seconds)", "Default 10.", 120),
      secs("every_secs", "Try again every (seconds)", "Only with a time limit below. Default 5."),
      secs("wait_secs", "Keep trying for (seconds)", "Without it the port is tried once."),
    ],
  },
  {
    kind: "docker",
    label: "Docker operation",
    about: "Start, stop, restart or pull on the host, or run a Compose stack.",
    icon: "Container",
    local: false,
    fields: [
      {
        key: "action",
        label: "Action",
        type: "select",
        required: true,
        options: [
          { value: "start", label: "Start a container" },
          { value: "stop", label: "Stop a container" },
          { value: "restart", label: "Restart a container" },
          { value: "pull", label: "Pull an image" },
          { value: "exec", label: "Run a command in a container" },
          { value: "compose_up", label: "Compose up (detached)" },
          { value: "compose_down", label: "Compose down" },
        ],
      },
      { key: "container", label: "Container", type: "text", mono: true },
      { key: "image", label: "Image", type: "text", mono: true, placeholder: "nginx:1.27" },
      { key: "file", label: "Compose file on the host", type: "text", mono: true },
      { key: "project", label: "Compose project", type: "text", mono: true },
      { key: "command", label: "Command in the container", type: "multiline", mono: true, help: "Run through sh -c inside the container." },
      { key: "sudo", label: "Use sudo -n", type: "bool", help: "Off unless you turn it on. sudo -n fails rather than ask for a password." },
    ],
  },
];

export const KIND_BY: Record<StepKind, KindInfo> = Object.fromEntries(KINDS.map((k) => [k.kind, k])) as Record<StepKind, KindInfo>;

/** Which fields of a Docker step apply to an action. */
export function dockerFields(action: string): string[] {
  switch (action) {
    case "start":
    case "stop":
    case "restart":
      return ["container"];
    case "pull":
      return ["image"];
    case "exec":
      return ["container", "command"];
    case "compose_up":
    case "compose_down":
      return ["file", "project"];
    default:
      return [];
  }
}

/** The fields to draw for a kind (a Docker step only the ones its action uses). */
export function fieldsFor(kind: StepKind, body: Obj): Field[] {
  const all = KIND_BY[kind].fields;
  if (kind !== "docker") return all;
  const used = new Set(["action", "sudo", ...dockerFields(typeof body.action === "string" ? body.action : "")]);
  return all.filter((f) => used.has(f.key));
}

const clip = (s: string, n = 70) => {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length > n ? `${one.slice(0, n - 1)}…` : one;
};

const str = (v: Json | undefined) => (typeof v === "string" ? v : "");
const num = (v: Json | undefined) => (typeof v === "number" ? v : null);

/** One line that says what a step does, as it would read on its card. */
export function summary(step: Obj, kind: StepKind | null): string {
  const b = (kind && step[kind] && typeof step[kind] === "object" ? (step[kind] as Obj) : {}) as Obj;
  switch (kind) {
    case "run":
      return clip(str(step.run));
    case "wait":
      return `Until ${clip(str(b.run), 50)} succeeds${num(b.timeout_secs) ? `, up to ${num(b.timeout_secs)} s` : ""}`;
    case "upload":
      return `${str(b.local) || "a file"} → ${str(b.remote)}`;
    case "download":
      return `${str(b.remote)} → ${str(b.to) || "a folder"}`;
    case "delay":
      return `Wait ${num(b.secs) ?? "?"} s`;
    case "http": {
      const code = num(b.expect_status);
      return `${(str(b.method) || "get").toUpperCase()} ${clip(str(b.url), 50)} expecting ${code ? code : "2xx or 3xx"}`;
    }
    case "tcp":
      return `Connect to ${str(b.host) || "the host"}:${num(b.port) ?? "?"}`;
    case "docker": {
      const a = str(b.action);
      return `docker ${a.replace("_", " ")} ${str(b.container) || str(b.image) || str(b.file)}`.trim();
    }
    default:
      return "This step doesn't say what to do.";
  }
}

// -- what the YAML editor suggests -----------------------------------------------------------------------

export const TOP_LEVEL = ["name", "description", "category", "defaults", "params", "steps", "rollback"];
export const STEP_COMMON = ["name", "id", "disabled", "when", "on_error", "timeout_secs", "retries", "retry_delay_secs"];
export const PARAM_KEYS = ["name", "label", "description", "kind", "default", "optional", "choices"];
export const DEFAULT_KEYS = ["timeout_secs", "retries", "retry_delay_secs", "on_error", "strategy", "hosts", "notify"];
export const WHEN_KEYS = ["step", "exit", "exit_not", "param", "equals", "not_equals"];
export const PARAM_KINDS = ["text", "number", "boolean", "file", "dir", "secret"];

/** Keys valid in the mapping the cursor is in, given the keys above it (`path` is like ["steps", "http"]). */
export function keysFor(path: string[]): string[] {
  const last = path.at(-1) ?? "";
  if (path.length === 0) return TOP_LEVEL;
  if (last === "defaults") return DEFAULT_KEYS;
  if (last === "params") return PARAM_KEYS;
  if (last === "when") return WHEN_KEYS;
  if (last === "steps" || last === "rollback") return [...STEP_COMMON, ...KINDS.map((k) => k.kind)];
  const kind = KINDS.find((k) => k.kind === last);
  if (kind) return kind.fields.map((f) => f.key);
  return [];
}

/** Values valid for a key (`enum`-like), for completion after the colon. */
export function valuesFor(path: string[], key: string): string[] {
  if (key === "on_error") return ["stop", "continue"];
  if (key === "kind" && path.at(-1) === "params") return PARAM_KINDS;
  if (key === "notify") return ["always", "failure", "never"];
  if (key === "category") return ["Deployments", "Maintenance", "Monitoring", "Backups", "Security", "Custom"];
  if (key === "method") return ["get", "head"];
  if (key === "on_conflict") return ["rename", "skip", "overwrite"];
  if (key === "action") return KIND_BY.docker.fields[0].options!.map((o) => o.value);
  if (key === "disabled" || key === "optional" || key === "sudo") return ["true", "false"];
  return [];
}

// -- where the cursor is in a YAML document ---------------------------------------------------------------

export interface YamlContext {
  /** The keys that contain the cursor, outermost first (`["steps", "http"]`). */
  path: string[];
  /** Set when the cursor is after `key:` on the line, typing its value. */
  valueOf: string | null;
  /** The word typed so far. */
  prefix: string;
}

const indentOf = (line: string) => line.length - line.trimStart().length;

/** Which mapping the cursor is in, found from the lines above it by their indentation. Good enough to suggest keys; the backend's check is the authority. */
export function yamlContext(text: string, offset: number): YamlContext {
  const before = text.slice(0, offset);
  const lines = before.split("\n");
  const current = lines.pop() ?? "";
  const trimmed = current.trimStart();
  const body = trimmed.startsWith("- ") ? trimmed.slice(2) : trimmed;
  const bodyIndent = indentOf(current) + (trimmed.startsWith("- ") ? 2 : 0);
  const colon = body.indexOf(":");
  const valueOf = colon >= 0 ? body.slice(0, colon).trim() : null;
  const prefix = colon >= 0 ? body.slice(colon + 1).trimStart() : body;
  const path: string[] = [];
  let level = bodyIndent;
  for (let i = lines.length - 1; i >= 0 && level > 0; i--) {
    const raw = lines[i];
    const t = raw.trim();
    if (t === "" || t.startsWith("#")) continue;
    const dash = t.startsWith("- ");
    const ind = indentOf(raw) + (dash ? 2 : 0);
    if (ind >= level) continue;
    const content = dash ? t.slice(2) : t;
    // A line that is just `key:` opens the mapping the cursor is in; any other line at a smaller indent only moves the level out.
    if (content.endsWith(":")) path.unshift(content.slice(0, -1).trim());
    level = ind;
  }
  return { path, valueOf, prefix };
}
