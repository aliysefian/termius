// What a typed command line is about to change on the host: the files an editor, `sed -i`, `tee` or a redirect will
// write, as absolute paths, and whether the Time-Machine is set to keep them. Pure text in, text out; the terminal
// calls it after Enter has already gone to the shell, so it can never delay a keystroke.

export interface EditCommand {
  /** `vim`, `sudoedit`, `sed -i`, `tee`, `>`: how it shows in the version's source. */
  program: string;
  /** Resolved paths: absolute, or `~/…` (the host expands that). */
  paths: string[];
  /** A relative path that could not be resolved because the shell's folder is not known. */
  unresolved: string[];
}

const EDITORS = new Set(["vim", "vi", "nvim", "nano", "pico", "micro", "emacs", "ed", "sudoedit", "joe", "mcedit", "kak", "hx", "helix"]);
/** Wrappers that run the next word as a command. */
const WRAPPERS = new Set(["sudo", "doas", "env", "command", "nohup", "time", "exec", "nice", "ionice", "stdbuf"]);
/** Wrapper options that take a value. */
const WRAPPER_VALUE_OPTS: Record<string, Set<string>> = {
  sudo: new Set(["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U"]),
  doas: new Set(["-u", "-C"]),
  env: new Set(["-u", "-C", "-S"]),
  nice: new Set(["-n"]),
  ionice: new Set(["-c", "-n", "-p"]),
};
/** Editor options that take a value, so it is not mistaken for a file. Options differ by editor: nano's `-c` is a flag, vim's takes a command. */
const VIM_VALUE_OPTS = new Set(["-c", "-S", "-u", "-i", "-T", "-s", "-w", "-W", "-t", "-q", "--cmd", "--startuptime", "-V"]);
const EDITOR_VALUE_OPTS: Record<string, Set<string>> = {
  vim: VIM_VALUE_OPTS,
  vi: VIM_VALUE_OPTS,
  nvim: VIM_VALUE_OPTS,
  nano: new Set(["-r", "-T", "-Y", "-o", "-Q", "-X", "-I", "-s", "-U"]),
  emacs: new Set(["-f", "-l", "--eval", "--load", "-u", "--funcall", "--directory", "-L"]),
  micro: new Set(["-config-dir", "-options", "-plugin"]),
};

type Quote = "" | "'" | '"';

/** Split a command line into simple commands: the words, and the files written by `>` and `>>`. A word with an expansion
 * we cannot know (`$VAR`, `$(…)`, a glob) is dropped rather than guessed. */
export function parseLine(line: string): { words: string[]; redirects: string[] }[] {
  const commands: { words: string[]; redirects: string[] }[] = [];
  let words: string[] = [];
  let redirects: string[] = [];
  let cur = "";
  let has = false;
  let tainted = false;
  let q: Quote = "";
  /** What the next word is: a command word, the file of `>`, or something to ignore (`< input`, `>&2`). */
  let next: "word" | "write" | "skip" = "word";
  const endWord = () => {
    if (!has) return;
    if (!tainted) {
      if (next === "write") redirects.push(cur);
      else if (next === "word") words.push(cur);
    }
    next = "word";
    cur = "";
    has = false;
    tainted = false;
  };
  const endCommand = () => {
    endWord();
    next = "word";
    if (words.length || redirects.length) commands.push({ words, redirects });
    words = [];
    redirects = [];
  };
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q === "'") {
      if (c === "'") q = "";
      else cur += c;
      continue;
    }
    if (q === '"') {
      if (c === '"') q = "";
      else if (c === "\\" && /["\\$`]/.test(line[i + 1] ?? "")) cur += line[++i];
      else {
        if (c === "$" || c === "`") tainted = true;
        cur += c;
      }
      continue;
    }
    if (c === "'" || c === '"') {
      q = c;
      has = true;
    } else if (c === "\\") {
      cur += line[++i] ?? "";
      has = true;
    } else if (/\s/.test(c)) {
      endWord();
    } else if (c === ">") {
      // `2>` is the digit of a file descriptor, not a word.
      if (has && /^\d+$/.test(cur) && !tainted) {
        cur = "";
        has = false;
      }
      endWord();
      while (line[i + 1] === ">") i++;
      // `>&2` and `&>` name no file we can keep.
      if (line[i + 1] === "&") {
        i++;
        next = "skip";
      } else next = "write";
    } else if (c === "<") {
      endWord();
      next = "skip";
    } else if (c === ";" || c === "&" || c === "|" || c === "(" || c === ")") {
      // `&&`, `||`, `|&`, `;` and a lone `&` each end a command.
      endCommand();
    } else {
      if (c === "$" || c === "`" || c === "*" || c === "?" || c === "[" || c === "{") tainted = true;
      cur += c;
      has = true;
    }
  }
  endCommand();
  return commands;
}

/** `a/./b/../c` to `a/c`; keeps a leading `/` or `~/`. */
export function normalizePath(p: string): string {
  const root = p.startsWith("/") ? "/" : p.startsWith("~/") ? "~/" : "";
  const out: string[] = [];
  for (const seg of p.slice(root.length).split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (out.length) out.pop();
    } else out.push(seg);
  }
  return root + out.join("/");
}

/** An absolute path, or `~/…`, from what was typed and the shell's folder; null when it cannot be told. */
export function resolvePath(arg: string, cwd: string | null): string | null {
  if (!arg || arg === "-") return null;
  if (arg === "~") return null;
  if (arg.startsWith("/") || arg.startsWith("~/")) return normalizePath(arg);
  if (arg.startsWith("~")) return null;
  if (!cwd || !cwd.startsWith("/")) return null;
  return normalizePath(`${cwd.replace(/\/+$/, "")}/${arg}`);
}

