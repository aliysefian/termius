import type { TutorialDefinition } from "@/engine/types";
import { makeHelpers } from "./helpers";

const { click, move, toast, wait } = makeHelpers(2.2);

const def: TutorialDefinition = {
  id: "run-on-many-hosts",
  initial: { view: "hosts" },
  cursorStart: { x: 0.85, y: 0.85 },
  posterTime: 26000,
  steps: [
    wait(700, { chapter: "Open Snippets", note: "Snippets are saved commands. They can hold {{variables}} that are filled in when they run." }),
    move("rail-snippets", 900),
    click("rail-snippets", { view: "snippets" }),
    wait(800),

    move("snippet-uptime", 850, { chapter: "Choose a snippet", note: "Select a snippet to run it here, or on several hosts at once." }),
    click("snippet-uptime", { selected: { snippet: "uptime" } }),
    wait(600),
    move("snippet-run-hosts", 800),
    click("snippet-run-hosts", { panel: "run-hosts" }),
    wait(600),

    move("pick-prod-api", 800, { chapter: "Pick the hosts", note: "Up to eight hosts run at a time. Hosts need saved credentials so nothing waits on a prompt." }),
    click("pick-prod-api", { flags: { "pick-prod-api": true } }),
    move("pick-web-01", 500),
    click("pick-web-01", { flags: { "pick-web-01": true } }),
    move("pick-db", 500),
    click("pick-db", { flags: { "pick-db": true } }),
    wait(500),

    move("run-go", 800, { chapter: "Run and compare", note: "Every host reports its exit code, duration and output, side by side." }),
    click("run-go", { flags: { runDone: true } }),
    toast("3 of 3 hosts finished", 1800),
    wait(2200),
  ],
};

export default def;
