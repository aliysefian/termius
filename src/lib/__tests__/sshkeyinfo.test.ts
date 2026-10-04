import { describe, expect, it } from "vitest";
import { parseSshPublicKey } from "../sshkeyinfo";

// Real keys from `ssh-keygen`, so the mpint-size math is checked against
// actual key material, not hand-rolled bytes.
const RSA_2048 =
  "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDZSLZ9wKlucuhr9orlEmY7912YyVnqdYF/T7ztyqrbaqdiJUe5frcpWvGrjcW+8B/PuIX4e0yCso5MhEiycB40okeTfEjDYJ0R6g4JOfbCLw0N5hHzt5Cxs7+VqDYWoJ/QlA435AeFuscx3PGFLnu1xj+MurjvW8GSJBaADW3Ox7i17oZGDin+xuI/XvsqioJHnrX5q5y74vWR8qtdHW6OV4kZNkmlujUuxdUz7S0suG2UKfEkd01vGn+7se9hgNo+UTrrERCKezq4VvyS5SBpbs0HwpCKaV2x9agYvM0fdS79RJTKl6T7FQt9QQp3YdMLDOQ4k3HcZB6VsWoHZ1FF";
const RSA_4096 =
  "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAACAQDsQzX9hOepK48mKtJnaNgL00t8MvN2YaywBud9z4zukWXYzDmumDj7d9c/94Y+1AQbg6zYNgRcHzdTE3tdMTr5IL1zsPmmmXYFOWqKo5Rz8H44EMl7yuO11iGJrrIww9W+RYFsL8cuajvyIxUinHxnKjf07si5D6YURv7iXDuDuFw7GpJet2wooH5z4N/Zvx+25kdVlMG/FiKSA0MVaF+vKzMLrW1yJeWz/YRSBA4EgZxoLMdpcj8Mcp8mD4loH3ewgRfQwCcrZBsNJdqXmqVZKrKAI3hcXhnQRP+eqlIdSkBY/o9cAqFDXJX5/0zsliiTooy8DKKsk1P3dCPjXC5YbFV3eJmKvP6lp3kJ0+oOCyvWnGPGI4dc3h/a6mi9TrCdp32BNpBzAwsJ54pKVqk0/IqJOfy1iNOAQza/CXRtDj+3nADoRex2mVKiBJOJ8KYMBARnfaPjRh1/TmtdElA9zAUBbpilFwSvRFcI90BpewTahrRbElhB1DW9HhyOvP12UYuYA43j/nsfwKL3oQCsezyAkpG0VeMkx/sijP+2r4DROMsY4Zu9lY9F7I7smxikDfdBRW4tvCWjtRT9CJ+1nSDFuoC6IntnXav7i1tMNlvTrrv5ocTLjVOgqcP66uHdej4CXwFrrhkKaj1jTONqAUzyS8jAv/pTjXduYyQ+XQ==";
const ED25519 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILXOK7rRzcT/vn19p0qVEr6Ywh99TJopl4O7p8E4IgGq";
const ECDSA_256 =
  "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBNrVSc1+ljyKgmHd3kDvRKUM0vOAmB41Q2T5akWR9oqvZKm8QCbKYDpiTrteQLWq13xC08lOZgzUF0SPQbaznSQ=";
const DSA_1024 =
  "ssh-dss AAAAB3NzaC1kc3MAAACBAOkWHKDK7nbHW7O+Z9srt7cYhvs2lScZQLuMlNNMrfZal+QaVPgYwfj+26tFRL7Yq9frarGP5fALFQq7edKpS2laEebjRB8DPd1i+wrQnyg9sHkP6vFDVWpjXnrg24Oaz+mHzVFjAMBz6Z5FPvgZ6efpZMEM5+Zak8vlsbcb+J7FAAAAFQCenSD567thNq7iB9yAZclutS3SYQAAAIB/ogiKA//FjtjF7TUTVhdcWNntc2s/Hi2kJx2NTW/a2COTPSrWs29U5K8SxgkWvtjwH5zZBaDnGYo+dIEdALOjNuVmv7aHCfssjzllgoWoxh6CabN7Q8gm3cNqca4VRkkpTrz9VbQ+ljsqq1JEA/3l+2GX7V0EDHXGEqwqFwnsQAAAAIAOYCF9Wy0ezbnwb3KDJhEOaUR7qLbsB7FSw7zr49uvPDt6AuXQ/KXq/BXbnDYYegjaITikkNIg5CV03vgDE9vKkXAkG4EYLadNGyxyl+za2yiLhSU1upYSSxeVuO/1lKnTJTYkploD4EczIXXRfDB4btrtDuXu3acAQ9gzf/rq/w==";

describe("parseSshPublicKey", () => {
  it("reads an RSA key's exact modulus size", () => {
    expect(parseSshPublicKey(RSA_2048)).toEqual({ algorithm: "ssh-rsa", bits: 2048 });
    expect(parseSshPublicKey(RSA_4096)).toEqual({ algorithm: "ssh-rsa", bits: 4096 });
  });

  it("reads a DSA key's modulus size", () => {
    expect(parseSshPublicKey(DSA_1024)).toEqual({ algorithm: "ssh-dss", bits: 1024 });
  });

  it("has no meaningful bit count for ed25519 or ECDSA", () => {
    expect(parseSshPublicKey(ED25519)).toEqual({ algorithm: "ssh-ed25519", bits: null });
    expect(parseSshPublicKey(ECDSA_256)).toEqual({ algorithm: "ecdsa-sha2-nistp256", bits: null });
  });

  it("works whether or not a trailing comment is present", () => {
    expect(parseSshPublicKey(`${RSA_2048} user@host`)?.bits).toBe(2048);
  });

  it("returns null for garbage", () => {
    expect(parseSshPublicKey("not a key")).toBeNull();
    expect(parseSshPublicKey("")).toBeNull();
  });
});
