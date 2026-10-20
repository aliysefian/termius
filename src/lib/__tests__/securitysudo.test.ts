import { beforeEach, describe, expect, it } from "vitest";
import { SudoError, classifySudo, execWithSudoFallback, looksDenied, sudoCommand, sudoPasswords, type ExecResult, type Runner } from "$lib/security/sudo";

const ok = (stdout = ""): ExecResult => ({ stdout, stderr: "", code: 0 });
const fail = (stderr: string, code = 1): ExecResult => ({ stdout: "", stderr, code });

/** A fake host: `user` or `root`, and a sudo that wants `password` (or none). */
function host(opts: { root?: boolean; password?: string | null; denies?: boolean; notAllowed?: boolean }) {
  const calls: { script: string; stdin?: string }[] = [];
  const run: Runner = async (script, _t, stdin) => {
    calls.push({ script, stdin });
    if (script === "id -u") return ok(opts.root ? "0\n" : "1000\n");
    if (script.startsWith("sudo")) {
      if (opts.notAllowed) return fail("sudo: ali is not in the sudoers file.");
      if (script.startsWith("sudo -n")) return opts.password === null ? ok("did it") : fail("sudo: a password is required");
      if (stdin?.trim() === opts.password) return ok("did it");
      return fail("Sorry, try again.\nsudo: incorrect password attempt");
    }
    return opts.denies ? fail("ufw: ERROR: You need to be root to run this script") : ok("plain");
  };
  return { run, calls };
}

beforeEach(() => sudoPasswords.forget("h"));

describe("sudo", () => {
  it("reads what sudo says", () => {
    expect(classifySudo("sudo: a password is required", false)).toBe("needs-password");
    expect(classifySudo("sudo: no tty present and no askpass program specified", false)).toBe("needs-password");
    expect(classifySudo("Sorry, try again.", true)).toBe("wrong-password");
    expect(classifySudo("ali is not in the sudoers file.", false)).toBe("not-allowed");
    expect(classifySudo("ufw: bad rule", false)).toBeNull();
  });
  it("puts the script in one quoted word, and never the password in the command", () => {
    expect(sudoCommand("echo 'hi'", false)).toBe(`sudo -n sh -c 'echo '\\''hi'\\'''`);
    expect(sudoCommand("id", true)).toBe("sudo -S -p '' sh -c 'id'");
  });
  it("tells a refusal from a failure", () => {
    expect(looksDenied(fail("iptables: Permission denied (you must be root)"))).toBe(true);
    expect(looksDenied(fail("ufw: bad rule"))).toBe(false);
    expect(looksDenied(ok())).toBe(false);
  });

  it("runs as the user when that works", async () => {
    const h = host({});
    const r = await execWithSudoFallback({ run: h.run, script: "uptime", key: "h", askPassword: async () => null });
    expect(r).toMatchObject({ stdout: "plain", usedSudo: false });
    expect(h.calls.some((c) => c.script.startsWith("sudo"))).toBe(false);
  });
  it("runs as root without sudo when already root", async () => {
    const h = host({ root: true });
    const r = await execWithSudoFallback({ run: h.run, script: "ufw status", key: "h", mode: "always", askPassword: async () => null });
    expect(r.usedSudo).toBe(false);
  });
  it("goes to sudo -n when denied, and needs no password where there is none", async () => {
    const h = host({ denies: true, password: null });
    const r = await execWithSudoFallback({ run: h.run, script: "ufw status", key: "h", askPassword: async () => { throw new Error("must not ask"); } });
    expect(r).toMatchObject({ stdout: "did it", usedSudo: true });
  });
  it("asks once, sends the password on stdin, and remembers it", async () => {
    const h = host({ password: "hunter2" });
    let asked = 0;
    const askPassword = async () => (asked++, "hunter2");
    const first = await execWithSudoFallback({ run: h.run, script: "ufw status", key: "h", mode: "always", askPassword });
    expect(first.usedSudo).toBe(true);
    const second = await execWithSudoFallback({ run: h.run, script: "ufw status", key: "h", mode: "always", askPassword });
    expect(second.stdout).toBe("did it");
    expect(asked).toBe(1);
    const withPw = h.calls.filter((c) => c.stdin);
    expect(withPw.every((c) => c.stdin === "hunter2\n" && !c.script.includes("hunter2"))).toBe(true);
  });
  it("asks again after a wrong password, saying so, and gives up after three", async () => {
    const h = host({ password: "right" });
    const reasons: (string | undefined)[] = [];
    const answers = ["nope", "right"];
    const r = await execWithSudoFallback({ run: h.run, script: "x", key: "h", mode: "always", askPassword: async (why) => (reasons.push(why), answers.shift() ?? null) });
    expect(r.usedSudo).toBe(true);
    expect(reasons[0]).toBeUndefined();
    expect(reasons[1]).toMatch(/did not accept/);
    sudoPasswords.forget("h");
    await expect(execWithSudoFallback({ run: host({ password: "right" }).run, script: "x", key: "h", mode: "always", askPassword: async () => "wrong" })).rejects.toMatchObject({ kind: "wrong-password" });
  });
  it("stops when the person cancels, or the user may not use sudo", async () => {
    await expect(execWithSudoFallback({ run: host({ password: "p" }).run, script: "x", key: "h", mode: "always", askPassword: async () => null })).rejects.toBeInstanceOf(SudoError);
    await expect(execWithSudoFallback({ run: host({ notAllowed: true }).run, script: "x", key: "h", mode: "always", askPassword: async () => "p" })).rejects.toMatchObject({ kind: "not-allowed" });
  });
});
