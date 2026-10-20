// Addresses, as far as the security views need them: telling an address from anything else before it goes into a
// command, and whether a source (an address or a subnet) covers an address.

const V4 = /^(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)$/;
// Loose on purpose (groups of hex separated by colons, one "::" at most); the host's tools have the last word.
const V6 = /^(?=.*:)(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}$/i;

export const isIPv4 = (s: string) => V4.test(s);
export const isIPv6 = (s: string) => V6.test(s) && (s.match(/::/g)?.length ?? 0) <= 1;
export const isIP = (s: string) => isIPv4(s) || isIPv6(s);

/** `10.0.0.0/8` or a bare address. Anything else is not allowed into a firewall command. */
export function isSource(s: string): boolean {
  const [addr, bits, extra] = s.split("/");
  if (extra !== undefined || !isIP(addr)) return false;
  if (bits === undefined) return true;
  const n = Number(bits);
  return /^\d{1,3}$/.test(bits) && n >= 0 && n <= (isIPv4(addr) ? 32 : 128);
}

function v4num(s: string): number {
  return s.split(".").reduce((n, o) => n * 256 + Number(o), 0);
}

/** Whether `source` ("Anywhere", an address, or a subnet) covers `ip`. IPv6 is compared as text, whole addresses only. */
export function sourceCovers(source: string, ip: string): boolean {
  const s = source.trim().replace(/\s*\(v6\)$/, "");
  if (/^(anywhere|any|0\.0\.0\.0\/0|::\/0|all)$/i.test(s)) return true;
  if (!isSource(s)) return false;
  const [addr, bits] = s.split("/");
  if (isIPv4(addr) && isIPv4(ip)) {
    const n = bits === undefined ? 32 : Number(bits);
    if (n === 0) return true;
    const mask = n === 32 ? 0xffffffff : (~((1 << (32 - n)) - 1)) >>> 0;
    return ((v4num(addr) & mask) >>> 0) === ((v4num(ip) & mask) >>> 0);
  }
  return isIPv6(addr) && isIPv6(ip) && bits === undefined && addr.toLowerCase() === ip.toLowerCase();
}

/** `22`, `80,443`, `6000:6007`, `1000-2000`, or a list of those: does it include `port`? */
export function portsInclude(ports: string, port: number): boolean {
  if (!ports || /^(any|anywhere)$/i.test(ports)) return true;
  return ports.split(",").some((part) => {
    const m = /^(\d+)(?:[:-](\d+))?$/.exec(part.trim());
    if (!m) return false;
    const lo = Number(m[1]);
    const hi = m[2] === undefined ? lo : Number(m[2]);
    return port >= lo && port <= hi;
  });
}
