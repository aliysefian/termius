import type { TutorialDefinition } from "@/engine/types";
import { click, move, set, toast, type as typeText, wait } from "./helpers";

const def: TutorialDefinition = {
  id: "port-forwarding",
  initial: { view: "hosts" },
  cursorStart: { x: 0.85, y: 0.85 },
  posterTime: 26000,
  steps: [
    wait(700, { chapter: "Open Tunnels", note: "The Tunnels page lists every forwarding rule with its type, mapping, host and state." }),
    move("rail-tunnels", 900),
    click("rail-tunnels", { view: "tunnels" }),
    wait(900),

    move("tunnel-create", 900, { chapter: "Create a tunnel", note: "Create Tunnel walks through the type and the details. Local is the equivalent of ssh -L." }),
    click("tunnel-create", { panel: "tunnel-form", selected: { tunnelType: "Local" } }),
    wait(500),
    move("tn-name", 700),
    click("tn-name"),
    typeText("tn-name", "Admin UI"),
    move("tn-lport", 500),
    click("tn-lport"),
    typeText("tn-lport", "8080", { cps: 6 }),
    move("tn-dest", 500),
    click("tn-dest"),
    typeText("tn-dest", "localhost"),
    move("tn-dport", 500),
    click("tn-dport"),
    typeText("tn-dport", "80", { cps: 5 }),
    wait(800, { note: "The equivalent OpenSSH command is shown as you type." }),
    move("tunnel-save", 800),
    click("tunnel-save", { panel: null, flags: { tunnelCreated: true } }),
    toast("Tunnel created (stopped)", 1500, "info"),

    move("tunnel-row-t-new", 850, { chapter: "Start it", note: "Creating and starting are separate. Select the tunnel and press Start." }),
    click("tunnel-row-t-new", { selected: { tunnel: "t-new" } }),
    wait(500),
    move("tunnel-start", 700),
    click("tunnel-start", { flags: { tunnelRunning: true } }),
    toast("Listening on 127.0.0.1:8080", 1800),

    move("tunnel-open", 800, { chapter: "Use it", note: "Anything on this computer can now reach the remote port through 127.0.0.1:8080, and nothing else is exposed." }),
    set({}),
    wait(2200),
  ],
};

export default def;
