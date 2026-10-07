import { describe, expect, it } from "vitest";
import { PRESETS, fingerprint, renderCommand } from "../commandhost";

const v = { host: "10.0.0.5", user: "deploy", id: "i-0abc" };

describe("a host that connects with a command", () => {
  it("puts the host's values in", () => {
    expect(renderCommand("aws ssm start-session --target {id}", v)).toEqual({ ok: true, command: "aws ssm start-session --target i-0abc" });
    expect(renderCommand("tsh ssh {user}@{host}", v)).toEqual({ ok: true, command: "tsh ssh deploy@10.0.0.5" });
    expect(renderCommand("echo {host} {host}", v)).toEqual({ ok: true, command: "echo 10.0.0.5 10.0.0.5" });
    expect(renderCommand("  ls  ", v)).toEqual({ ok: true, command: "ls" });
  });

  it("refuses values that could change what the command means", () => {
    for (const bad of ["a b", "a;b", "$(x)", "a`b`", "a|b", "a&b", "a'b", 'a"b', "a>b", "a\nb", "é", "a*b", "a\\b", "a%b", "a^b"]) {
      const r = renderCommand("ssh {host}", { ...v, host: bad });
      expect(r.ok, JSON.stringify(bad)).toBe(false);
    }
  });

  it("says what is missing or unknown", () => {
    expect(renderCommand("", v)).toMatchObject({ ok: false });
    expect(renderCommand("ssh {user}@{host}", { ...v, user: "" })).toMatchObject({ ok: false, error: expect.stringContaining("user name") });
    expect(renderCommand("echo {label}", v)).toMatchObject({ ok: false, error: expect.stringContaining("{label}") });
    expect(renderCommand("a\nb", v)).toMatchObject({ ok: false });
    expect(renderCommand("x".repeat(501), v)).toMatchObject({ ok: false });
  });

  it("a shell variable written like a placeholder is refused rather than guessed at", () => {
    expect(renderCommand("echo ${HOME}", v)).toMatchObject({ ok: false });
    expect(renderCommand("echo {{host}}", v)).toEqual({ ok: true, command: "echo {10.0.0.5}" });
  });

  it("a fingerprint changes with the command", () => {
    expect(fingerprint("ssh a")).toBe(fingerprint("ssh a"));
    expect(fingerprint("ssh a")).not.toBe(fingerprint("ssh b"));
    expect(fingerprint("")).toBeTruthy();
  });

  it("every preset renders for a host that has everything", () => {
    for (const p of PRESETS) expect(renderCommand(p.command, v).ok, p.label).toBe(true);
  });
});
