// Transfer speed from progress events. Events arrive in bursts, so a rate is taken over at least
// MIN_WINDOW_MS and smoothed, or the number would flicker between "0" and "200 MB/s".

export interface SpeedSample {
  /** When the sample was taken (ms). */
  at: number;
  /** Bytes done at that time. */
  bytes: number;
  /** Smoothed bytes per second; 0 until a rate has been measured. */
  bps: number;
}

const MIN_WINDOW_MS = 250;
/** Weight of the newest interval in the running rate. */
const ALPHA = 0.3;

export function nextSample(prev: SpeedSample | undefined, bytes: number, now: number): SpeedSample {
  if (!prev) return { at: now, bytes, bps: 0 };
  // Fewer bytes than before: the transfer restarted (a retry), so the old rate means nothing.
  if (bytes < prev.bytes) return { at: now, bytes, bps: 0 };
  const dt = now - prev.at;
  if (dt < MIN_WINDOW_MS) return prev;
  const instant = ((bytes - prev.bytes) * 1000) / dt;
  return { at: now, bytes, bps: prev.bps === 0 ? instant : prev.bps * (1 - ALPHA) + instant * ALPHA };
}

/** "12s", "3m 05s", "1h 02m"; empty when it can't be known. */
export function formatEta(remainingBytes: number, bps: number): string {
  if (!(bps > 0) || !(remainingBytes >= 0)) return "";
  const s = Math.max(1, Math.round(remainingBytes / bps));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}
