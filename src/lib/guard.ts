// Best-effort safety checks on terminal input: confirming multi-line pastes
// and flagging destructive commands on production hosts. The app only sees
// keystrokes, not what the shell makes of them (history, completion,
// aliases, scripts), so this is a safety net and never a guarantee.

/** Commands a paste would submit: every line that ends in a line break, plus a final unterminated one. */
export function pastedLines(text: string): number {
  if (!text) return 0;
  const parts = text.split(/\r\n|\r|\n/);
  // "a\nb" = 2 lines; "a\n" = 1 line (submitted); "a" = 1 line (not submitted).
  return parts[parts.length - 1] === "" ? parts.length - 1 : parts.length;
}

/**
 * True when a paste should be confirmed first: it would submit `threshold`
 * or more commands, or (on production hosts) it would submit anything at
 * all. A paste with no line break never runs by itself. `threshold` 0 turns
 * the check off.
 */
export function pasteNeedsConfirm(text: string, threshold: number, production = false): boolean {
  if (threshold <= 0 || !/[\r\n]/.test(text)) return false;
  return production || pastedLines(text) >= threshold;
}

/** The first pattern `line` matches, or null. Invalid patterns are skipped. */
export function matchDestructive(line: string, patterns: string[]): string | null {
  const text = line.trim();
  if (!text) return null;
  for (const p of patterns) {
    try {
      // Accept the Rust/PCRE-style leading "(?i)" that JavaScript lacks.
      const re = p.startsWith("(?i)") ? new RegExp(p.slice(4), "i") : new RegExp(p);
      if (re.test(text)) return p;
    } catch {
      // A bad pattern in settings must not break typing.
    }
  }
  return null;
}

/**
 * A short reason to pause before pasting, if `text` looks like it contains
 * a private key or an API token rather than ordinary shell input. A
 * best-effort scan of recognisable formats, not a secret scanner: it won't
 * catch everything, and a false negative here is still just a paste.
 */
export function looksLikeSecret(text: string): string | null {
  if (/-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/.test(text)) return "This looks like it contains a private key.";
  if (/\bAKIA[0-9A-Z]{16}\b/.test(text)) return "This looks like it contains an AWS access key.";
  if (/\bgh[pousr]_[A-Za-z0-9]{36,}\b/.test(text)) return "This looks like it contains a GitHub token.";
  if (/\bxox[baprs]-[A-Za-z0-9-]{10,}\b/.test(text)) return "This looks like it contains a Slack token.";
  return null;
}

/**
 * The first line of `text` that matches a destructive pattern, with the
 * pattern, or null. Used for snippets and anything else sent as a whole.
 */
export function firstDestructiveLine(text: string, patterns: string[]): { line: string; pattern: string } | null {
  for (const line of text.split(/\r\n|\r|\n/)) {
    const pattern = matchDestructive(line, patterns);
    if (pattern) return { line: line.trim(), pattern };
  }
  return null;
}

/**
 * Follows what's typed on the current line. `line` is null once the line
 * can't be known reliably (history, completion, cursor movement).
 */
export class LineTracker {
  #buf = "";
  #known = true;

  get line(): string | null {
    return this.#known ? this.#buf : null;
  }

  /** Feed input the user typed, before Enter. */
  feed(data: string) {
    for (let i = 0; i < data.length; i++) {
      const c = data[i];
      const code = c.charCodeAt(0);
      if (c === "\r" || c === "\n") {
        this.reset();
      } else if (c === "\x7f" || c === "\b") {
        this.#buf = this.#buf.slice(0, -1);
      } else if (c === "\x15" || c === "\x03") {
        // Ctrl-U clears the line, Ctrl-C abandons it.
        this.reset();
      } else if (c === "\x1b") {
        // Arrow keys, history, word jumps: position is now unknown.
        this.#known = false;
        // Skip the rest of a CSI/SS3 sequence.
        if (data[i + 1] === "[" || data[i + 1] === "O") {
          i += 2;
          while (i < data.length && !/[@-~]/.test(data[i])) i++;
        }
      } else if (c === "\t" || code < 0x20) {
        // Completion and other control keys change the line in ways we can't see.
        this.#known = false;
      } else {
        this.#buf += c;
      }
    }
  }

  reset() {
    this.#buf = "";
    this.#known = true;
  }
}
