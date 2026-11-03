// Words for the status bar. Pure.
import type { HostStream } from "$lib/stores/opslogs.svelte";

/** "just now", "12 s ago", "3 min ago", "2 h ago", from a time and the clock. */
export function relativeTime(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 3) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return `${Math.round(s / 3600)} h ago`;
}

/** What a host's stream is doing, in a few words. */
export function retryText(h: Pick<HostStream, "state" | "attempt" | "retryAt" | "message">, now: number): string {
  switch (h.state) {
    case "streaming":
      return "streaming";
    case "connecting":
      return "connecting";
    case "stopped":
      return "stopped";
    case "failed":
      return h.message ? `failed: ${h.message}` : "failed";
    case "reconnecting": {
      const wait = h.retryAt ? Math.max(0, Math.ceil((h.retryAt - now) / 1000)) : 0;
      const n = h.attempt > 0 ? `, attempt ${h.attempt}` : "";
      return h.retryAt && wait > 0 ? `reconnecting in ${wait} s${n}` : `reconnecting${n}`;
    }
  }
}
