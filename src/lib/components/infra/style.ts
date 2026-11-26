// How a resource looks: its icon and the colour of its status. Colour is never the only signal: each status also has a
// word, and each freshness has a label.
import { Box, Container, Layers, Network, Server, SquareTerminal, Wrench } from "lucide-svelte";
import type { Freshness, ResourceStatus, ResourceType } from "$lib/infra/model";

export const TYPE_ICON: Record<ResourceType, typeof Server> = {
  host: Server,
  docker_engine: Layers,
  container: Container,
  port: Network,
  systemd_service: Wrench,
  tmux_session: SquareTerminal,
};
export const FALLBACK_ICON = Box;

export const STATUS_TONE: Record<ResourceStatus, string> = {
  healthy: "bg-success",
  running: "bg-success",
  stopped: "bg-fg-muted/50",
  warning: "bg-warning",
  critical: "bg-danger",
  unknown: "bg-fg-muted/30",
  offline: "bg-fg-muted/30",
};

export const FRESH_LABEL: Record<Freshness, string> = { live: "Live", fresh: "Recent", stale: "Stale", unavailable: "Unavailable", unsupported: "Not supported" };
export const FRESH_TONE: Record<Freshness, string> = { live: "text-success", fresh: "text-fg-muted", stale: "text-warning", unavailable: "text-fg-muted", unsupported: "text-fg-muted" };

/** A property's value for display: dates are times, a missing value is a dash, never 0 or an empty string. */
export function show(label: string, v: string | number | boolean | null): string {
  if (v === null || v === "") return "—";
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (typeof v === "number" && /^(Created|Last connected)$/.test(label)) return new Date(v).toLocaleString();
  return String(v);
}
