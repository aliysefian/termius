// @ts-nocheck: this file runs the generated scripts with the real `sh` in a throw-away home folder.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { BACKUP_SUFFIX, DETECT_SCRIPT, MARK_BEGIN, MARK_END, RC_FILE, blockFor, installScript, parseDetect, removeScript, snippetFor } from "../integrationinstall";

const homes: string[] = [];
function home(files: Record<string, string> = {}) {
  const h = mkdtempSync(join(tmpdir(), "sv-int-"));
  homes.push(h);
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(join(h, name, ".."), { recursive: true });
    writeFileSync(join(h, name), text);
  }
  return h;
}
afterAll(() => homes.forEach((h) => rmSync(h, { recursive: true, force: true })));

const sh = (script: string, h: string, shell = "/bin/bash") => execFileSync("sh", ["-c", script], { env: { PATH: process.env.PATH, HOME: h, SHELL: shell }, encoding: "utf8" }).trim();
const rc = (shell: string) => RC_FILE[shell].replace("$HOME/", "");

describe("the block", () => {
  it.each(["bash", "zsh", "fish"])("%s: is the integration lines between markers, and the marker can't be mistaken for code", (shell) => {
    const b = blockFor(shell);
    expect(b.startsWith(MARK_BEGIN + "\n") && b.endsWith("\n" + MARK_END)).toBe(true);
    expect(b).toContain("133");
    expect(snippetFor(shell)).not.toMatch(/^# SSHVault shell integration/);
  });
});

describe.each(["bash", "zsh", "fish"] as const)("%s", (shell) => {
  it("adds the block to a file that exists, keeps a copy, and says so", () => {
    const h = home({ [rc(shell)]: "export A=1\nalias ll='ls -l'\n" });
    expect(sh(installScript(shell), h)).toBe("DONE");
    const after = readFileSync(join(h, rc(shell)), "utf8");
    expect(after.startsWith("export A=1\nalias ll='ls -l'\n")).toBe(true);
    expect(after).toContain(blockFor(shell));
    expect(readFileSync(join(h, rc(shell) + BACKUP_SUFFIX), "utf8")).toBe("export A=1\nalias ll='ls -l'\n");
  });

  it("creates the file (and its folder) when there is none", () => {
    const h = home();
    expect(sh(installScript(shell), h)).toBe("DONE");
    expect(readFileSync(join(h, rc(shell)), "utf8")).toContain(MARK_BEGIN);
  });

  it("adds it once: a second install changes nothing", () => {
    const h = home({ [rc(shell)]: "x\n" });
    sh(installScript(shell), h);
    const once = readFileSync(join(h, rc(shell)), "utf8");
    expect(sh(installScript(shell), h)).toBe("ALREADY");
    expect(readFileSync(join(h, rc(shell)), "utf8")).toBe(once);
    expect(once.split(MARK_BEGIN).length - 1).toBe(1);
  });

  it("does not glue the marker onto a last line with no newline", () => {
    const h = home({ [rc(shell)]: "export LAST=1" });
    sh(installScript(shell), h);
    const lines = readFileSync(join(h, rc(shell)), "utf8").split("\n");
    expect(lines[0]).toBe("export LAST=1");
    expect(lines).toContain(MARK_BEGIN);
  });

  it("takes exactly its block out again and leaves the rest as it was", () => {
    const original = "export A=1\n\n# my own stuff\nalias ll='ls -l'\n";
    const h = home({ [rc(shell)]: original });
    sh(installScript(shell), h);
    expect(sh(removeScript(shell), h)).toBe("GONE");
    const after = readFileSync(join(h, rc(shell)), "utf8");
    expect(after.replace(/\n+$/, "\n")).toBe(original);
    expect(after).not.toContain("SSHVault");
    expect(sh(removeScript(shell), h)).toBe("NOTHING");
    expect(existsSync(join(h, rc(shell) + ".sshvault.tmp"))).toBe(false);
  });

  it("removing from a file that has no block, or no file, is NOTHING and touches nothing", () => {
    const h = home({ [rc(shell)]: "keep\n" });
    expect(sh(removeScript(shell), h)).toBe("NOTHING");
    expect(readFileSync(join(h, rc(shell)), "utf8")).toBe("keep\n");
    expect(sh(removeScript(shell), home())).toBe("NOTHING");
  });
});

describe("detecting", () => {
  it("reports the login shell, its file, and whether the block is there", () => {
    const h = home({ ".bashrc": "x\n" });
    expect(parseDetect(sh(DETECT_SCRIPT, h, "/bin/bash"))).toEqual({ shell: "bash", name: "bash", file: join(h, ".bashrc"), installed: false });
    sh(installScript("bash"), h);
    expect(parseDetect(sh(DETECT_SCRIPT, h, "/bin/bash")).installed).toBe(true);
    expect(parseDetect(sh(DETECT_SCRIPT, h, "/usr/bin/zsh"))).toMatchObject({ shell: "zsh", installed: false });
    expect(parseDetect(sh(DETECT_SCRIPT, h, "/usr/bin/fish")).file).toBe(join(h, ".config/fish/config.fish"));
  });

  it("says plainly when the shell isn't one it can do", () => {
    const d = parseDetect(sh(DETECT_SCRIPT, home(), "/bin/tcsh"));
    expect(d.shell).toBeNull();
    expect(d.name).toBe("tcsh");
    expect(parseDetect("").shell).toBeNull();
  });

  it("a hostile home folder name is only a path", () => {
    const h = mkdtempSync(join(tmpdir(), "sv int $(touch pwned) '"));
    homes.push(h);
    expect(sh(installScript("bash"), h)).toBe("DONE");
    expect(existsSync(join(h, "pwned"))).toBe(false);
    expect(readFileSync(join(h, ".bashrc"), "utf8")).toContain(MARK_BEGIN);
  });
});
