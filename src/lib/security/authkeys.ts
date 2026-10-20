// `authorized_keys`, line by line: which keys let someone in, with what restrictions, and the edits that add or
// remove one. The file is rewritten whole (new file, mode 600, moved over the old), never patched in place.
import { shq } from "$lib/ops/quote";
import { parseSshPublicKey } from "$lib/sshkeyinfo";

export interface AuthorizedKey {
  /** Position among the file's lines (0-based), to say which one. */
  line: number;
  /** The line exactly as in the file. */
  raw: string;
  /** `from="10.0.0.0/8"`, `no-agent-forwarding`, `command="…"`. */
  options: string[];
  type: string;
  bits: number | null;
  blob: string;
  comment: string;
  /** `SHA256:…`, as `ssh-keygen -lf` prints it. */
  fingerprint: string;
}

const KEY_TYPE = /^(?:ssh-(?:ed25519|rsa|dss)|ecdsa-sha2-nistp(?:256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)(?:-cert-v01@openssh\.com)?$/;

/** Split off the options: a first word that isn't a key type, up to the first space outside quotes. */
function splitOptions(line: string): { options: string[]; rest: string } | null {
  const first = line.split(/\s+/, 1)[0];
  if (KEY_TYPE.test(first)) return { options: [], rest: line };
  let quoted = false;
  let end = -1;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === "\\" && quoted) i++;
    else if (c === '"') quoted = !quoted;
    else if (/\s/.test(c) && !quoted) {
      end = i;
      break;
    }
  }
  if (end < 0 || quoted) return null;
  const options: string[] = [];
  let cur = "";
  quoted = false;
  for (let i = 0; i < end; i++) {
    const c = line[i];
    if (c === "\\" && quoted) {
      cur += c + line[++i];
      continue;
    }
    if (c === '"') quoted = !quoted;
    if (c === "," && !quoted) {
      options.push(cur);
      cur = "";
    } else cur += c;
  }
  if (cur) options.push(cur);
  return { options, rest: line.slice(end).trim() };
}

/** SHA-256 of the key blob, base64 without padding: what `ssh-keygen -lf` shows after `SHA256:`. */
export async function fingerprintOf(blob: string): Promise<string> {
  const bytes = Uint8Array.from(atob(blob), (c) => c.charCodeAt(0));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  let bin = "";
  for (const b of digest) bin += String.fromCharCode(b);
  return "SHA256:" + btoa(bin).replace(/=+$/, "");
}

export async function parseAuthorizedKeys(text: string): Promise<AuthorizedKey[]> {
  const out: AuthorizedKey[] = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const s = splitOptions(line);
    if (!s) continue;
    const m = /^(\S+)\s+(\S+)(?:\s+(.*))?$/.exec(s.rest);
    if (!m || !KEY_TYPE.test(m[1])) continue;
    let fingerprint = "";
    try {
      fingerprint = await fingerprintOf(m[2]);
    } catch {
      continue;
    }
    out.push({ line: i, raw, options: s.options, type: m[1], bits: parseSshPublicKey(`${m[1]} ${m[2]}`)?.bits ?? null, blob: m[2], comment: (m[3] ?? "").trim(), fingerprint });
  }
  return out;
}

export type KeyRisk = "ok" | "weak" | "legacy";

/** RSA under 3072 bits and DSA are the ones to replace. */
export function keyRisk(k: Pick<AuthorizedKey, "type" | "bits">): KeyRisk {
  if (k.type === "ssh-dss") return "legacy";
  if (k.type.startsWith("ssh-rsa") && k.bits !== null && k.bits < 3072) return "weak";
  return "ok";
}

export const isHardwareKey = (type: string) => type.startsWith("sk-");

/** A public key line as pasted or taken from the vault: one line, a known type, a base64 body. */
export function checkPublicKeyLine(line: string): string | null {
  const t = line.trim();
  if (/[\r\n]/.test(t)) return "A public key is one line.";
  const m = /^(\S+)\s+(\S+)(?:\s+(.*))?$/.exec(t);
  if (!m || !KEY_TYPE.test(m[1]) || !/^[A-Za-z0-9+/]+={0,2}$/.test(m[2])) return "That isn't a public key.";
  return null;
}

/** Where the file is, as shell text: the user's own, or root's (which takes sudo). */
export const keyFilePath = (who: "me" | "root") => (who === "root" ? "/root/.ssh/authorized_keys" : '"$HOME/.ssh/authorized_keys"');

export const READ_SCRIPT = (who: "me" | "root") => `cat ${keyFilePath(who)} 2>/dev/null || true`;

/** Utf-8 safe base64, for carrying a file's text inside a script. */
export function b64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

/**
 * Write the whole file: the directory at 700, the new text beside the old, mode 600, then moved over it. A copy
 * of the old file is kept next to it as `authorized_keys.sshvault-bak`. `expectedOld` guards against the file
 * having changed since it was read: the script stops if it differs.
 */
export function writeScript(who: "me" | "root", text: string, expectedOld: string): string {
  const path = keyFilePath(who);
  const body = text.endsWith("\n") || text === "" ? text : text + "\n";
  return [
    "set -e",
    `f=${path}`,
    'd=$(dirname "$f")',
    'mkdir -p "$d"; chmod 700 "$d"',
    '[ -e "$f" ] || : > "$f"',
    `old=$(base64 < "$f" | tr -d '\\n')`,
    `[ "$old" = ${shq(b64(expectedOld))} ] || { echo 'authorized_keys changed since it was read; reload and try again' >&2; exit 3; }`,
    'cp -p "$f" "$f.sshvault-bak"',
    `printf %s ${shq(b64(body))} | base64 -d > "$f.sshvault-new"`,
    'chmod 600 "$f.sshvault-new"',
    'mv "$f.sshvault-new" "$f"',
    'chmod 600 "$f"',
    // Root's file written by sudo stays root's; the user's own, the user's.
  ].join("; ");
}

/** The file's text with one key line added (unless it is already there). */
export function withKey(text: string, line: string): string {
  const key = line.trim();
  const body = text.replace(/\n*$/, "");
  const have = body.split(/\r?\n/).some((l) => l.trim() === key);
  return have ? text : (body ? body + "\n" : "") + key + "\n";
}

/** The file's text without the line that is exactly `raw`, at position `line`. */
export function withoutKey(text: string, k: Pick<AuthorizedKey, "line" | "raw">): string {
  const lines = text.split("\n");
  if (lines[k.line] !== k.raw) throw new Error("The file changed; reload it first.");
  lines.splice(k.line, 1);
  return lines.join("\n");
}
