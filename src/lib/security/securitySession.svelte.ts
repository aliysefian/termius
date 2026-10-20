// The connection the security cockpit works over: one extra SSH connection to the host (never the shell the person
// is typing in, so nothing lands in its history), who we are there, and the sudo password prompt.
import * as api from "$lib/api";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Uuid } from "$lib/types";
import { execWithSudoFallback, sudoPasswords, SudoError, type SudoResult } from "./sudo";
import { isIP } from "./net";

export interface WhoAmI {
  uid: number;
  user: string;
  home: string;
  /** The address the host sees this connection come from. */
  clientIp: string | null;
  /** The port the host's sshd listens on, as this connection came in. */
  sshPort: number;
}

export function parseWhoAmI(output: string): WhoAmI {
  const [uid = "", user = "", home = "", conn = ""] = output.split(/\r?\n/);
  // SSH_CONNECTION: client address, client port, server address, server port.
  const f = conn.trim().split(/\s+/);
  const port = Number(f[3]);
  return { uid: Number(uid) || 0, user: user.trim(), home: home.trim(), clientIp: f[0] && isIP(f[0]) ? f[0] : null, sshPort: Number.isInteger(port) && port > 0 ? port : 22 };
}

const WHOAMI_SCRIPT = 'id -u; id -un; printf "%s\\n" "$HOME"; printf "%s\\n" "$SSH_CONNECTION"';

export class SecuritySession {
  hostId = $state<Uuid | "">("");
  connecting = $state(false);
  error = $state<string | null>(null);
  me = $state<WhoAmI | null>(null);
  /** Set while sudo wants a password; the cockpit shows a prompt and answers with `answerPassword`. */
  passwordPrompt = $state<{ reason?: string } | null>(null);
  /** The password is remembered for this host until the app closes or the person forgets it. */
  get sudoKnown() {
    return this.hostId ? sudoPasswords.has(this.hostId) : false;
  }
  #id: Uuid | null = null;
  #answer: ((p: string | null) => void) | null = null;

  get connected() {
    return this.#id !== null && this.me !== null;
  }

  get host() {
    return this.hostId ? vaultStore.hostById.get(this.hostId)?.data : undefined;
  }

  async open(hostId: Uuid) {
    await this.close();
    this.hostId = hostId;
    this.connecting = true;
    this.error = null;
    try {
      this.#id = await api.monitor.open(hostId);
      const r = await api.monitor.exec(this.#id, WHOAMI_SCRIPT, 15);
      this.me = parseWhoAmI(r.stdout);
    } catch (e) {
      this.error = errorMessage(e);
      await this.close(false);
    } finally {
      this.connecting = false;
    }
  }

  async close(forget = true) {
    const id = this.#id;
    this.#id = null;
    this.me = null;
    this.answerPassword(null);
    if (forget && this.hostId) sudoPasswords.forget(this.hostId);
    if (id) await api.monitor.close(id).catch(() => {});
  }

  forgetSudo() {
    if (this.hostId) sudoPasswords.forget(this.hostId);
  }

  answerPassword(password: string | null) {
    this.#answer?.(password);
    this.#answer = null;
    this.passwordPrompt = null;
  }

  #ask = (reason?: string) =>
    new Promise<string | null>((resolve) => {
      this.#answer = resolve;
      this.passwordPrompt = { reason };
    });

  /** Run a script, as root when it takes root. `always` goes straight to sudo (a firewall's rules need it). */
  async run(script: string, opts: { mode?: "auto" | "always"; timeoutSecs?: number } = {}): Promise<SudoResult> {
    const id = this.#id;
    if (!id || !this.hostId) throw new Error("Not connected.");
    try {
      return await execWithSudoFallback({
        run: (s, t, stdin) => api.monitor.exec(id, s, t, stdin),
        script,
        timeoutSecs: opts.timeoutSecs,
        key: this.hostId,
        mode: opts.mode ?? "auto",
        askPassword: this.#ask,
      });
    } catch (e) {
      if (e instanceof SudoError && e.kind === "needs-password") throw new Error("That needs root, and no sudo password was given.");
      throw e;
    }
  }
}
