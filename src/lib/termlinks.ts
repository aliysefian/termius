// Pure matchers for the terminal's extra clickable links: absolute/relative
// paths and `host:port` pairs. Kept separate from TerminalPane so the
// regexes (the part most likely to need a tweak later) are unit-testable
// without a live xterm instance.

export interface TextMatch {
  start: number;
  end: number;
  text: string;
}

const TRAILING_PUNCTUATION = /[.,;:!?)]+$/;

/** `/abs/path`, `~/path`, `./rel` or `../rel`, each at least one segment deep. */
const PATH_RE = /(?:~\/|\.\.?\/)[\w.\-/]*[\w.\-]|\/[\w.\-]+(?:\/[\w.\-]+)+/g;

export function findPathMatches(line: string): TextMatch[] {
  const out: TextMatch[] = [];
  for (const m of line.matchAll(PATH_RE)) {
    const start = m.index!;
    // Not the tail of a URL's scheme (http://, ssh://, ...): either right
    // after the colon, or right after the second slash of "//".
    if (line[start - 1] === ":" || line[start - 1] === "/") continue;
    const trimmed = m[0].replace(TRAILING_PUNCTUATION, "");
    if (!trimmed) continue;
    out.push({ start, end: start + trimmed.length, text: trimmed });
  }
  return out;
}

const HOST_PORT_RE = /\b((?:\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})|(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+)):([0-9]{1,5})\b/g;

/**
 * Resolves a matched path against the pane's known directory (from OSC 7),
 * then drops the last segment: SFTP browses to a folder, and a file's
 * folder is the next most useful place to land, whether the match turns
 * out to be a file or (close enough) a directory.
 */
export function resolveBrowsePath(match: string, cwd: string | undefined): string {
  let abs: string;
  if (match.startsWith("/")) {
    abs = match;
  } else if (match.startsWith("~/")) {
    // The remote home directory isn't known from OSC 7 (it reports the
    // current directory, not home); pass it through and let SFTP resolve
    // or report it, same as typing it there directly.
    abs = match;
  } else if (cwd) {
    const parts = cwd.split("/").filter(Boolean);
    for (const seg of match.split("/")) {
      if (seg === "." || seg === "") continue;
      if (seg === "..") parts.pop();
      else parts.push(seg);
    }
    abs = `/${parts.join("/")}`;
  } else {
    abs = match; // No known cwd either: pass it through as typed.
  }
  if (!abs.includes("/")) return abs;
  const parent = abs.slice(0, abs.lastIndexOf("/"));
  return parent || "/";
}

export function findHostPortMatches(line: string): TextMatch[] {
  const out: TextMatch[] = [];
  for (const m of line.matchAll(HOST_PORT_RE)) {
    const port = Number(m[2]);
    if (port < 1 || port > 65535) continue;
    const start = m.index!;
    // Not the authority part of a URL (scheme://host:port).
    if (line[start - 1] === "/" && line[start - 2] === "/") continue;
    out.push({ start, end: start + m[0].length, text: m[0] });
  }
  return out;
}
