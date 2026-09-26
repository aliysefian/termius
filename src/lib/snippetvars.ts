// Snippet variables: `{{name}}` placeholders filled in when a snippet runs.
//
// Built-ins come from the target host, so a snippet broadcast to several
// panes or run on several hosts gets each host's own values. Any other name
// is asked for once per run. Write `{{{{` to get a literal `{{`.

export interface HostContext {
  /** Saved label, or user@host for quick connections. */
  host: string;
  hostname: string;
  port: number;
  user: string;
}

export const BUILTINS = ["host", "hostname", "port", "user", "date", "time"] as const;
type Builtin = (typeof BUILTINS)[number];

const VAR = /\{\{\s*([A-Za-z_][\w.-]*)\s*\}\}/g;
const ESCAPED = /\{\{\{\{/g;
const SENTINEL = "\u0000LBRACE\u0000";

/** Names of the variables the user must supply (built-ins excluded), in order. */
export function promptedVariables(command: string): string[] {
  const names: string[] = [];
  for (const m of command.replace(ESCAPED, "").matchAll(VAR)) {
    const name = m[1];
    if (!(BUILTINS as readonly string[]).includes(name) && !names.includes(name)) names.push(name);
  }
  return names;
}

function pad(n: number) {
  return String(n).padStart(2, "0");
}

function builtin(name: Builtin, ctx: HostContext, now: Date): string {
  switch (name) {
    case "host":
      return ctx.host;
    case "hostname":
      return ctx.hostname;
    case "port":
      return String(ctx.port);
    case "user":
      return ctx.user;
    case "date":
      return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
    case "time":
      return `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
  }
}

/** Fill in every placeholder. Unknown names without a value stay as written. */
export function render(command: string, ctx: HostContext, values: Record<string, string>, now = new Date()): string {
  return command
    .replace(ESCAPED, SENTINEL)
    .replace(VAR, (whole, name: string) => {
      if ((BUILTINS as readonly string[]).includes(name)) return builtin(name as Builtin, ctx, now);
      return name in values ? values[name] : whole;
    })
    .replaceAll(SENTINEL, "{{");
}
