// How a connection-log entry reads on the Vault screen. Built as one string, because Svelte drops the space at the
// start of a text-only `{:else if}` branch and the outcome ran into the duration ("25m 41s· dropped").
import type { ConnectionLogEntry } from "./stores/connectionlog.svelte";

export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

type Outcome = Pick<ConnectionLogEntry, "startedAt" | "endedAt" | "exitCode" | "reason">;

/** How long it lasted, then its exit code or how it ended; empty while it lasts. */
export function outcomeParts(e: Outcome, duration: (ms: number) => string = formatDuration): string[] {
  if (!e.endedAt) return [];
  const parts = [duration(e.endedAt - e.startedAt)];
  if (e.exitCode != null) parts.push(`exit ${e.exitCode}`);
  else if (e.reason === "dropped" || e.reason === "failed") parts.push(e.reason);
  return parts;
}

/** "· 18m 4s · exit 0", "· 25m 41s · dropped", "· 3s · failed", or "· connected" while it lasts. */
export function entryOutcome(e: Outcome): string {
  const parts = outcomeParts(e);
  return parts.length ? `· ${parts.join(" · ")}` : "· connected";
}
