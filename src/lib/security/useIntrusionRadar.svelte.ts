// Who is logged in, what fail2ban has done, and who has been failing to log in.
import { ask } from "$lib/dialogs.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { errorMessage } from "$lib/types";
import { AUTH_LOG_SCRIPT, FAIL2BAN_SCRIPT, SESSIONS_SCRIPT, banScript, geoScript, killSessionScript, parseAuthOutput, parseFail2ban, parseGeo, parseSessions, summarizeAuthLog, type Attacker, type Fail2ban, type LoginSession, type RecentLogin } from "./intrusion";
import { isIP } from "./net";
import type { FirewallManager } from "./useFirewallManager.svelte";
import type { SecuritySession } from "./securitySession.svelte";

export class IntrusionRadar {
  current = $state<LoginSession[]>([]);
  recent = $state<RecentLogin[]>([]);
  fail2ban = $state<Fail2ban | null>(null);
  attackers = $state<Attacker[]>([]);
  failedLastHour = $state<number | null>(null);
  loading = $state(false);
  loaded = $state(false);
  error = $state<string | null>(null);

  constructor(
    private readonly session: SecuritySession,
    private readonly firewall: FirewallManager,
  ) {}

  /** Every jail's banned addresses, with the jail they are in. */
  get banned() {
    return (this.fail2ban?.jails ?? []).flatMap((j) => j.banned.map((ip) => ({ ip, jail: j.name })));
  }

  get totalBanned() {
    return this.banned.length;
  }

  /** Whether a session is this person's own: the same address the cockpit connects from. */
  isMine(s: LoginSession) {
    const me = this.session.me;
    return !!me?.clientIp && s.source === me.clientIp;
  }

  async refresh() {
    this.loading = true;
    this.error = null;
    try {
      const [sessions, f2b, auth] = await Promise.all([
        this.session.run(SESSIONS_SCRIPT, { mode: "auto" }),
        this.session.run(FAIL2BAN_SCRIPT, { mode: "always" }),
        this.session.run(AUTH_LOG_SCRIPT, { mode: "auto", timeoutSecs: 45 }),
      ]);
      const s = parseSessions(sessions.stdout);
      this.current = s.current;
      this.recent = s.recent;
      this.fail2ban = parseFail2ban(f2b.stdout);
      const { now, log } = parseAuthOutput(auth.stdout);
      const sum = summarizeAuthLog(log, now);
      this.attackers = sum.attackers;
      this.failedLastHour = sum.failedLastHour;
      this.loaded = true;
      void this.#countries();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  async #countries() {
    const ips = this.attackers.map((a) => a.ip);
    if (ips.length === 0) return;
    try {
      const r = await this.session.run(geoScript(ips), { mode: "auto", timeoutSecs: 20 });
      const geo = parseGeo(r.stdout);
      this.attackers = this.attackers.map((a) => ({ ...a, country: geo.get(a.ip) ?? a.country }));
    } catch {
      // Countries are a nicety.
    }
  }

  async #do(what: string, script: string, then: () => Promise<void> | void = () => this.refresh()) {
    try {
      const r = await this.session.run(script, { mode: "always", timeoutSecs: 30 });
      if (r.code !== 0) ui.notify("error", `${what}: ${(r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.code}`}`);
      else ui.notify("info", `${what}: done.`);
    } catch (e) {
      ui.notify("error", `${what}: ${errorMessage(e)}`);
    }
    await then();
  }

  async terminate(s: LoginSession) {
    if (this.isMine(s)) return ui.notify("error", "That session comes from your own address. Close it from its terminal.");
    if (!(await ask(`End everything ${s.user} is running on ${s.tty}${s.source ? ` (from ${s.source})` : ""}? Its processes are killed with SIGKILL.`, { title: "Terminate session", confirm: "Terminate", danger: true }))) return;
    await this.#do(`Terminate ${s.tty}`, killSessionScript(s.tty));
  }

  async unban(ip: string, jail: string) {
    await this.#do(`Unban ${ip}`, banScript(jail, ip, false));
  }

  async ban(ip: string, jail?: string) {
    if (!isIP(ip)) return ui.notify("error", "That isn't an address.");
    const j = jail ?? this.fail2ban?.jails.find((x) => x.name === "sshd")?.name ?? this.fail2ban?.jails[0]?.name;
    if (!j) return ui.notify("error", "fail2ban has no jail to ban in.");
    if (ip === this.session.me?.clientIp) return ui.notify("error", "That is your own address; banning it would lock you out.");
    await this.#do(`Ban ${ip}`, banScript(j, ip, true));
  }

  /** Drop an attacker at the firewall when there is one running, else in fail2ban. */
  async block(ip: string) {
    if (this.firewall.state.active) {
      if (await this.firewall.block(ip, "sshvault: brute force")) await this.refresh();
    } else await this.ban(ip);
  }
}
