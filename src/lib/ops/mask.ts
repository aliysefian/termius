// Hiding secrets in text that is about to be shown for sharing or kept: log lines, exports. The same rules as
// src-tauri/src/mask.rs (which hides them in a run's saved record), and tested against the same cases.
//
// A safety net, not a guarantee: it recognises the common shapes (private key blocks, `password=...`,
// `Authorization: Bearer ...`, well-known token prefixes, `user:pass@` in URLs) and leaves other text alone.

export const HIDDEN = "[hidden]";

const NAMES = ["password", "passwd", "passphrase", "secret", "token", "api_key", "api-key", "apikey", "access_key", "private_key", "authorization", "credential"];

/** Token prefixes and the shortest total length that makes a hit (so words like "sk-learn" stay). */
const PREFIXES: [string, number][] = [["ghp_", 24], ["gho_", 24], ["github_pat_", 30], ["xoxb-", 20], ["xoxp-", 20], ["xoxa-", 20], ["sk-", 24]];

/** Whether a field called `name` is likely to hold a secret. */
export const isSecretName = (name: string) => NAMES.some((w) => name.toLowerCase().includes(w));

/** Masks lines one at a time, remembering whether it is inside a private key block that spans lines. */
export class SecretMasker {
  #inKey = false;

  /** The masked line, or null when the line is part of a private key block and is dropped. */
  line(text: string): string | null {
    if (this.#inKey) {
      if (text.includes("-----END ")) this.#inKey = false;
      return null;
    }
    if (text.includes("-----BEGIN ") && text.includes("PRIVATE KEY")) {
      this.#inKey = !text.includes("-----END ");
      return "[private key hidden]";
    }
    return maskLine(text);
  }
}

/** Masks a whole text. A private key block becomes the single line `[private key hidden]`. */
export function maskSecrets(text: string): string {
  const masker = new SecretMasker();
  const out: string[] = [];
  for (const part of text.split("\n")) {
    const cr = part.endsWith("\r");
    const masked = masker.line(cr ? part.slice(0, -1) : part);
    if (masked !== null) out.push(masked + (cr ? "\r" : ""));
  }
  return out.join("\n");
}

function maskLine(line: string): string {
  return maskPrefixedTokens(maskNamedValues(maskUrlPasswords(line)));
}

function maskUrlPasswords(line: string): string {
  let out = "";
  let rest = line;
  for (;;) {
    const i = rest.indexOf("://");
    if (i < 0) break;
    out += rest.slice(0, i + 3);
    const tail = rest.slice(i + 3);
    const stop = /[\s/"']/.exec(tail);
    const end = stop ? stop.index : tail.length;
    const authority = tail.slice(0, end);
    const at = authority.lastIndexOf("@");
    const colon = at >= 0 ? authority.slice(0, at).indexOf(":") : -1;
    out += colon >= 0 ? authority.slice(0, colon + 1) + HIDDEN + authority.slice(at) : authority;
    rest = tail.slice(end);
  }
  return out + rest;
}

const isValueChar = (c: string) => !/[\s"'&,;})<>]/.test(c);
const isNameChar = (c: string) => /[A-Za-z0-9_.-]/.test(c);

function maskNamedValues(line: string): string {
  const lower = line.replace(/[A-Z]/g, (c) => c.toLowerCase());
  let out = "";
  let pos = 0;
  while (pos < line.length) {
    let at = -1;
    let word = "";
    for (const w of NAMES) {
      const i = lower.indexOf(w, pos);
      if (i >= 0 && (at < 0 || i < at)) {
        at = i;
        word = w;
      }
    }
    if (at < 0) break;
    let i = at + word.length;
    while (i < line.length && isNameChar(line[i])) i++;
    let j = i;
    while (j < line.length && (line[j] === '"' || line[j] === "'")) j++;
    while (j < line.length && (line[j] === " " || line[j] === "\t")) j++;
    if (line[j] !== "=" && line[j] !== ":") {
      out += line.slice(pos, i);
      pos = i;
      continue;
    }
    j++;
    while (j < line.length && " \t\"'".includes(line[j])) j++;
    for (const scheme of ["bearer ", "basic ", "token "]) if (lower.startsWith(scheme, j)) j += scheme.length;
    let end = j;
    while (end < line.length && isValueChar(line[end])) end++;
    out += line.slice(pos, j);
    if (end > j) out += HIDDEN;
    pos = end;
  }
  return out + line.slice(pos);
}

function maskPrefixedTokens(line: string): string {
  let out = "";
  let i = 0;
  outer: while (i < line.length) {
    const rest = line.slice(i);
    for (const [prefix, min] of PREFIXES) {
      if (rest.startsWith(prefix)) {
        let len = 0;
        while (len < rest.length && /[A-Za-z0-9_-]/.test(rest[len])) len++;
        if (len >= min) {
          out += HIDDEN;
          i += len;
          continue outer;
        }
      }
    }
    if ((rest.startsWith("AKIA") || rest.startsWith("ASIA")) && /^[A-Z0-9]{16}$/.test(rest.slice(4, 20))) {
      out += HIDDEN;
      i += 20;
      continue;
    }
    out += line[i];
    i++;
  }
  return out;
}
