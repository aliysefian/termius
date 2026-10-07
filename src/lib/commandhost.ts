// A host whose "connect" is a command this app runs for you in a local terminal: AWS Session Manager, `gcloud
// compute ssh`, Teleport, `kubectl exec`. The command is the person's own text; the host's values go into it
// only if they are safe to put on any shell's command line, and the person approves the exact command first.

export interface Values {
  host: string;
  user: string;
  id: string;
}

export const PLACEHOLDERS = ["host", "user", "id"] as const;
export const MAX_COMMAND = 500;

/** Characters that mean the same thing on bash, zsh, fish, PowerShell and cmd. */
const SAFE_VALUE = /^[A-Za-z0-9._:@/=+,-]{1,253}$/;

const WHAT: Record<string, string> = { host: "the host's address", user: "the user name", id: "the host's id (from the import)" };

export type Rendered = { ok: true; command: string } | { ok: false; error: string };

export function renderCommand(template: string, v: Values): Rendered {
  const t = template.trim();
  if (!t) return { ok: false, error: "This host has no command to run." };
  if (t.length > MAX_COMMAND) return { ok: false, error: `The command is longer than ${MAX_COMMAND} characters.` };
  if (/[\r\n\0]/.test(t)) return { ok: false, error: "The command must be one line." };
  let error: string | null = null;
  const command = t.replace(/\{([^{}]*)\}/g, (whole, name: string) => {
    if (!(PLACEHOLDERS as readonly string[]).includes(name)) {
      error ??= `{${name}} isn't something a command can use. Use {host}, {user} or {id}.`;
      return whole;
    }
    const value = v[name as keyof Values];
    if (!value) {
      error ??= `This host has no ${WHAT[name]}, which the command uses ({${name}}).`;
      return whole;
    }
    if (!SAFE_VALUE.test(value)) {
      error ??= `${WHAT[name][0].toUpperCase()}${WHAT[name].slice(1)} has characters that can't safely go on a command line.`;
      return whole;
    }
    return value;
  });
  return error ? { ok: false, error } : { ok: true, command };
}

/** A short fingerprint of the command, so approval holds only for the command that was approved. */
export function fingerprint(command: string): string {
  let h = 5381;
  for (let i = 0; i < command.length; i++) h = ((h << 5) + h + command.charCodeAt(i)) >>> 0;
  return h.toString(36) + command.length.toString(36);
}

export const PRESETS: { label: string; command: string }[] = [
  { label: "AWS Session Manager", command: "aws ssm start-session --target {id}" },
  { label: "Google Cloud (gcloud compute ssh)", command: "gcloud compute ssh {id}" },
  { label: "Teleport", command: "tsh ssh {user}@{host}" },
  { label: "Kubernetes pod (kubectl exec)", command: "kubectl exec -it {id} -- sh" },
  { label: "Boundary", command: "boundary connect ssh -target-id {id}" },
  { label: "Tailscale SSH", command: "tailscale ssh {user}@{host}" },
];
