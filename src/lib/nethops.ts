// Pure helpers for showing the route of a session: what colour a hop is, how long it has been up, how a countdown
// reads. The live numbers come from the network store; nothing here touches the backend.

import type { HopRtt } from "$lib/types";

export type HopHealth = "up" | "slow" | "down" | "unknown";

/** Above this round trip a hop is shown as slow (amber). */
export const SLOW_MS = 300;
/** Polls in a row with no answer before a hop is shown as dropped. A single miss is amber: one lost probe is not an outage. */
export const MISSES_BEFORE_DOWN = 2;

/**
 * A hop's health from its latest measurement. `misses` counts the polls in a row that got no answer from it;
 * `sessionUp` is false once the whole session has ended.
 */
export function hopHealth(rtt: HopRtt | undefined, misses: number, sessionUp: boolean): HopHealth {
  if (!sessionUp) return "down";
  if (!rtt) return "unknown";
  if (rtt.total_ms === null) return misses >= MISSES_BEFORE_DOWN ? "down" : "slow";
  return rtt.total_ms > SLOW_MS ? "slow" : "up";
}

/** The dot colour classes for a health, using the app's own colour tokens. */
export const HEALTH_DOT: Record<HopHealth, string> = {
  up: "bg-success",
  slow: "bg-warning",
  down: "bg-danger",
  unknown: "bg-fg-muted/40",
};

/** "42 ms", or a dash where there is no number. */
export const formatMs = (ms: number | null | undefined) => (ms === null || ms === undefined ? "–" : `${ms} ms`);

/** "45s", "3m 05s", "2h 10m", "3d 4h": how long a hop has been up. */
export function formatUptime(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

/** "4:32" for a countdown of seconds. */
export function formatCountdown(secs: number): string {
  const s = Math.max(0, Math.ceil(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The short form of a host key fingerprint for a narrow popover: `SHA256:abcdefgh…wxyz`. */
export function shortFingerprint(fp: string | null | undefined): string {
  if (!fp) return "–";
  if (fp.length <= 24) return fp;
  return `${fp.slice(0, 15)}…${fp.slice(-6)}`;
}

/** The address a forward listens on, as someone would type it into a browser. */
export const localUrl = (port: number) => `http://localhost:${port}`;
