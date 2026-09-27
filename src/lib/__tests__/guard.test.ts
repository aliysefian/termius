import { describe, expect, it } from "vitest";
import { LineTracker, firstDestructiveLine, matchDestructive, pasteNeedsConfirm, pastedLines } from "../guard";
import { DEFAULT_DESTRUCTIVE } from "./fixtures";

describe("paste protection", () => {
  it("counts the commands a paste would submit", () => {
    expect(pastedLines("")).toBe(0);
    expect(pastedLines("ls")).toBe(1);
    expect(pastedLines("ls\n")).toBe(1);
    expect(pastedLines("ls\r\npwd")).toBe(2);
    expect(pastedLines("a\nb\nc\n")).toBe(3);
  });

  it("asks for multi-line pastes, never for plain text", () => {
    expect(pasteNeedsConfirm("echo hi", 2)).toBe(false);
    expect(pasteNeedsConfirm("a\nb", 2)).toBe(true);
    expect(pasteNeedsConfirm("a\nb", 3)).toBe(false);
    expect(pasteNeedsConfirm("a\nb", 0)).toBe(false);
    // One line that would run immediately: only asked on production.
    expect(pasteNeedsConfirm("rm -rf /tmp/x\n", 2)).toBe(false);
    expect(pasteNeedsConfirm("rm -rf /tmp/x\n", 2, true)).toBe(true);
  });
});

describe("destructive command warnings", () => {
  it("matches the default patterns", () => {
    for (const cmd of ["rm -rf /var/lib/app", "sudo reboot", "DROP TABLE users;", "kubectl delete ns prod", "terraform destroy", "git push --force origin main", "mkfs.ext4 /dev/sdb1", "dd if=x of=/dev/sda"]) {
      expect(matchDestructive(cmd, DEFAULT_DESTRUCTIVE), cmd).not.toBeNull();
    }
    for (const cmd of ["ls -la", "rm file.txt", "git push origin main", "kubectl get pods", "echo reboot-notes"]) {
      expect(matchDestructive(cmd, DEFAULT_DESTRUCTIVE), cmd).toBeNull();
    }
  });

  it("skips invalid patterns instead of failing", () => {
    expect(matchDestructive("reboot", ["(", "\\breboot\\b"])).toBe("\\breboot\\b");
  });
});

describe("snippets sent as a whole", () => {
  it("finds the dangerous line in a multi-line snippet", () => {
    const hit = firstDestructiveLine("cd /srv/app\ngit pull\nsudo systemctl stop app\n", DEFAULT_DESTRUCTIVE);
    expect(hit?.line).toBe("sudo systemctl stop app");
    expect(firstDestructiveLine("uptime\ndf -h", DEFAULT_DESTRUCTIVE)).toBeNull();
  });
});

describe("LineTracker", () => {
  it("follows typing, backspace and clearing", () => {
    const t = new LineTracker();
    t.feed("rm -rf /x");
    t.feed("\x7f\x7fy");
    expect(t.line).toBe("rm -rf y");
    t.feed("\x15");
    expect(t.line).toBe("");
    t.feed("reboot");
    t.feed("\r");
    expect(t.line).toBe("");
  });

  it("gives up after history or completion", () => {
    const t = new LineTracker();
    t.feed("\x1b[A");
    expect(t.line).toBeNull();
    t.feed("\r");
    expect(t.line).toBe("");
    t.feed("sys\t");
    expect(t.line).toBeNull();
    t.feed("\x03");
    expect(t.line).toBe("");
  });
});
