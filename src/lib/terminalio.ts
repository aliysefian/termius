// One place that knows which backend a pane talks to: an SSH session, a
// local shell, or a Telnet/serial console.
import * as api from "$lib/api";
import { ssh } from "$lib/ssh";
import type { Pane } from "$lib/stores/ui.svelte";

const encoder = new TextEncoder();

/** Saved hosts set to Telnet use the raw backend; recorded at connect. */
const rawPanes = new Set<string>();

export function markRaw(paneId: string, raw: boolean) {
  if (raw) rawPanes.add(paneId);
  else rawPanes.delete(paneId);
}

/** Mosh panes run the system's mosh-client in a local terminal. */
const localPanes = new Set<string>();

export function markLocal(paneId: string, local: boolean) {
  if (local) localPanes.add(paneId);
  else localPanes.delete(paneId);
}

function backend(pane: Pane): "local" | "raw" | "ssh" {
  const k = pane.target.kind;
  if (k === "local" || localPanes.has(pane.id)) return "local";
  if (k === "telnet" || k === "serial" || rawPanes.has(pane.id)) return "raw";
  return "ssh";
}

const bytes = (data: string | Uint8Array) => (typeof data === "string" ? encoder.encode(data) : data);

export function writeToPane(pane: Pane, data: string | Uint8Array): Promise<void> {
  switch (backend(pane)) {
    case "local":
      return api.localTerm.write(pane.id, bytes(data));
    case "raw":
      return api.raw.write(pane.id, bytes(data));
    default:
      return ssh.write(pane.id, data);
  }
}

export function resizePane(pane: Pane, cols: number, rows: number): Promise<void> {
  switch (backend(pane)) {
    case "local":
      return api.localTerm.resize(pane.id, cols, rows);
    case "raw":
      return api.raw.resize(pane.id, cols, rows);
    default:
      return ssh.resize(pane.id, cols, rows);
  }
}

export function closePane(pane: Pane): Promise<void> {
  const b = backend(pane);
  rawPanes.delete(pane.id);
  localPanes.delete(pane.id);
  switch (b) {
    case "local":
      return api.localTerm.close(pane.id);
    case "raw":
      return api.raw.close(pane.id);
    default:
      return ssh.disconnect(pane.id);
  }
}
