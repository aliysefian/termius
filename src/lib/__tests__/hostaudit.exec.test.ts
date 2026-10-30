// Runs the real collection script in a real shell, as the app does on a host, so it is skipped where there isn't a
// POSIX one (Windows). The app's type setup has no Node types, hence the suppressed imports.
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { spawn } from "node:child_process";
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { platform, userInfo } from "node:os";
import { describe, expect, it } from "vitest";
import { AUDIT_SCRIPT, auditHost } from "../hostaudit";

const posix = platform() !== "win32";

function sh(script: string): Promise<{ stdout: string; stderr: string; code: number | null }> {
  return new Promise((resolve, reject) => {
    const p = spawn("sh", ["-c", script]);
    let stdout = "";
    let stderr = "";
    p.stdout.on("data", (d: unknown) => (stdout += String(d)));
    p.stderr.on("data", (d: unknown) => (stderr += String(d)));
    p.on("error", reject);
    p.on("close", (code: number | null) => resolve({ stdout, stderr, code }));
  });
}

describe("the collection script only reads", () => {
  // The script is the one thing that runs on someone's server, so what it may do is checked as text: no sudo, nothing
  // that writes, installs, starts, stops, deletes, signals or sends, and no output redirected anywhere but nowhere.
  const code = AUDIT_SCRIPT.split("\n").filter((l) => !/^\s*#/.test(l)).join("\n");

  it("has none of the commands that change a host or talk to the network", () => {
    const forbidden = [
      /\bsudo\b/, /\bsu\b\s+-?\w/, /\brm\b/, /\bmv\b/, /\bcp\b/, /\bchmod\b/, /\bchown\b/, /\bkill(all)?\b/, /\bpkill\b/, /\bmkdir\b/, /\btouch\b/, /\btee\b/, /\bdd\b/, /\bsed\s+-i/,
      /\bsystemctl\s+(start|stop|restart|reload|enable|disable|mask|unmask|daemon-reload|edit)\b/, /\bservice\s+\w+\s+(start|stop|restart)\b/,
      /\bapt(-get)?\s+(install|remove|purge|upgrade|dist-upgrade|update|autoremove|clean)\b/, /\bdnf\s+(install|remove|upgrade|update)\b/, /\byum\s+(install|remove|update)\b/,
      /\bdocker\s+(run|exec|rm|rmi|stop|start|restart|kill|pull|push|build|create|cp|commit|login|network|volume|system)\b/,
      /\bcurl\b/, /\bwget\b/, /\bnc\b/, /\bncat\b/, /(?<![/.\w-])ssh(?![\w/.-])/, /\bscp\b/, /\bping\b/, /\bnmap\b/, /\beval\b/, /\bsysctl\s+-w\b/, /\bufw\s+(enable|disable|allow|deny|delete|reset|reload)\b/,
      /\bfail2ban-client\s+(set|start|stop|reload|unban)\b/, /\buseradd\b|\busermod\b|\buserdel\b|(?<![/.\w-])passwd(?![\w/.-])/, /\bcrontab\b/,
    ];
    for (const re of forbidden) expect(code, String(re)).not.toMatch(re);
  });

  it("writes nowhere: every redirection goes to /dev/null or merges stderr", () => {
    const rest = code.replace(/2>\/dev\/null/g, "").replace(/>\/dev\/null/g, "").replace(/2>&1/g, "");
    expect(rest).not.toContain(">");
  });

  it("never prints a password hash: /etc/shadow is only asked whether a field is empty", () => {
    const uses = code.split("\n").filter((l) => l.includes("/etc/shadow"));
    expect(uses.length).toBeGreaterThan(0);
    for (const l of uses) expect(l).toMatch(/stat |-r \/etc\/shadow|awk -F: '\$2 == ""/);
    expect(code).not.toMatch(/cat\s+\/etc\/shadow|\/etc\/shadow\s*\|/);
  });
});

describe.skipIf(!posix)("the collection script, run here", () => {
  it("runs once, quietly, in a plain shell, and what it prints reads back as a whole report", async () => {
    const started = Date.now();
    const r = await sh(AUDIT_SCRIPT);
    const took = Date.now() - started;
    expect(r.code).toBe(0);
    expect(r.stderr).toBe("");
    // Every slow tool is bounded, so a healthy host is done well inside a minute.
    expect(took).toBeLessThan(60_000);

    const report = auditHost(r.stdout);
    if (report.os !== "Linux") {
      expect(report.supported).toBe(false);
      return;
    }
    expect(report.supported).toBe(true);
    expect(report.complete).toBe(true);
    expect(report.root).toBe(userInfo().uid === 0);
    expect(report.checks.length).toBeGreaterThan(25);
    expect(new Set(report.checks.map((c) => c.id)).size).toBe(report.checks.length);
    for (const c of report.checks) {
      expect(c.finding, c.id).not.toBe("");
      if (c.outcome === "fail") {
        expect(c.severity, c.id).not.toBeNull();
        expect(c.risk, c.id).not.toBe("");
        expect(c.remediation, c.id).not.toBe("");
      }
      if (c.outcome === "unknown") expect(c.remediation, c.id).not.toBe("");
    }
    // The score is made only of what was judged.
    expect(report.score.passed + report.score.failed + report.score.unknown).toBe(report.checks.length);
  }, 90_000);

  it("changes nothing it was run beside: the files it reads keep their modification times", async () => {
    const stat = async () => (await sh("stat -c '%Y %n' /etc/passwd /etc/ssh/sshd_config /etc/login.defs 2>/dev/null")).stdout;
    const before = await stat();
    await sh(AUDIT_SCRIPT);
    expect(await stat()).toBe(before);
  }, 90_000);
});
