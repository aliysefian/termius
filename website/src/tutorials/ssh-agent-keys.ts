import type { TutorialDefinition } from "@/engine/types";
import { makeHelpers } from "./helpers";

const { click, highlight, move, toast, type: typeText, wait } = makeHelpers(2);

const def: TutorialDefinition = {
  id: "ssh-agent-keys",
  initial: { view: "hosts" },
  cursorStart: { x: 0.85, y: 0.85 },
  posterTime: 28000,
  steps: [
    wait(700, { chapter: "Open the Key Manager", note: "Keys live in the encrypted vault. Credentials refer to keys, so replacing one applies everywhere." }),
    move("rail-keys", 900),
    click("rail-keys", { view: "keys" }),
    wait(900),

    move("key-generate", 900, { chapter: "Generate a key", note: "Generate Ed25519, ECDSA or RSA keys, or import existing ones." }),
    click("key-generate", { panel: "key-form" }),
    wait(400),
    move("k-name", 700),
    click("k-name"),
    typeText("k-name", "deploy-key"),
    move("key-save", 800),
    click("key-save", { panel: null, flags: { keyCreated: true }, selected: { key: "ed-new" } }),
    toast("Key generated; copy the public half for authorized_keys", 2000),
    wait(500),

    move("agent-toggle", 900, { chapter: "Turn on the SSH agent", note: "The built-in agent lets ssh, git and your editor use vault keys. No private key file is written to disk." }),
    click("agent-toggle", { flags: { agentOn: true } }),
    wait(700),
    highlight("agent-sock", 1500),
    wait(300, { note: "Copy this line into your shell profile to point ssh and git at the agent." }),

    move("offer-mode-Ask", 900, { chapter: "Offer the key", note: "Keys are offered one by one: ask every time, or allow while the vault is unlocked." }),
    click("offer-mode-Ask", { selected: { offerMode: "Ask every time" }, flags: { "offer-ed-new": true } }),
    toast("deploy-key is available to the agent", 1700),
    wait(1500),
  ],
};

export default def;
