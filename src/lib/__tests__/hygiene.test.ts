import { describe, expect, it } from "vitest";
import { canUseSshKey, checkKeyHygiene } from "../hygiene";
import type { Host, SshKey, VaultRecord } from "../types";

const RSA_2048 =
  "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDZSLZ9wKlucuhr9orlEmY7912YyVnqdYF/T7ztyqrbaqdiJUe5frcpWvGrjcW+8B/PuIX4e0yCso5MhEiycB40okeTfEjDYJ0R6g4JOfbCLw0N5hHzt5Cxs7+VqDYWoJ/QlA435AeFuscx3PGFLnu1xj+MurjvW8GSJBaADW3Ox7i17oZGDin+xuI/XvsqioJHnrX5q5y74vWR8qtdHW6OV4kZNkmlujUuxdUz7S0suG2UKfEkd01vGn+7se9hgNo+UTrrERCKezq4VvyS5SBpbs0HwpCKaV2x9agYvM0fdS79RJTKl6T7FQt9QQp3YdMLDOQ4k3HcZB6VsWoHZ1FF";
const ED25519 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILXOK7rRzcT/vn19p0qVEr6Ywh99TJopl4O7p8E4IgGq";
const DSA_1024 =
  "ssh-dss AAAAB3NzaC1kc3MAAACBAOkWHKDK7nbHW7O+Z9srt7cYhvs2lScZQLuMlNNMrfZal+QaVPgYwfj+26tFRL7Yq9frarGP5fALFQq7edKpS2laEebjRB8DPd1i+wrQnyg9sHkP6vFDVWpjXnrg24Oaz+mHzVFjAMBz6Z5FPvgZ6efpZMEM5+Zak8vlsbcb+J7FAAAAFQCenSD567thNq7iB9yAZclutS3SYQAAAIB/ogiKA//FjtjF7TUTVhdcWNntc2s/Hi2kJx2NTW/a2COTPSrWs29U5K8SxgkWvtjwH5zZBaDnGYo+dIEdALOjNuVmv7aHCfssjzllgoWoxh6CabN7Q8gm3cNqca4VRkkpTrz9VbQ+ljsqq1JEA/3l+2GX7V0EDHXGEqwqFwnsQAAAAIAOYCF9Wy0ezbnwb3KDJhEOaUR7qLbsB7FSw7zr49uvPDt6AuXQ/KXq/BXbnDYYegjaITikkNIg5CV03vgDE9vKkXAkG4EYLadNGyxyl+za2yiLhSU1upYSSxeVuO/1lKnTJTYkploD4EczIXXRfDB4btrtDuXu3acAQ9gzf/rq/w==";
// Real certificate (ssh-keygen -s), valid 2020-01-01..2030-06-15 UTC.
const ED25519_CERT =
  "ssh-ed25519-cert-v01@openssh.com AAAAIHNzaC1lZDI1NTE5LWNlcnQtdjAxQG9wZW5zc2guY29tAAAAIEn7dlVo1yBPZgArlYYFHxk+Qta6ri8HTxwJvyiLMppqAAAAIBMyWZ5935vrECQ8ycRohht8pkpHPGqhTcXBdOydus0RAAAAAAAAAAAAAAABAAAACHRlc3R1c2VyAAAADAAAAAh0ZXN0dXNlcgAAAABeC+EAAAAAAHG1YAAAAAAAAAAAggAAABVwZXJtaXQtWDExLWZvcndhcmRpbmcAAAAAAAAAF3Blcm1pdC1hZ2VudC1mb3J3YXJkaW5nAAAAAAAAABZwZXJtaXQtcG9ydC1mb3J3YXJkaW5nAAAAAAAAAApwZXJtaXQtcHR5AAAAAAAAAA5wZXJtaXQtdXNlci1yYwAAAAAAAAAAAAAAMwAAAAtzc2gtZWQyNTUxOQAAACAdTsn4jmSLdd+HwNgQLQtpua2qXg8VD7BpiJ3C4XzvcAAAAFMAAAALc3NoLWVkMjU1MTkAAABA16/U2KEe5up6lnaO7/q57s4sx8LW1EnNY/w0PEKGODVO8mhM7fb2XXGZ8JYM/xIcEO7JgbJtdnTw57b0ijnFBg==";

function key(name: string, public_key: string, overrides: Partial<SshKey> = {}): VaultRecord<SshKey> {
  return {
    id: name,
    rev: 1,
    updated_at: 0,
    deleted: false,
    device_id: "test",
    data: { name, algorithm: "", public_key, fingerprint: "", comment: "", encrypted: false, created_at: Date.now(), ...overrides },
  };
}

