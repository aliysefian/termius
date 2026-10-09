// What the status bar says about the synced vault folder. Quiet when there is nothing to say: sync itself is done by the
// tool that copies the folder (Dropbox, Syncthing, ...), so this never claims that the devices "are in sync"; it only
// points at problems the app can see.

export interface SyncProblems {
  /** Items that two devices changed at once and that still need a decision. */
  conflicts: number;
  /** Records that went back to an older revision, or vanished, since this computer last saw them. */
  rollbacks: number;
}

export interface SyncBadge {
  text: string;
  /** What the badge's tooltip explains. */
  title: string;
  /** "danger" for a folder that went backwards, "warning" for conflicts to resolve. */
  tone: "warning" | "danger";
}

const plural = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** The badge to show, or null when the folder has no problems. A rolled-back folder is the more serious, so it comes first. */
export function syncBadge(p: SyncProblems): SyncBadge | null {
  const conflicts = Math.max(0, Math.trunc(p.conflicts) || 0);
  const rollbacks = Math.max(0, Math.trunc(p.rollbacks) || 0);
  if (rollbacks > 0) {
    return {
      text: `Folder went backwards: ${plural(rollbacks, "record", "records")}`,
      title: `${plural(rollbacks, "record is", "records are")} older than, or missing since, what this computer last saw${conflicts ? `; ${plural(conflicts, "sync conflict needs", "sync conflicts need")} a decision too` : ""}. Open the Vault screen.`,
      tone: "danger",
    };
  }
  if (conflicts > 0) {
    return { text: `${plural(conflicts, "sync conflict", "sync conflicts")}`, title: "Two devices changed the same item at once. Open the Vault screen to choose.", tone: "warning" };
  }
  return null;
}