function baseName(p: string): string {
  return p.slice(p.lastIndexOf("/") + 1);
}

/** Strip assignments and wrappers (`FOO=1 sudo -u root nice vim x`) to the program and its arguments; `sudo -e` is sudoedit. */
function unwrap(words: string[]): { program: string; args: string[] } | null {
  let i = 0;
  let sudoE = false;
  while (i < words.length) {
    const w = words[i];
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(w)) {
      i++;
      continue;
    }
    const name = baseName(w);
    if (!WRAPPERS.has(name)) break;
    i++;
    const valueOpts = WRAPPER_VALUE_OPTS[name];
    while (i < words.length && words[i].startsWith("-") && words[i] !== "--") {
      const o = words[i++];
      if (name === "sudo" && (o === "-e" || o === "--edit")) sudoE = true;
      if (valueOpts?.has(o)) i++;
    }
    if (words[i] === "--") i++;
    // env's own NAME=value words
    while (name === "env" && i < words.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i])) i++;
  }
  if (i >= words.length) return null;
  // `sudo -e` takes the files where a command would be.
  return sudoE ? { program: "sudoedit", args: words.slice(i) } : { program: baseName(words[i]), args: words.slice(i + 1) };
}

/** The non-option words of an argument list, skipping the values of options that take one. */
function operands(args: string[], valueOpts: Set<string>): string[] {
  const out: string[] = [];
  let endOfOptions = false;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (!endOfOptions && a === "--") {
      endOfOptions = true;
      continue;
    }
    if (!endOfOptions && a.startsWith("--")) {
      if (valueOpts.has(a)) i++;
      continue;
    }
    if (!endOfOptions && a.startsWith("-") && a.length > 1) {
      if (valueOpts.has(a)) i++;
      continue;
    }
    // vim's `+42` and `+/pattern`
    if (!endOfOptions && /^\+/.test(a)) continue;
    out.push(a);
  }
  return out;
}

const SED_VALUE_OPTS = new Set(["-e", "-f", "--expression", "--file", "-l", "--line-length", "-s"]);

/** Whether a `sed` option cluster asks for in-place editing: `-i`, `-i.bak`, `-ni`, `--in-place`. */
function sedInPlace(args: string[]): boolean {
  return args.some((a) => a === "--in-place" || a.startsWith("--in-place=") || (/^-[A-Za-z]+/.test(a) && !a.startsWith("--") && /^-[A-Za-z]*i/.test(a)));
}

/** Every file the line will change, per simple command. */
export function detectEdits(line: string, cwd: string | null): EditCommand[] {
  const found: EditCommand[] = [];
  for (const cmd of parseLine(line)) {
    const u = unwrap(cmd.words);
    let program = "";
    let files: string[] = [];
    if (u) {
      if (EDITORS.has(u.program)) {
        program = u.program;
        files = operands(u.args, EDITOR_VALUE_OPTS[u.program] ?? new Set());
      } else if (u.program === "sed" && sedInPlace(u.args)) {
        program = "sed -i";
        const ops = operands(u.args, SED_VALUE_OPTS);
        // Without -e or -f the first operand is the script, not a file.
        const hasScriptOpt = u.args.some((a) => a === "-e" || a === "-f" || a.startsWith("--expression") || a.startsWith("--file") || /^-[A-Za-z]*[ef]$/.test(a));
        files = hasScriptOpt ? ops : ops.slice(1);
      } else if (u.program === "tee") {
        program = "tee";
        files = operands(u.args, new Set());
      }
    }
    if (!program && cmd.redirects.length) {
      program = ">";
    }
    // `echo x > /etc/hosts`: whatever the command is, a redirect writes the file.
    if (!program && cmd.redirects.length) program = ">";
    const targets = [...files, ...cmd.redirects];
    if (!program || targets.length === 0) continue;
    const paths: string[] = [];
    const unresolved: string[] = [];
    for (const f of targets) {
      const p = resolvePath(f, cwd);
      if (p && !p.startsWith("/dev/")) paths.push(p);
      else if (f && f !== "-" && f !== "/dev/null") unresolved.push(f);
    }
    if (paths.length || unresolved.length) found.push({ program, paths: [...new Set(paths)], unresolved });
  }
  return found;
}

// -- which paths are watched --------------------------------------------------------------------------------------

// A glob as a regular expression: a star and a question mark stay inside a folder, two stars cross folders,
// and two stars followed by a slash may match nothing at all.
export function globToRegExp(glob: string): RegExp {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*") {
      if (glob[i + 1] === "*") {
        i++;
        if (glob[i + 1] === "/") {
          i++;
          re += "(?:.*/)?";
        } else re += ".*";
      } else re += "[^/]*";
    } else if (c === "?") re += "[^/]";
    else re += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`);
}

const cache = new Map<string, RegExp>();

export function matchesWatched(path: string, patterns: string[]): boolean {
  return patterns.some((p) => {
    const g = p.trim();
    if (!g) return false;
    let re = cache.get(g);
    if (!re) cache.set(g, (re = globToRegExp(g)));
    return re.test(path);
  });
}

/** A pattern list from a text area: one per line or comma, blanks and duplicates gone. */
export function parsePatterns(text: string): string[] {
  return [...new Set(text.split(/[\n,]/).map((s) => s.trim()).filter(Boolean))];
}