describe("checkKeyHygiene", () => {
  it("flags an RSA key under 3072 bits", () => {
    const f = checkKeyHygiene([key("legacy", RSA_2048)]);
    expect(f.map((x) => x.id)).toEqual(["legacy:weak-rsa"]);
    expect(f[0].severity).toBe("warning");
  });

  it("flags a DSA key regardless of size", () => {
    const f = checkKeyHygiene([key("old-dsa", DSA_1024)]);
    expect(f.map((x) => x.id)).toEqual(["old-dsa:dsa"]);
  });

  it("doesn't flag ed25519", () => {
    expect(checkKeyHygiene([key("fine", ED25519)])).toEqual([]);
  });

  it("flags a key older than the age threshold", () => {
    const sixYearsAgo = Date.now() - 6 * 365 * 24 * 60 * 60 * 1000;
    const f = checkKeyHygiene([key("ancient", ED25519, { created_at: sixYearsAgo })]);
    expect(f.map((x) => x.id)).toEqual(["ancient:age"]);
  });

  it("doesn't flag a recent key for age", () => {
    expect(checkKeyHygiene([key("new", ED25519, { created_at: Date.now() })])).toEqual([]);
  });

  it("flags an already-expired certificate as danger", () => {
    // "Now" is well after the cert's 2030-06-15 valid_before.
    const now = new Date("2031-01-01T00:00:00Z").getTime();
    const f = checkKeyHygiene([key("expired", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    const hit = f.find((x) => x.id === "expired:cert-expired");
    expect(hit?.severity).toBe("danger");
    expect(hit?.message).toContain("expired");
  });

  it("counts the days since expiry without adding one", () => {
    // valid_before is 2030-06-15T00:00:00Z; three and a half days later it expired 3 days ago, not 4.
    const now = new Date("2030-06-18T12:00:00Z").getTime();
    const f = checkKeyHygiene([key("late", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    expect(f.find((x) => x.id === "late:cert-expired")?.message).toBe("late: certificate expired 3 days ago.");
  });

  it("says a certificate that expired hours ago expired today", () => {
    const now = new Date("2030-06-15T02:00:00Z").getTime();
    const f = checkKeyHygiene([key("fresh", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    expect(f.find((x) => x.id === "fresh:cert-expired")?.message).toBe("fresh: certificate expired today.");
  });

  it("says a certificate expiring within hours expires today", () => {
    const now = new Date("2030-06-14T20:00:00Z").getTime();
    const f = checkKeyHygiene([key("eod", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    expect(f.find((x) => x.id === "eod:cert-expiring")?.message).toBe("eod: certificate expires today.");
  });

  it("flags a certificate expiring within the warning window", () => {
    // 10 days before 2030-06-15T00:00:00Z.
    const now = new Date("2030-06-05T00:00:00Z").getTime();
    const f = checkKeyHygiene([key("soon", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    const hit = f.find((x) => x.id === "soon:cert-expiring");
    expect(hit?.severity).toBe("warning");
    expect(hit?.message).toContain("10 day");
  });

  it("doesn't flag a certificate with plenty of time left", () => {
    const now = new Date("2025-01-01T00:00:00Z").getTime();
    const f = checkKeyHygiene([key("ok", ED25519, { certificate: ED25519_CERT, created_at: now })], now);
    expect(f.some((x) => x.id.includes("cert"))).toBe(false);
  });
});

describe("which hosts could log in with a key", () => {
  const host = (protocol?: string, extra: Partial<Host> = {}): Host => ({ label: "h", hostname: "h", port: 22, group: "", tags: [], notes: "", protocol, ...extra });

  it("SSH hosts can (with or without Mosh)", () => {
    expect(canUseSshKey(host())).toBe(true);
    expect(canUseSshKey(host(""))).toBe(true);
    expect(canUseSshKey(host(undefined, { mosh: true }))).toBe(true);
  });

  it("VNC through SSH can, plain VNC can't", () => {
    expect(canUseSshKey(host("vnc", { vnc: { ssh_tunnel: true, ssh_port: 22 } }))).toBe(true);
    expect(canUseSshKey(host("vnc", { vnc: { ssh_tunnel: false, ssh_port: 22 } }))).toBe(false);
    expect(canUseSshKey(host("vnc"))).toBe(true);
  });

  it("Remote Desktop, FTP, Telnet and command hosts can't", () => {
    for (const p of ["rdp", "ftp", "telnet", "command"]) expect(canUseSshKey(host(p))).toBe(false);
  });
});
