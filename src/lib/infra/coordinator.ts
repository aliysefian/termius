// Running discovery for many hosts without letting one slow host hold up the rest.

export interface RunReport {
  started: number;
  finished: number;
  failed: number;
  cancelled: boolean;
}

/**
 * Runs `work` for each item with at most `limit` at once, in order. A worker that throws is counted and the others
 * carry on. When `signal` aborts, nothing new is started; what is running is left to finish (and to look at the signal).
 */
export async function runBounded<T>(items: T[], limit: number, work: (item: T, signal: AbortSignal) => Promise<void>, signal: AbortSignal): Promise<RunReport> {
  const report: RunReport = { started: 0, finished: 0, failed: 0, cancelled: false };
  let next = 0;
  const lane = async () => {
    while (next < items.length) {
      if (signal.aborted) {
        report.cancelled = true;
        return;
      }
      const item = items[next++];
      report.started++;
      try {
        await work(item, signal);
      } catch {
        report.failed++;
      }
      report.finished++;
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, lane));
  return report;
}

/** One discovery at a time per key: asking again while one is running joins it instead of starting another. */
export class SingleFlight {
  #running = new Map<string, Promise<void>>();

  run(key: string, work: () => Promise<void>): Promise<void> {
    const existing = this.#running.get(key);
    if (existing) return existing;
    const p = work().finally(() => this.#running.delete(key));
    this.#running.set(key, p);
    return p;
  }

  has(key: string): boolean {
    return this.#running.has(key);
  }
}
