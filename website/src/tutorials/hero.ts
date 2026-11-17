import type { TutorialDefinition } from "@/engine/types";
import { c } from "@/components/sim/ansi";
import { makeHelpers } from "./helpers";

const { click, dblclick, move, out, progress, run, set, wait } = makeHelpers(1);

const P = "demo@prod-api:~$ ";

/** The 20-second loop on the home page: pick a host, connect, run a command. */
const def: TutorialDefinition = {
  id: "hero",
  initial: { view: "hosts" },
  cursorStart: { x: 0.85, y: 0.8 },
  posterTime: 12500,
  steps: [
    wait(600, { chapter: "Find a host" }),
    move("host-row-prod-api", 1000),
    click("host-row-prod-api", { selected: { host: "prod-api" } }),
    wait(700),
    move("host-connect", 800),
    dblclick("host-connect", { panel: "connecting", progress: { connect: 0 } }, { chapter: "Connect" }),
    progress("connect", 0, 100, 1300),
    set({
      panel: null,
      view: "terminal",
      flags: { "tab-prod-api": true },
      selected: { tab: "prod-api" },
      terminals: { "prod-api": ["Welcome to Ubuntu 24.04 LTS (GNU/Linux 6.8.0-45-generic x86_64)", ""] },
    }),
    wait(500),
    move("term-prod-api", 800, { chapter: "Work" }),
    click("term-prod-api"),
    run("prod-api", P, "systemctl is-active app"),
    out("prod-api", [c.g("active")], 300),
    wait(300),
    run("prod-api", P, "df -h /srv | tail -1"),
    out("prod-api", ["/dev/sda2  80G   31G   46G  41% /srv"], 400),
    wait(2200),
  ],
};

export default def;
