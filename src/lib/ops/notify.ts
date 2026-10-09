// How an alert reaches the person. Each way is an adapter, so a new one is added by registering it, and a failing
// one never stops the others. The built-in adapters are the in-app toast and the operating system's notification
// (stores/alerts.svelte.ts); nothing here sends anything off this computer.
import { atLeast, type Alert, type Severity } from "./alerts";

export interface NotificationAdapter {
  id: string;
  label: string;
  /** May be asynchronous. A rejection is caught by the notifier. */
  notify(alert: Alert): void | Promise<void>;
}

export class Notifier {
  #adapters: NotificationAdapter[] = [];

  get adapters(): readonly NotificationAdapter[] {
    return this.#adapters;
  }

  /** Registers an adapter; one with the same id replaces the earlier one. */
  add(adapter: NotificationAdapter) {
    this.#adapters = [...this.#adapters.filter((a) => a.id !== adapter.id), adapter];
  }

  remove(id: string) {
    this.#adapters = this.#adapters.filter((a) => a.id !== id);
  }

  /**
   * Sends `alert` through every adapter, unless it was raised in quiet hours or is below `floor`. Returns the ids of
   * the adapters that failed, so the caller can say so.
   */
  async send(alert: Alert, floor: Severity = "info"): Promise<string[]> {
    if (alert.quiet || !atLeast(alert.severity, floor)) return [];
    const failed: string[] = [];
    await Promise.all(
      this.#adapters.map(async (a) => {
        try {
          await a.notify(alert);
        } catch {
          failed.push(a.id);
        }
      }),
    );
    return failed;
  }
}
