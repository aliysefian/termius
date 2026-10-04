// Minimal reader for OpenSSH certificates (PROTOCOL.certkeys), just enough
// to pull out the fields a hygiene check needs: validity window, serial and
// key id. Parses the wire format directly rather than depending on any
// native crypto; nothing here signs or verifies anything.

class Reader {
  #bytes: Uint8Array;
  #pos = 0;
  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
  }
  get done(): boolean {
    return this.#pos >= this.#bytes.length;
  }
  uint32(): number {
    const v = new DataView(this.#bytes.buffer, this.#bytes.byteOffset + this.#pos, 4).getUint32(0, false);
    this.#pos += 4;
    return v;
  }
  uint64(): bigint {
    const v = new DataView(this.#bytes.buffer, this.#bytes.byteOffset + this.#pos, 8).getBigUint64(0, false);
    this.#pos += 8;
    return v;
  }
  bytes(): Uint8Array {
    const len = this.uint32();
    const v = this.#bytes.subarray(this.#pos, this.#pos + len);
    this.#pos += len;
    return v;
  }
  string(): string {
    return new TextDecoder().decode(this.bytes());
  }
  /** Skips `n` length-prefixed fields without decoding them. */
  skip(n: number) {
    for (let i = 0; i < n; i++) this.bytes();
  }
}

/** Fields preceding the common `serial` field, by certificate key type. */
function keySpecificFields(certType: string): number | null {
  if (certType.startsWith("ssh-ed25519-cert-")) return 1; // pk
  if (certType.startsWith("ecdsa-sha2-") && certType.includes("-cert-")) return 2; // curve, public_key
  if (certType.startsWith("ssh-rsa-cert-")) return 2; // e, n
  if (certType.startsWith("ssh-dss-cert-")) return 4; // p, q, g, y
  return null;
}

export interface ParsedCertificate {
  type: string;
  serial: bigint;
  keyId: string;
  /** Seconds since epoch, or null for "forever" (OpenSSH's all-ones sentinel, or out of JS's safe integer range). */
  validAfter: number | null;
  validBefore: number | null;
}

const NO_EXPIRY = (1n << 64n) - 1n;

function toSeconds(v: bigint): number | null {
  if (v === NO_EXPIRY || v === 0n) return null;
  return v > BigInt(Number.MAX_SAFE_INTEGER) ? null : Number(v);
}

/**
 * Parses one `authorized_keys`-style certificate line ("type base64
 * [comment]"), or a bare base64 blob. Returns null for anything that isn't
 * a recognised OpenSSH certificate.
 */
export function parseOpenSshCertificate(line: string): ParsedCertificate | null {
  const token = line.trim().split(/\s+/)[1] ?? line.trim().split(/\s+/)[0];
  if (!token) return null;
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(atob(token), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
  try {
    const r = new Reader(bytes);
    const type = r.string();
    const fieldCount = keySpecificFields(type);
    if (fieldCount === null) return null;
    r.bytes(); // nonce
    r.skip(fieldCount);
    const serial = r.uint64();
    r.uint32(); // cert type (user/host); not needed here
    const keyId = r.string();
    r.bytes(); // valid principals
    const validAfter = r.uint64();
    const validBefore = r.uint64();
    return { type, serial, keyId, validAfter: toSeconds(validAfter), validBefore: toSeconds(validBefore) };
  } catch {
    return null;
  }
}
