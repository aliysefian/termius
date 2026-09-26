// Drag a host onto a group (or the empty area of the tree) to move it.
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage } from "$lib/types";

export const HOST_MIME = "application/x-sshvault-host";

export function hostDragStart(e: DragEvent, hostId: string) {
  e.dataTransfer?.setData(HOST_MIME, hostId);
  if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
}

export function acceptsHost(e: DragEvent): boolean {
  return !!e.dataTransfer?.types.includes(HOST_MIME);
}

/** Move the dragged host into `group` ("" = top level). */
export async function dropHostInto(e: DragEvent, group: string) {
  const id = e.dataTransfer?.getData(HOST_MIME);
  if (!id) return;
  e.preventDefault();
  e.stopPropagation();
  const rec = vaultStore.hostById.get(id);
  if (!rec?.data || rec.data.group === group) return;
  try {
    await vaultStore.saveHost(id, { ...$state.snapshot(rec.data), group });
    if (group) {
      // Make sure the destination is visible.
      const next = new Set(ui.collapsedGroups);
      let path = "";
      for (const part of group.split("/")) {
        path = path ? `${path}/${part}` : part;
        next.delete(path);
      }
      ui.collapsedGroups = next;
    }
  } catch (err) {
    ui.notify("error", errorMessage(err));
  }
}
