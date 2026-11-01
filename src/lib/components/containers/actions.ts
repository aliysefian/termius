// What a container action does, in one place for the row buttons, the menu, the drawer and the bulk bar. Everything
// goes through the Containers store, which asks first where it should, makes the backend call and says how it went.

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { containers } from "$lib/stores/containers.svelte";
import { ui } from "$lib/stores/ui.svelte";
import type { MenuId } from "$lib/containerview";
import { errorMessage, type ContainerInfo } from "$lib/types";

export type DrawerTab = "overview" | "logs" | "stats" | "inspect" | "terminal";

/** The drawer tab a container's menu entry opens, if it is one. */
export const MENU_TAB: Partial<Record<MenuId, DrawerTab>> = { logs: "logs", inspect: "inspect", terminal: "terminal" };

async function copy(text: string, what: string) {
  try {
    await writeText(text);
    ui.notify("info", `Copied the ${what}.`);
  } catch (e) {
    ui.notify("error", errorMessage(e));
  }
}

/** Run a menu entry for a container on a source. `openTab` shows the drawer on a tab. */
export function runMenu(key: string, c: ContainerInfo, id: MenuId, openTab: (tab: DrawerTab) => void) {
  switch (id) {
    case "start":
      return void containers.act(key, c, { action: "start" });
    case "stop":
      return void containers.act(key, c, { action: "stop" });
    case "restart":
      return void containers.act(key, c, { action: "restart" });
    case "pause":
      return void containers.act(key, c, { action: "pause" });
    case "resume":
      return void containers.act(key, c, { action: "unpause" });
    case "delete":
      return void containers.act(key, c, { action: "remove", force: false });
    case "copy-id":
      return void copy(c.id, "container ID");
    case "copy-name":
      return void copy(c.name, "container name");
    default: {
      const tab = MENU_TAB[id];
      if (tab) openTab(tab);
    }
  }
}
