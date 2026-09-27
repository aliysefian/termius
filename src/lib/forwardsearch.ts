// Search for port-forwarding rules. Every word typed must appear somewhere
// in the rule: its label, its host (name, address, group, tags), ports,
// destination, type or current status.
import { describeForward, type ForwardRule, type ForwardStatus, type Host } from "./types";

export type StatusFilter = "all" | "running" | "stopped" | "error";

export function statusWord(st: ForwardStatus | undefined): "running" | "stopped" | "error" {
  if (st?.state === "active" || st?.state === "starting") return "running";
  if (st?.state === "error") return "error";
  return "stopped";
}

function haystack(rule: ForwardRule, host: Host | undefined, st: ForwardStatus | undefined): string {
  const kindWords = rule.kind === "local" ? "local -L" : rule.kind === "remote" ? "remote -R" : "dynamic socks socks5 -D";
  return [
    rule.label,
    describeForward(rule),
    kindWords,
    rule.bind_addr,
    String(rule.bind_port),
    "dest_host" in rule ? `${rule.dest_host} ${rule.dest_port} ${rule.dest_host}:${rule.dest_port}` : "",
    st?.state === "active" ? String(st.port) : "",
    statusWord(st),
    host?.label ?? "missing host",
    host?.hostname ?? "",
    host?.group ?? "",
    ...(host?.tags ?? []),
  ]
    .join(" ")
    .toLowerCase();
}

export function matchesForward(
  rule: ForwardRule,
  host: Host | undefined,
  st: ForwardStatus | undefined,
  query: string,
  status: StatusFilter = "all",
): boolean {
  if (status !== "all" && statusWord(st) !== status) return false;
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = haystack(rule, host, st);
  return words.every((w) => hay.includes(w));
}
