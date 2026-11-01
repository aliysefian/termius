// What each tunnel action does, in one place for the table's row buttons, its menu, the details panel and the
// keyboard. They go through the Tunnels store (which makes the backend calls and refuses a second click while one is
// underway), and the terminal action goes through the existing terminal code: a terminal and a tunnel are separate
// connections, so opening or closing one never starts or stops the other.

import { tunnels } from "$lib/stores/tunnels.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { canRestart, canStart, canStop, type TunnelRow } from "$lib/tunnels";

/** A terminal to the host the tunnel goes through. The tunnel keeps its own connection and is not touched. */
export function openTunnelTerminal(row: TunnelRow) {
  if (!row.host) return;
  tunnels.select(row.id);
  ui.openTerminal(row.rule.host_id, row.hostLabel);
}

export function tunnelActions(row: TunnelRow) {
  const busy = tunnels.busy(row.id);
  return {
    canStart: canStart(row) && !busy,
    canStop: canStop(row) && !busy,
    canRestart: canRestart(row) && !busy,
    startWhy: !row.host ? "Its host no longer exists" : row.status === "active" ? "Already running" : busy ? "An operation is in progress" : "",
    stopWhy: row.status === "stopped" || row.status === "failed" ? "Not running" : busy ? "An operation is in progress" : "",
    restartWhy: row.status === "stopped" ? "Start it instead" : busy ? "An operation is in progress" : "",
    start: () => void tunnels.start(row.id),
    stop: () => void tunnels.stop(row.id),
    restart: () => void tunnels.restart(row.id),
    edit: () => {
      tunnels.select(row.id, "configuration");
      tunnels.reveal();
    },
    duplicate: () => (ui.modal = { kind: "forward", id: null, template: JSON.parse(JSON.stringify(row.rule)) }),
    logs: () => {
      tunnels.select(row.id, "logs");
      tunnels.reveal();
    },
    remove: () => void tunnels.remove(row.id),
  };
}
