import type { TutorialDefinition } from "@/engine/types";
import { c } from "@/components/sim/ansi";
import { click, move, out, progress, run, set, toast, type as typeText, wait } from "./helpers";

const PROMPT = "demo@new-host:~$ ";

const def: TutorialDefinition = {
  id: "create-ssh-connection",
  initial: { view: "hosts" },
  cursorStart: { x: 0.8, y: 0.85 },
  posterTime: 21000,
  steps: [
    wait(900, { chapter: "Open the new host form", note: "The Hosts page lists every saved host, grouped. The + button adds one." }),
    move("host-add", 900),
    click("host-add", { panel: "host-form", flags: { editingHost: false } }),
    wait(500),

    move("f-label", 750, { chapter: "Fill in the connection", note: "Give the host a label, its address and port, and the user to log in as." }),
    click("f-label"),
    typeText("f-label", "Demo Server"),
    move("f-hostname", 500),
    click("f-hostname"),
    typeText("f-hostname", "203.0.113.50", { cps: 11 }),
    move("f-port", 450),
    click("f-port"),
    typeText("f-port", "22"),
    move("f-username", 450),
    click("f-username"),
    typeText("f-username", "demo"),
    move("f-auth-key", 550, { note: "Choose how to log in. An SSH key from the vault means no password is typed or stored in the host." }),
    click("f-auth-key", { selected: { auth: "key" } }),
    wait(500),

    move("f-save", 800, { chapter: "Save the host", note: "Save adds the host to the encrypted vault." }),
    click("f-save", { panel: null, flags: { created: true } }),
    toast("Host saved to the vault", 1400),
    wait(300),

    move("host-row-new", 800, { chapter: "Select it and connect", note: "Select the new host to see its details, then press Connect." }),
    click("host-row-new", { selected: { host: "new" } }),
    wait(600),
    move("host-connect", 700),
    click("host-connect", { panel: "connecting", progress: { connect: 0 } }),
    progress("connect", 0, 70, 1400),
    set({ panel: "hostkey" }, { note: "The first time, SSHVault shows the server's fingerprint. Trusting it pins the key for later." }),
    wait(900),
    move("trust-key", 800),
    click("trust-key", { panel: "connecting" }),
    progress("connect", 70, 100, 900),
    set({
      panel: null,
      view: "terminal",
      flags: { "tab-new": true, connectedNew: true },
      selected: { tab: "new" },
      terminals: {
        new: [
          "Welcome to Ubuntu 24.04 LTS (GNU/Linux 6.8.0-45-generic x86_64)",
          "",
          "Last login: Sun Nov 15 21:40:12 2026 from 198.51.100.9",
        ],
      },
    }),
    toast("Connected to Demo Server", 1500),
    wait(500),

    move("term-new", 800, { chapter: "Run a command", note: "The session opens in a terminal tab. Type a command as usual." }),
    click("term-new"),
    run("new", PROMPT, "whoami"),
    out("new", ["demo"], 350),
    wait(500),
    run("new", PROMPT, "uname -sr"),
    out("new", ["Linux 6.8.0-45-generic"], 350),
    wait(500),
    run("new", PROMPT, "uptime"),
    out("new", [" 14:20:01 up 41 days,  3:12,  1 user,  load average: " + c.g("0.12, 0.10, 0.08")], 400),
    wait(1800, { note: "That is the whole flow: add a host, trust its key, connect. All data here is fictional." }),
  ],
};

export default def;
