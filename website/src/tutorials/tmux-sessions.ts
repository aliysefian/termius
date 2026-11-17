import type { TutorialDefinition } from "@/engine/types";
import { c } from "@/components/sim/ansi";
import { makeHelpers } from "./helpers";

const { click, highlight, move, set, toast, wait } = makeHelpers(2);

const def: TutorialDefinition = {
  id: "tmux-sessions",
  initial: { view: "hosts", selected: { host: "prod-api" } },
  cursorStart: { x: 0.85, y: 0.85 },
  posterTime: 26000,
  steps: [
    wait(700, { chapter: "Open the Tmux page", note: "tmux sessions live on the server. SSHVault reads them over a connection it already keeps, without opening a terminal." }),
    move("rail-tmux", 900),
    click("rail-tmux", { view: "tmux" }),
    wait(900),

    move("tmux-session-deploy", 900, { chapter: "Pick a session", note: "Each host lists its sessions, with an attached marker." }),
    click("tmux-session-deploy", { selected: { session: "$0", window: "@0" } }),
    wait(1200, { note: "The window's panes appear as read-only live previews, laid out as tmux has them." }),

    move("tmux-pane-1", 900, { chapter: "Inspect the panes", note: "Select a pane to see its details and actions in the inspector." }),
    click("tmux-pane-1", { selected: { pane: "%1" } }),
    highlight("tmux-pane-1", 1100),
    wait(500),

    move("tmux-win-logs", 900, { chapter: "Switch windows", note: "Windows of the selected session are listed under it." }),
    click("tmux-win-logs", { selected: { window: "@1", pane: "" } }),
    wait(1400),
    move("tmux-win-editor", 800),
    click("tmux-win-editor", { selected: { window: "@0", pane: "" } }),
    wait(700),

    move("tmux-attach", 900, { chapter: "Attach to the session", note: "Attach opens a real terminal tab on the session. Closing the tab only detaches; the session keeps running." }),
    click("tmux-attach", {
      view: "terminal",
      flags: { "tab-prod-api": true },
      selected: { tab: "prod-api" },
      terminals: {
        "prod-api": [
          "demo@prod-api:~$ tmux attach-session -t '$0'",
          "",
          "12:01:07 GET /health 200 3ms",
          "12:01:09 GET /v1/hosts 200 18ms",
          "12:01:12 POST /v1/sync 202 41ms",
          "",
          c.g("[deploy] 0:editor* 1:logs                              prod-api  14:21 16-Nov-26"),
        ],
      },
    }),
    toast("Attached to session “deploy”", 1700),
    set({}),
    wait(1500),
  ],
};

export default def;
