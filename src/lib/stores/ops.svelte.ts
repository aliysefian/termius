// Where you are in Operations, and the ways to get there from elsewhere: a service's "View logs", a log entry's "View
// service", a monitored host's "Inspect logs". Each one reuses what is already running (the stream, the connection).
import { opsLogs } from "$lib/stores/opslogs.svelte";
import { ui } from "$lib/stores/ui.svelte";
import type { Uuid } from "$lib/types";

export type OpsTab = "logs" | "services" | "security" | "alerts";

class Ops {
  tab = $state<OpsTab>("logs");
  /** A service to select when the Services tab opens. */
  serviceFocus = $state<{ hostId: Uuid; unit: string } | null>(null);

  go(tab: OpsTab) {
    this.tab = tab;
    ui.view = "ops";
  }

  /** The logs of one host (and unit): follows them, or filters a stream that is already running. */
  async viewLogs(hostId: Uuid, unit?: string, filter?: { units?: string[]; text?: string; regex?: boolean }) {
    this.go("logs");
    await opsLogs.inspect(hostId, unit, filter);
  }

  /** A service on a host, selected in the Services tab. */
  viewService(hostId: Uuid, unit: string) {
    this.serviceFocus = { hostId, unit };
    this.go("services");
  }
}

export const ops = new Ops();
