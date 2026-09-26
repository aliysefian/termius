// One place that knows whether a pane is an SSH session or a local shell.
import * as api from "$lib/api";
import { ssh } from "$lib/ssh";
import type { Pane } from "$lib/stores/ui.svelte";

const encoder = new TextEncoder();

export function writeToPane(pane: Pane, data: string | Uint8Array): Promise<void> {
  if (pane.target.kind === "local") {
    return api.localTerm.write(pane.id, typeof data === "string" ? encoder.encode(data) : data);
  }
  return ssh.write(pane.id, data);
}

export function resizePane(pane: Pane, cols: number, rows: number): Promise<void> {
  return pane.target.kind === "local" ? api.localTerm.resize(pane.id, cols, rows) : ssh.resize(pane.id, cols, rows);
}

export function closePane(pane: Pane): Promise<void> {
  return pane.target.kind === "local" ? api.localTerm.close(pane.id) : ssh.disconnect(pane.id);
}
