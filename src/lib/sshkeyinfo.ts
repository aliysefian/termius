// Reads an SSH public key blob (the standard "algorithm base64 [comment]"
// line) just far enough to get its algorithm and, for RSA and DSA, its
// modulus size — the inputs the hygiene checks need. No crypto, no
// verification; purely the wire format (RFC 4253 §6.6 and friends).

function mpintBits(field: Uint8Array): number {
  // SSH mpints are two's-complement big-endian with a leading 0x00 added
  // whenever the high bit of the first real byte would otherwise be set,
  // so stripping one leading zero byte (if present) gives the true size.
  const len = field.length > 0 && field[0] === 0 ? field.length - 1 : field.length;
  return len * 8;
}

export interface SshPublicKeyInfo {
  /** The wire algorithm name, e.g. "ssh-rsa", "ssh-ed25519", "ecdsa-sha2-nistp256". */
  algorithm: string;
  /** Modulus size for RSA/DSA; null for key types where "bits" isn't the relevant measure. */
  bits: number | null;
}

export function parseSshPublicKey(line: string): SshPublicKeyInfo | null {
  const parts = line.trim().split(/\s+/);
  const token = parts.length > 1 ? parts[1] : parts[0];
  if (!token) return null;
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(atob(token), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
  let pos = 0;
  const nextField = (): Uint8Array => {
    const len = new DataView(bytes.buffer, bytes.byteOffset + pos, 4).getUint32(0, false);
    pos += 4;
    const v = bytes.subarray(pos, pos + len);
    pos += len;
    return v;
  };
  try {
    const algorithm = new TextDecoder().decode(nextField());
    if (algorithm === "ssh-rsa") {
      nextField(); // e
      const n = nextField();
      return { algorithm, bits: mpintBits(n) };
    }
    if (algorithm === "ssh-dss") {
      const p = nextField();
      return { algorithm, bits: mpintBits(p) };
    }
    return { algorithm, bits: null };
  } catch {
    return null;
  }
}
