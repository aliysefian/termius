// These run the real scripts in a real shell, so they are skipped where there isn't a POSIX one
// (Windows). The app's type setup has no Node types, hence the suppressed imports.
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { spawn } from "node:child_process";
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { platform } from "node:os";
import { describe, expect, it } from "vitest";
import { DETAIL_SCRIPT, killScript, parseDetailOutput, parseKillOutput, sortProcesses } from "../hostdetail";

const posix = platform() !== "win32";

interface Run {
  stdout: string;
  stderr: string;
  code: number | null;
}

/** Run a script under `sh -c`, as the app does on a host. */
function sh(script: string): Promise<Run> {
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

/** A process we own, to signal. `body` is its shell script. */
function start(body: string): { pid: number; stop: () => void } {
  const p = spawn("sh", ["-c", body], { stdio: "ignore", detached: true });
  return { pid: p.pid as number, stop: () => { try { p.kill("SIGKILL"); } catch { /* already gone */ } } };
}

const alive = async (pid: number) => (await sh(`kill -0 ${pid} 2>/dev/null && echo yes || echo no`)).stdout.trim() === "yes";
const settle = (ms = 300) => new Promise((r) => setTimeout(r, ms));

describe.skipIf(!posix)("the sampling script, run here", () => {
  it("runs once, quietly, and its output reads back whole", async () => {
    const started = Date.now();
    const r = await sh(DETAIL_SCRIPT);
    const took = Date.now() - started;
    expect(r.code).toBe(0);
    expect(r.stderr).toBe("");
    // It samples twice, a second apart, and does no more than that.
    expect(took).toBeGreaterThan(900);
    expect(took).toBeLessThan(8000);
    const d = parseDetailOutput(r.stdout);
    if (d.os !== "Linux") return;
    expect(d.cpuPct).not.toBeNull();
    expect(d.memPct).toBeGreaterThan(0);
    expect(d.processes.length).toBeGreaterThan(3);
    expect(d.rates.length).toBeGreaterThan(0);
    expect(d.interfaces.length).toBeGreaterThan(0);
    expect(d.diskPct).not.toBeNull();
    expect(d.uptimeSecs).toBeGreaterThan(0);
  });

  it("sees a busy process of ours, with its name and user, using more CPU than an idle one beside it", async () => {
    if ((await sh("uname -s")).stdout.trim() !== "Linux") return;
    const busy = start("while :; do :; done");
    const idle = start("while :; do sleep 1; done");
    try {
      await settle();
      const d = parseDetailOutput((await sh(DETAIL_SCRIPT)).stdout);
      const mine = d.processes.find((p) => p.pid === busy.pid);
      const quiet = d.processes.find((p) => p.pid === idle.pid);
      expect(mine).toBeDefined();
      expect(quiet).toBeDefined();
      expect(mine!.name).toBe("sh");
      expect(mine!.user).not.toBe("");
      // How much CPU the loop gets depends on what else is running (the other tests are), so the
      // claim is relative: it is using CPU, and the sleeper beside it is not.
      expect(mine!.cpuPct).toBeGreaterThan(0);
      expect(mine!.cpuPct).toBeGreaterThan(quiet!.cpuPct);
    } finally {
      busy.stop();
      idle.stop();
    }
  });

  it("changes nothing it was only asked to read", () => {
    const code = DETAIL_SCRIPT.split("\n").filter((l) => !l.trim().startsWith("#")).join("\n");
    // No command that writes, deletes or signals.
    expect(code).not.toMatch(/\b(kill|rm|mv|cp|chmod|chown|tee|dd|mkfs|touch|mkdir|sed -i)\b/);
    // The only redirections are to /dev/null or between its own streams: it writes no file.
    // (The awk program in single quotes has comparisons like `k > 0`, which aren't redirections.)
    const rest = code.replace(/'[^']*'/g, "''").replace(/\d?>\s*\/dev\/null/g, "").replace(/\d?>&\d/g, "");
    expect(rest).not.toContain(">");
  });
});

describe.skipIf(!posix)("signalling a process, for real", () => {
  const OS = "Linux";
  const linux = async () => (await sh("uname -s")).stdout.trim() === OS;

  it("TERM ends an ordinary process, and says so", async () => {
    if (!(await linux())) return;
    const p = start("while :; do sleep 1; done");
    try {
      await settle();
      expect(await alive(p.pid)).toBe(true);
      const r = await sh(killScript(OS, p.pid, "sh", "TERM"));
      expect(parseKillOutput(r.stdout, r.stderr, r.code)).toEqual({ kind: "ok" });
      await settle(500);
      expect(await alive(p.pid)).toBe(false);
    } finally {
      p.stop();
    }
  });

  it("a process that ignores TERM survives it, and KILL is the separate action that ends it", async () => {
    if (!(await linux())) return;
    const p = start(`trap '' TERM; while :; do sleep 1; done`);
    try {
      await settle();
      expect(parseKillOutput(...Object.values(await sh(killScript(OS, p.pid, "sh", "TERM"))) as [string, string, number | null])).toEqual({ kind: "ok" });
      await settle(500);
      expect(await alive(p.pid), "TERM was sent, but the process ignored it").toBe(true);
      const r = await sh(killScript(OS, p.pid, "sh", "KILL"));
      expect(parseKillOutput(r.stdout, r.stderr, r.code)).toEqual({ kind: "ok" });
      await settle(500);
      expect(await alive(p.pid)).toBe(false);
    } finally {
      p.stop();
    }
  });

  it("leaves a process alone when the pid now belongs to something with another name", async () => {
    if (!(await linux())) return;
    const p = start("while :; do sleep 1; done");
    try {
      await settle();
      // The screen showed "nginx" for this pid; the pid has since been reused by a shell.
      const r = await sh(killScript(OS, p.pid, "nginx", "KILL"));
      expect(parseKillOutput(r.stdout, r.stderr, r.code)).toEqual({ kind: "changed" });
      expect(await alive(p.pid), "a different process must not be signalled").toBe(true);
    } finally {
      p.stop();
    }
  });

  it("reports a process that has already gone", async () => {
    if (!(await linux())) return;
    const p = start("exit 0");
    await settle(500);
    const r = await sh(killScript(OS, p.pid, "sh", "TERM"));
    expect(parseKillOutput(r.stdout, r.stderr, r.code)).toEqual({ kind: "gone" });
  });

  it("passes the host's refusal on, in its own words", async () => {
    if (!(await linux())) return;
    const p = start("while :; do sleep 1; done");
    try {
      await settle();
      // A kill that fails, as it would for a process owned by someone else.
      const stub = `kill() { echo "sh: kill: (${p.pid}) - Operation not permitted" >&2; return 1; }\n`;
      const r = await sh(stub + killScript(OS, p.pid, "sh", "TERM"));
      const out = parseKillOutput(r.stdout, r.stderr, r.code);
      expect(out).toEqual({ kind: "failed", reason: `sh: kill: (${p.pid}) - Operation not permitted` });
    } finally {
      p.stop();
    }
  });

  it("a name that tries to run a command is just a name that doesn't match", async () => {
    if (!(await linux())) return;
    const p = start("while :; do sleep 1; done");
    try {
      await settle();
      const marker = `/tmp/sshvault-kill-injection-${p.pid}`;
      const r = await sh(killScript(OS, p.pid, `sh'; touch ${marker}; echo '`, "TERM"));
      expect(parseKillOutput(r.stdout, r.stderr, r.code)).toEqual({ kind: "changed" });
      const exists = await sh(`[ -e ${marker} ] && echo yes || echo no`);
      expect(exists.stdout.trim()).toBe("no");
    } finally {
      p.stop();
    }
  });
});

// The summary script's BSD branch, run with the system tools replaced by stubs that answer in the
// documented formats. HAND-WRITTEN output: no Mac or FreeBSD was available to record from.
import { METRICS_SCRIPT, parseMetricsOutput } from "../hostmetrics";

describe.skipIf(!posix)("the summary script on macOS and FreeBSD (stubbed tools)", () => {
  const run = async (stubs: string) => parseMetricsOutput((await sh(stubs + "\n" + METRICS_SCRIPT)).stdout);
  // Replace /proc detection by pretending there is no /proc: the script tests `[ -f /proc/stat ]`.
  const noProc = (script: string) => script.replace("[ -f /proc/stat ]", "[ -f /nonexistent/stat ]");
  const runBsd = async (stubs: string) => parseMetricsOutput((await sh(stubs + "\n" + noProc(METRICS_SCRIPT))).stdout);

  const darwin = `
uname() { echo Darwin; }
top() {
  cat <<'__T__'
Processes: 400 total, 2 running, 398 sleeping, 2000 threads
CPU usage: 30.0% user, 20.0% sys, 50.0% idle
Processes: 401 total, 3 running, 398 sleeping, 2001 threads
CPU usage: 3.2% user, 5.1% sys, 91.6% idle
__T__
}
sysctl() {
  case "$*" in
    *hw.memsize*) echo 17179869184 ;;
    *vm.loadavg*) echo "{ 1.84 1.62 1.50 }" ;;
    *kern.boottime*) echo "{ sec = 1791200000, usec = 0 } Sun Oct  4 00:00:00 2026" ;;
  esac
}
vm_stat() {
  cat <<'__V__'
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               20000.
Pages active:                            400000.
Pages inactive:                          300000.
Pages speculative:                        10000.
Pages wired down:                        150000.
Pages occupied by compressor:             50000.
__V__
}
`;

  it("macOS: CPU is 100 minus the idle of the second top display, memory is active + wired + compressed", async () => {
    const m = await runBsd(darwin);
    expect(m.cpuPct).toBe(8);
    // (400000 + 150000 + 50000) pages * 16384 bytes of 16 GiB = 9830400000 / 17179869184 = 57%
    expect(m.memPct).toBe(57);
    expect(m.load).toBe("1.84 1.62 1.50");
  });

  it("macOS: anything it can't read stays empty rather than becoming 0", async () => {
    const m = await runBsd(`uname() { echo Darwin; }\ntop() { :; }\nvm_stat() { :; }\nsysctl() { :; }`);
    expect(m.cpuPct).toBeNull();
    expect(m.memPct).toBeNull();
  });

  it("FreeBSD: CPU from top's CPU line, memory is everything but free and inactive", async () => {
    const m = await runBsd(`
uname() { echo FreeBSD; }
top() {
  cat <<'__T__'
last pid:  1234;  load averages:  0.52,  0.40,  0.31
CPU: 10.0% user,  0.0% nice,  5.0% system,  0.0% interrupt, 85.0% idle
last pid:  1235;  load averages:  0.52,  0.40,  0.31
CPU:  1.2% user,  0.0% nice,  0.5% system,  0.0% interrupt, 98.3% idle
__T__
}
sysctl() {
  case "$*" in
    *hw.physmem*) printf '%s\\n' 8589934592 4096 100000 900000 ;;
    *vm.loadavg*) echo "{ 0.52 0.40 0.31 }" ;;
    *kern.boottime*) echo "{ sec = 1791200000, usec = 0 }" ;;
  esac
}
`);
    expect(m.cpuPct).toBe(2);
    // physmem 8 GiB = 2097152 pages; free + inactive = 1000000; used = 1097152 / 2097152 = 52%
    expect(m.memPct).toBe(52);
  });

  it("another BSD gets what it always did: load and uptime, no CPU or memory", async () => {
    const m = await runBsd(`uname() { echo OpenBSD; }\nsysctl() { case "$*" in *vm.loadavg*) echo "{ 0.1 0.2 0.3 }";; esac; }`);
    expect(m.cpuPct).toBeNull();
    expect(m.memPct).toBeNull();
    expect(m.load).toBe("0.1 0.2 0.3");
  });

  it("is unchanged on Linux", async () => {
    if ((await sh("uname -s")).stdout.trim() !== "Linux") return;
    const m = await run("");
    expect(m.cpuPct).not.toBeNull();
    expect(m.memPct).toBeGreaterThan(0);
  });
});
