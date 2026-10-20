import { describe, expect, it } from "vitest";
import { b64, checkPublicKeyLine, fingerprintOf, isHardwareKey, keyRisk, parseAuthorizedKeys, withKey, withoutKey, writeScript } from "$lib/security/authkeys";

// Both keys and their fingerprints are from `ssh-keygen -lf`.
const ED = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHWekNpohD2bsBKRjv8wSSlq3U9WlT/ZtKaYHSPB4MxL";
const ED_FP = "SHA256:Elv2wN3GK+ts+kh1d0t2D0adL67Cvn9YTOadKb/xLk8";
const RSA =
  "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC2TXeXfbQO6TSoRVb/QpAV+A3mMdZMHE7YKhLPJ8D9tP0vIaF+1cxBHE53A4ciFumbfoOObV6x0h7hfOeqwBk2rfAQcIRQ6Z699+h97spZeZPGbSi6ubDUJt1o/HmIkPoUFkB7mfXLQsGOYKchCyvEOpRn97piaPKffTsDCuKy4zjXsit1IMd4SzNLFNTyJ8/l19Kuo5wZ4xfA6U9JA2N/qL3Z2JrDdX3wuGUPqTg24ai+kLpVR46zM7DRBBkTTL27txx63w/frdFr7QT890/mJwP9MuHZGByVV340GN2v5p8giL1pjuL7aHdAHnZnJh/szSJtVHZmiTMGdFbPvZzn";
const RSA_FP = "SHA256:1DwLDWr9ndRsFLHhYwsqWUH0k+vV9u32zh7vN2n+N3Q";

const FILE = `# deploy keys
${ED} ali@workstation
from="10.0.0.0/8,192.168.*",no-agent-forwarding,no-port-forwarding ${RSA} deploy-key

command="/usr/bin/backup --now, quickly",restrict ${ED} backup job
not a key at all
`;

describe("authorized_keys", () => {
  it("computes the fingerprint ssh-keygen shows", async () => {
    expect(await fingerprintOf(ED.split(" ")[1])).toBe(ED_FP);
    expect(await fingerprintOf(RSA.split(" ")[1])).toBe(RSA_FP);
  });

  it("reads keys, comments and options, skipping comments and junk", async () => {
    const k = await parseAuthorizedKeys(FILE);
    expect(k.map((x) => [x.line, x.type, x.comment, x.options])).toEqual([
      [1, "ssh-ed25519", "ali@workstation", []],
      [2, "ssh-rsa", "deploy-key", ['from="10.0.0.0/8,192.168.*"', "no-agent-forwarding", "no-port-forwarding"]],
      [4, "ssh-ed25519", "backup job", ['command="/usr/bin/backup --now, quickly"', "restrict"]],
    ]);
    expect(k[0].fingerprint).toBe(ED_FP);
    expect(k[1]).toMatchObject({ fingerprint: RSA_FP, bits: 2048 });
    expect(k[0].raw).toBe(`${ED} ali@workstation`);
  });

  it("flags a short RSA key, not an ed25519 one", async () => {
    const [ed, rsa] = await parseAuthorizedKeys(FILE);
    expect(keyRisk(ed)).toBe("ok");
    expect(keyRisk(rsa)).toBe("weak");
    expect(keyRisk({ type: "ssh-dss", bits: 1024 })).toBe("legacy");
    expect(keyRisk({ type: "ssh-rsa", bits: 4096 })).toBe("ok");
    expect(isHardwareKey("sk-ssh-ed25519@openssh.com")).toBe(true);
    expect(isHardwareKey("ssh-ed25519")).toBe(false);
  });

  it("recognises hardware keys", async () => {
    const [k] = await parseAuthorizedKeys("sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29tAAAAIHWekNpohD2bsBKRjv8wSSlq3U9WlT/ZtKaYHSPB4MxLAAAABHNzaDo= yubikey");
    expect(k).toMatchObject({ type: "sk-ssh-ed25519@openssh.com", comment: "yubikey" });
  });

  it("checks a public key before it goes into the file", () => {
    expect(checkPublicKeyLine(ED)).toBeNull();
    expect(checkPublicKeyLine(`${ED} a comment`)).toBeNull();
    expect(checkPublicKeyLine("ssh-ed25519 AAAA\nssh-rsa BBBB")).not.toBeNull();
    expect(checkPublicKeyLine("hello world")).not.toBeNull();
    expect(checkPublicKeyLine('command="x" ' + ED)).not.toBeNull();
  });

  it("adds a key once and removes exactly one line", () => {
    const text = `${ED} one\n${RSA} two\n`;
    expect(withKey(text, `${ED} one`)).toBe(text);
    expect(withKey("", ED)).toBe(`${ED}\n`);
    expect(withKey("no newline", ED)).toBe(`no newline\n${ED}\n`);
    expect(withoutKey(text, { line: 0, raw: `${ED} one` })).toBe(`${RSA} two\n`);
    expect(() => withoutKey(text, { line: 1, raw: "something else" })).toThrow(/changed/);
  });

  it("writes the file safely: modes, a backup, a stale-file check, and no key text in the clear", () => {
    const s = writeScript("me", `${ED} x\n`, "old\n");
    expect(s).toContain("chmod 700");
    expect(s).toContain("chmod 600");
    expect(s).toContain('mv "$f.sshvault-new" "$f"');
    expect(s).toContain("sshvault-bak");
    expect(s).toContain(b64("old\n"));
    expect(s).toContain("exit 3");
    expect(s).not.toContain("ssh-ed25519");
    expect(writeScript("root", "", "")).toContain("/root/.ssh/authorized_keys");
  });
});
