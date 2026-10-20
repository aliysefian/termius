// The firewall view's state and actions. Every change goes through the same gate: the checks that keep this
// connection alive, then a confirmation on production hosts, then the script over the cockpit's connection.
import { ask } from "$lib/dialogs.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { envInfo, errorMessage } from "$lib/types";
import { allowScript, blockScript, checkAddDeny, checkBlock, checkDelete, checkEnable, deleteScript, enableUfwScript, incomingVerdict, parseFirewall, EMPTY_STATE, NO_TIMER, PROBE_SCRIPT, type FirewallState, type FwRule } from "./firewall";
import { isSource } from "./net";
import type { SecuritySession } from "./securitySession.svelte";

export class FirewallManager {
  state = $state<FirewallState>(EMPTY_STATE);
  loading = $state(false);
  loaded = $state(false);
  error = $state<string | null>(null);
  busy = $state<string | null>(null);

  constructor(private readonly session: SecuritySession) {}

  get ctx() {
    return { clientIp: this.session.me?.clientIp ?? null, sshPort: this.session.me?.sshPort ?? 22 };
  }

  get production() {
    return envInfo(vaultStore.effectiveEnv(this.session.host)).value === "production";
  }

  /** Whether the way in is open, for the header. */
  get sshOpen(): "allow" | "deny" | "unknown" {
    const { clientIp, sshPort } = this.ctx;
    return clientIp ? incomingVerdict(this.state, sshPort, clientIp) : "unknown";
  }

  async refresh() {
    this.loading = true;
    this.error = null;
    try {
      // `always`: every one of these tools wants root.
      const r = await this.session.run(PROBE_SCRIPT, { mode: "always" });
      this.state = parseFirewall(r.stdout);
      this.loaded = true;
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  /** Confirm if this is production, run the script, say what happened, and reload. */
  async #apply(what: string, script: string): Promise<boolean> {
    const host = this.session.host;
    if (this.production && host) {
      const ok = await ask(`${what} on ${host.label}? This is a production host.`, { title: "Production host", confirm: "Apply", danger: true, requireText: host.label });
      if (!ok) return false;
    }
    this.busy = what;
    try {
      const r = await this.session.run(script, { mode: "always", timeoutSecs: 60 });
      if (r.code !== 0) {
        const why = r.code === NO_TIMER ? "This host has neither systemd-run nor at, so a rule that expires can't be set up. Nothing was added." : (r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.code}`;
        ui.notify("error", `${what}: ${why}`);
        return false;
      }
      ui.notify("info", `${what}: done.`);
      return true;
    } catch (e) {
      ui.notify("error", `${what}: ${errorMessage(e)}`);
      return false;
    } finally {
      this.busy = null;
      void this.refresh();
    }
  }

  async allowMyIp(port: string, proto: "tcp" | "udp", ttlSecs?: number) {
    const ip = this.session.me?.clientIp;
    if (!ip) return ui.notify("error", "The host did not say which address you are connecting from.");
    try {
      const script = allowScript(this.state, { ip, port, proto, ttlSecs, comment: "sshvault: my ip" });
      await this.#apply(`Allow ${ip}${port ? ` on ${port}/${proto}` : ""}${ttlSecs ? " (temporary)" : ""}`, script);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async block(source: string, reason = "") {
    if (!isSource(source)) return ui.notify("error", "That isn't an address or subnet.");
    const v = checkBlock(source, this.ctx);
    if (!v.ok) return ui.notify("error", v.reason ?? "That would lock you out.");
    const sshHit = checkAddDeny(this.state, "", source, this.ctx);
    if (!sshHit.ok) return ui.notify("error", sshHit.reason ?? "That would lock you out.");
    try {
      return await this.#apply(`Block ${source}`, blockScript(this.state, source, reason));
    } catch (e) {
      ui.notify("error", errorMessage(e));
      return false;
    }
  }

  async remove(rule: FwRule) {
    const v = checkDelete(this.state, rule, this.ctx);
    if (!v.ok) return ui.notify("error", `Not deleted. ${v.reason}`);
    try {
      const script = deleteScript(this.state, rule);
      if (!(await ask(`Delete this rule?\n\n${rule.raw}`, { title: "Delete rule", confirm: "Delete", danger: true }))) return;
      await this.#apply("Delete rule", script);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  /** Turn ufw on. If nothing keeps SSH open, offer to add that first; the order of the two keeps the connection. */
  async enable() {
    if (this.state.backend !== "ufw") return ui.notify("error", "Turning on is only offered for ufw. Start the firewall service on the host.");
    const v = checkEnable(this.state, this.ctx);
    if (!v.ok && !(await ask(`${v.reason}\n\nAdd a rule for it first, then turn the firewall on?`, { title: "Keep this connection open", confirm: "Add the rule and enable" }))) return;
    await this.#apply("Enable ufw", enableUfwScript(this.ctx.sshPort, this.ctx.clientIp, v.ok));
  }
}
