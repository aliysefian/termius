import { describe, expect, it } from "vitest";
import { parseOpenSshCertificate } from "../sshcert";

// Real certificates from `ssh-keygen -s`, so the parser is checked against
// an actual implementation of the format, not just hand-rolled bytes.
const ED25519_CERT =
  "ssh-ed25519-cert-v01@openssh.com AAAAIHNzaC1lZDI1NTE5LWNlcnQtdjAxQG9wZW5zc2guY29tAAAAIEn7dlVo1yBPZgArlYYFHxk+Qta6ri8HTxwJvyiLMppqAAAAIBMyWZ5935vrECQ8ycRohht8pkpHPGqhTcXBdOydus0RAAAAAAAAAAAAAAABAAAACHRlc3R1c2VyAAAADAAAAAh0ZXN0dXNlcgAAAABeC+EAAAAAAHG1YAAAAAAAAAAAggAAABVwZXJtaXQtWDExLWZvcndhcmRpbmcAAAAAAAAAF3Blcm1pdC1hZ2VudC1mb3J3YXJkaW5nAAAAAAAAABZwZXJtaXQtcG9ydC1mb3J3YXJkaW5nAAAAAAAAAApwZXJtaXQtcHR5AAAAAAAAAA5wZXJtaXQtdXNlci1yYwAAAAAAAAAAAAAAMwAAAAtzc2gtZWQyNTUxOQAAACAdTsn4jmSLdd+HwNgQLQtpua2qXg8VD7BpiJ3C4XzvcAAAAFMAAAALc3NoLWVkMjU1MTkAAABA16/U2KEe5up6lnaO7/q57s4sx8LW1EnNY/w0PEKGODVO8mhM7fb2XXGZ8JYM/xIcEO7JgbJtdnTw57b0ijnFBg==";
const RSA_CERT =
  "ssh-rsa-cert-v01@openssh.com AAAAHHNzaC1yc2EtY2VydC12MDFAb3BlbnNzaC5jb20AAAAgXX7Z23nv+kLU4yd+I2P8h68MSxdDIyC+8YI0OK/Np2EAAAADAQABAAABAQDIntbbEOVvtII2lYrr3BZ+Wv6QJicj6I5ybKIt36eE9ELQ1owx+BswqLcW8NYpx2/UZ0Xgt5OmFxAubTtYkXvEjux/OYJeqJLwIPc/ZlEvhTDLiRiHKy6BxKo7Ff/epIPF7OlGgV4xdP7/mkTSgRCmWKHD1m9Jw1uIUvJ62JkOm0TZ549pC+inWgWzz2s2kIatssKRQTdCS3Nklhab0Doo+RAnuATPBcjEfR77NR1CGK0FPQVQV4iuKxD9Y1K+n/VMPAPqHvwMOYWrnxhvroC2Eev9M5mfmddL5bPzYWaZNaITf0RNnEOWiuQjjV0SHtMXO43MdrttE9de+A6/LA7dAAAAAAAAAAAAAAABAAAAB3Rlc3Ryc2EAAAALAAAAB3Rlc3Ryc2EAAAAAYDwugAAAAAD0hQWAAAAAAAAAAIIAAAAVcGVybWl0LVgxMS1mb3J3YXJkaW5nAAAAAAAAABdwZXJtaXQtYWdlbnQtZm9yd2FyZGluZwAAAAAAAAAWcGVybWl0LXBvcnQtZm9yd2FyZGluZwAAAAAAAAAKcGVybWl0LXB0eQAAAAAAAAAOcGVybWl0LXVzZXItcmMAAAAAAAAAAAAAADMAAAALc3NoLWVkMjU1MTkAAAAgHU7J+I5ki3Xfh8DYEC0Labmtql4PFQ+waYidwuF873AAAABTAAAAC3NzaC1lZDI1NTE5AAAAQMGTyDZoZPkSYX2d8EuRzcZsL3KLmeHqgJpz0Jh/k0IwHfcM75L7F+QK0OX10leUwu6IR1a8UJwTJUZa67WBPgg=";
const ECDSA_CERT_NO_EXPIRY =
  "ecdsa-sha2-nistp256-cert-v01@openssh.com AAAAKGVjZHNhLXNoYTItbmlzdHAyNTYtY2VydC12MDFAb3BlbnNzaC5jb20AAAAgroaX5w4NrArlYtA1I1Y+RfqRs3XqRJwZ+OPZJgRJrvwAAAAIbmlzdHAyNTYAAABBBNQHxhw9h/KCCyYUjZGXm9Q36Zajl3nvOkBQkHo/IP5UX2DUyjysXc76VAEaAN5xUowoe+ibqYRZ1vMaS46idvkAAAAAAAAAAAAAAAEAAAAGdGVzdGVjAAAACgAAAAZ0ZXN0ZWMAAAAAAAAAAP//////////AAAAAAAAAIIAAAAVcGVybWl0LVgxMS1mb3J3YXJkaW5nAAAAAAAAABdwZXJtaXQtYWdlbnQtZm9yd2FyZGluZwAAAAAAAAAWcGVybWl0LXBvcnQtZm9yd2FyZGluZwAAAAAAAAAKcGVybWl0LXB0eQAAAAAAAAAOcGVybWl0LXVzZXItcmMAAAAAAAAAAAAAADMAAAALc3NoLWVkMjU1MTkAAAAgHU7J+I5ki3Xfh8DYEC0Labmtql4PFQ+waYidwuF873AAAABTAAAAC3NzaC1lZDI1NTE5AAAAQAremL50GffqmB76b1k5xJBTrSoSDB3InyfbMvdU1z2g/IkWPsA5qpBukvMScmR+N28njUVQoPB0uaUCB+ew0QU=";

describe("parseOpenSshCertificate", () => {
  it("reads an ed25519 certificate's validity window and key id", () => {
    const c = parseOpenSshCertificate(ED25519_CERT);
    expect(c?.type).toBe("ssh-ed25519-cert-v01@openssh.com");
    expect(c?.keyId).toBe("testuser");
    expect(c?.serial).toBe(0n);
    expect(c?.validAfter).toBe(1577836800); // 2020-01-01T00:00:00Z
    expect(c?.validBefore).toBe(1907712000); // 2030-06-15T00:00:00Z
  });

  it("reads an RSA certificate, which has two key-specific fields (e, n)", () => {
    const c = parseOpenSshCertificate(RSA_CERT);
    expect(c?.type).toBe("ssh-rsa-cert-v01@openssh.com");
    expect(c?.keyId).toBe("testrsa");
    expect(c?.validAfter).toBe(1614556800); // 2021-03-01T00:00:00Z
    expect(c?.validBefore).toBe(4102358400); // 2099-12-31T00:00:00Z
  });

  it("treats OpenSSH's all-ones sentinel as no expiry", () => {
    const c = parseOpenSshCertificate(ECDSA_CERT_NO_EXPIRY);
    expect(c?.type).toBe("ecdsa-sha2-nistp256-cert-v01@openssh.com");
    expect(c?.validBefore).toBeNull();
  });

  it("works whether or not a trailing comment is present", () => {
    expect(parseOpenSshCertificate(`${ED25519_CERT} someone@example.com`)?.keyId).toBe("testuser");
  });

  it("returns null for a plain (non-certificate) public key or garbage", () => {
    expect(parseOpenSshCertificate("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB")).toBeNull();
    expect(parseOpenSshCertificate("not a key at all")).toBeNull();
    expect(parseOpenSshCertificate("")).toBeNull();
  });
});
