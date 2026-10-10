// The entries on the left rail, in their groups, and how a person's choices (hide this, put that first) are
// applied to them. Pure apart from the icons, so the arranging is tested.
import { Activity, ArrowLeftRight, Code, Container, Database, FolderSync, ListChecks, Server, Star, Wrench } from "lucide-svelte";
import type { View } from "$lib/stores/ui.svelte";

export interface RailItem {
  view: View;
  label: string;
  icon: typeof Server;
  /** Other pages that belong to the same entry (Kubernetes is a tab of Containers). */
  also?: View[];
}

/** The groups the rail is drawn in, separated by a line. */
export const RAIL_GROUPS: RailItem[][] = [
  [
    { view: "hosts", label: "Hosts", icon: Server },
    { view: "favorites", label: "Favorites", icon: Star },
    { view: "fleetlist", label: "Fleet", icon: Activity, also: ["fleet"] },
  ],
  [
    { view: "snippets", label: "Snippets", icon: Code },
    { view: "sftp", label: "Files", icon: FolderSync },
    { view: "forwarding", label: "Tunnels", icon: ArrowLeftRight },
  ],
  [
    { view: "databases", label: "Databases", icon: Database },
    { view: "containers", label: "Containers", icon: Container, also: ["kubernetes"] },
    { view: "ops", label: "Ops", icon: Wrench },
    { view: "runbooks", label: "Runbooks", icon: ListChecks },
  ],
];

/** Hosts is where everything starts, so it can't be put away. */
export const ALWAYS_SHOWN: View[] = ["hosts"];

/**
 * The groups with `order` applied inside each group (what `order` doesn't mention keeps its place, after what it
 * does), and the entries in `hidden` taken out and returned separately; they go in the Manage menu instead.
 * Unknown names in either list are ignored, so a rail that changes between versions never breaks.
 */
export function arrange(groups: RailItem[][], order: string[], hidden: string[]): { groups: RailItem[][]; away: RailItem[] } {
  const away: RailItem[] = [];
  const out = groups.map((g) => {
    const rank = (i: RailItem) => {
      const at = order.indexOf(i.view);
      return at < 0 ? order.length + g.indexOf(i) : at;
    };
    const sorted = [...g].sort((a, b) => rank(a) - rank(b));
    return sorted.filter((i) => {
      if (hidden.includes(i.view) && !ALWAYS_SHOWN.includes(i.view)) {
        away.push(i);
        return false;
      }
      return true;
    });
  });
  return { groups: out.filter((g) => g.length), away };
}

/** `order` with `view` moved one place up or down among the others in its group. */
export function move(groups: RailItem[][], order: string[], view: View, by: -1 | 1): string[] {
  const g = groups.find((x) => x.some((i) => i.view === view));
  if (!g) return order;
  const current = arrange([g], order, []).groups[0].map((i) => i.view as string);
  const at = current.indexOf(view);
  const to = at + by;
  if (to < 0 || to >= current.length) return order;
  [current[at], current[to]] = [current[to], current[at]];
  // This group's order, then everything that was ordered for other groups.
  return [...current, ...order.filter((v) => !current.includes(v))];
}
