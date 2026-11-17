import type { TutorialDefinition } from "@/engine/types";
import { click, move, set, toast, type as typeText, wait } from "./helpers";

const def: TutorialDefinition = {
  id: "host-management",
  initial: { view: "hosts" },
  cursorStart: { x: 0.9, y: 0.9 },
  posterTime: 20000,
  steps: [
    wait(800, { chapter: "View your hosts", note: "Hosts are grouped, with a status dot and an environment badge so production stands out." }),
    move("host-row-prod-api", 900),
    move("host-row-db", 700),
    move("host-row-dev", 700),
    wait(300),

    move("host-search", 800, { chapter: "Search", note: "Search matches names, addresses, users and tags." }),
    click("host-search"),
    typeText("host-search", "198.51", { cps: 7 }),
    wait(1200, { note: "Only the hosts on that network remain." }),
    set({ fields: { "host-search": "" } }),
    typeText("host-search", "postgres", { cps: 9 }),
    wait(1200, { note: "Tags are searchable too." }),
    set({ fields: { "host-search": "" } }),
    typeText("host-search", "dev", { cps: 7 }),
    wait(700),

    move("host-row-dev", 800, { chapter: "Select a host", note: "Selecting a host shows its address, port, user, group and tags." }),
    click("host-row-dev", { selected: { host: "dev" } }),
    wait(1200),

    move("host-edit", 800, { chapter: "Edit its settings", note: "Edit opens the same form used to create a host, with the saved values filled in." }),
    click("host-edit", {
      panel: "host-form",
      flags: { editingHost: true },
      fields: { "f-label": "Development Server", "f-hostname": "198.51.100.20", "f-port": "22", "f-username": "developer", "f-tags": "dev" },
    }),
    wait(700),
    move("f-label", 700),
    click("f-label"),
    typeText("f-label", "Dev Server (EU)"),
    move("f-port", 600),
    click("f-port"),
    typeText("f-port", "2222", { cps: 6 }),
    move("f-tags", 600),
    click("f-tags"),
    typeText("f-tags", "dev, eu", { cps: 8 }),
    wait(500),

    move("f-save", 800, { chapter: "Save the changes", note: "Saving updates the host everywhere it is used, and syncs to your other computers." }),
    click("f-save", { panel: null, flags: { "edited-dev": true } }),
    toast("Host updated", 1500),
    wait(1800),
  ],
};

export default def;
